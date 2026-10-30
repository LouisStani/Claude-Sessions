#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod layout;
mod pane;
mod projects;
mod tabs;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Mutex, MutexGuard, PoisonError};

use claude_sessions_core::attach::attach_command;
use claude_sessions_core::launcher::{self, Request, resolve_directory};
use claude_sessions_core::{accounts, mailbox, refresher};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use crate::layout::{PaneId, SessionKey, Shown};
use crate::pane::Pane;
use crate::projects::Project;
use crate::tabs::Tabs;
use crate::view::{ListView, SessionRow};

/// The tabs, their panes and the sessions attached in them. `generation` changes with every attach, so
/// the end of a session that was since replaced in its pane is ignored.
#[derive(Default)]
struct Workspace {
    tabs: Tabs,
    generation: u64,
    attached: HashMap<PaneId, (u64, Pane)>,
}

struct Shared {
    view: Mutex<ListView>,
    /// Problems found at startup, shown above every list since a refresh cannot fix them.
    startup_errors: Vec<String>,
    workspace: Mutex<Workspace>,
    /// Where the projects are kept, or why they cannot be: a file that did not load is
    /// never written over.
    projects_file: Result<PathBuf, String>,
}

impl Shared {
    fn workspace(&self) -> MutexGuard<'_, Workspace> {
        self.workspace.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[tauri::command]
fn current_list(shared: State<'_, Shared>) -> ListView {
    shared.view.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[tauri::command]
fn filter_sessions(shared: State<'_, Shared>, query: String) -> Vec<SessionRow> {
    view::filter(&shared.view.lock().unwrap_or_else(PoisonError::into_inner), &query)
}

#[derive(Serialize)]
struct Created {
    account: String,
    short_id: String,
    warnings: Vec<String>,
}

fn start_session(account: String, directory: &str, name: &str) -> Result<Created, String> {
    let home = std::env::home_dir().ok_or("HOME is not set")?;
    let request = Request { account, directory: resolve_directory(directory, &home)?, name: name.trim().to_string() };
    let launched = launcher::launch(&request)?;
    Ok(Created { account: request.account, short_id: launched.short_id, warnings: launched.warnings })
}

/// Starting a session takes a few seconds: it runs off the window's thread.
#[tauri::command]
async fn new_session(account: String, directory: String, name: String) -> Result<Created, String> {
    tauri::async_runtime::spawn_blocking(move || start_session(account, &directory, &name))
        .await
        .map_err(|error| error.to_string())?
}

#[derive(Clone, Serialize)]
struct Ended {
    pane: PaneId,
    generation: u64,
}

/// What `open_session` did. `generation` is None when the session was already shown: the
/// pane showing it now has the focus.
#[derive(Serialize)]
struct Opened {
    tabs: Tabs,
    generation: Option<u64>,
}

#[tauri::command]
fn tabs(shared: State<'_, Shared>) -> Tabs {
    shared.workspace().tabs.clone()
}

/// Ends every command that may move the focus: the session now looked at loses its
/// "finished" mark. The two locks are never held together.
fn respond(app: &AppHandle, shared: &Shared, workspace: MutexGuard<'_, Workspace>) -> Tabs {
    let tabs = workspace.tabs.clone();
    drop(workspace);
    if let Some(key) = tabs.watched() {
        let changed = {
            let mut view = shared.view.lock().unwrap_or_else(PoisonError::into_inner);
            view::seen(&mut view, key).then(|| view.clone())
        };
        if let Some(view) = changed {
            let _ = app.emit("sessions", &view);
        }
    }
    tabs
}

#[tauri::command]
fn split_pane(app: AppHandle, shared: State<'_, Shared>, width: f64, height: f64) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.split(width, height);
    respond(&app, &shared, workspace)
}

#[tauri::command]
fn close_pane(app: AppHandle, shared: State<'_, Shared>) -> Tabs {
    let mut workspace = shared.workspace();
    if let Some(closed) = workspace.tabs.current_mut().close() {
        // Detaching only: the background session keeps running in Claude Code.
        workspace.attached.remove(&closed);
    }
    respond(&app, &shared, workspace)
}

#[tauri::command]
fn focus_pane(app: AppHandle, shared: State<'_, Shared>, pane: PaneId) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.current_mut().focus(pane);
    respond(&app, &shared, workspace)
}

#[tauri::command]
fn focus_step(app: AppHandle, shared: State<'_, Shared>, step: i64) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.current_mut().focus_step(step);
    respond(&app, &shared, workspace)
}

#[tauri::command]
fn resize_split(shared: State<'_, Shared>, split: PaneId, ratio: f32) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.current_mut().set_ratio(split, ratio);
    workspace.tabs.clone()
}

fn save_projects(shared: &Shared, tabs: &Tabs) -> Result<(), String> {
    let path = shared.projects_file.as_ref()?;
    projects::save(path, &tabs.projects()).map_err(|error| format!("cannot save {}: {error}", path.display()))
}

/// The projects changed: every session goes to its tab again.
fn reassign(app: &AppHandle, shared: &Shared, tabs: &Tabs) {
    let view = {
        let mut view = shared.view.lock().unwrap_or_else(PoisonError::into_inner);
        view::assign(&mut view, tabs);
        view.clone()
    };
    let _ = app.emit("sessions", &view);
}

/// A tab for the folder typed in the page, kept in `projects.json`.
#[tauri::command]
fn add_project(app: AppHandle, shared: State<'_, Shared>, directory: String) -> Result<Tabs, String> {
    let home = std::env::home_dir().ok_or("HOME is not set")?;
    let directory = resolve_directory(&directory, &home)?;
    let mut workspace = shared.workspace();
    let mut next = workspace.tabs.clone();
    next.add_project(Project::new(directory))?;
    save_projects(&shared, &next)?;
    workspace.tabs = next;
    let tabs = respond(&app, &shared, workspace);
    reassign(&app, &shared, &tabs);
    Ok(tabs)
}

/// Forgets the project of the active tab. Detaching only: its sessions keep running.
#[tauri::command]
fn close_tab(app: AppHandle, shared: State<'_, Shared>) -> Result<Tabs, String> {
    let mut workspace = shared.workspace();
    let mut next = workspace.tabs.clone();
    let Some(panes) = next.close_tab() else {
        return Err("the all tab stays".into());
    };
    save_projects(&shared, &next)?;
    workspace.tabs = next;
    for pane in panes {
        workspace.attached.remove(&pane);
    }
    let tabs = respond(&app, &shared, workspace);
    reassign(&app, &shared, &tabs);
    Ok(tabs)
}

#[tauri::command]
fn tab_step(app: AppHandle, shared: State<'_, Shared>, step: i64) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.step(step);
    respond(&app, &shared, workspace)
}

#[tauri::command]
fn go_tab(app: AppHandle, shared: State<'_, Shared>, number: usize) -> Tabs {
    let mut workspace = shared.workspace();
    workspace.tabs.go(number);
    respond(&app, &shared, workspace)
}

/// A session ended by itself: its pane is emptied, unless another session was opened there since.
fn end_session(app: &AppHandle, pane: PaneId, generation: u64) {
    let shared = app.state::<Shared>();
    let tabs = {
        let mut workspace = shared.workspace();
        if workspace.attached.get(&pane).map(|(current, _)| *current) != Some(generation) {
            return;
        }
        workspace.attached.remove(&pane);
        workspace.tabs.clear(pane);
        workspace.tabs.clone()
    };
    let _ = app.emit("pane-ended", Ended { pane, generation });
    let _ = app.emit("tabs", &tabs);
}

/// Shows a session in `pane`, unless another pane, in any tab, already shows it. The session shown
/// there before is detached, not stopped.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn open_session(
    app: AppHandle,
    shared: State<'_, Shared>,
    pane: PaneId,
    account: String,
    short_id: String,
    columns: u16,
    rows: u16,
    output: Channel<InvokeResponseBody>,
) -> Result<Opened, String> {
    let mut workspace = shared.workspace();
    let key = SessionKey { account: account.clone(), short_id: short_id.clone() };
    match workspace.tabs.show(pane, key) {
        None => return Err(format!("no pane {pane}")),
        Some(Shown::Already(_)) => return Ok(Opened { tabs: respond(&app, &shared, workspace), generation: None }),
        Some(Shown::Attach) => {},
    }
    workspace.attached.remove(&pane);
    workspace.generation += 1;
    let generation = workspace.generation;
    let ended = app.clone();
    let opened = Pane::open(
        &attach_command(&account, &short_id),
        columns,
        rows,
        move |bytes| {
            // Fails only once the page has dropped this channel for a newer session.
            let _ = output.send(InvokeResponseBody::Raw(bytes.to_vec()));
        },
        // Waits for the lock held here, so it always sees this attach recorded.
        move || end_session(&ended, pane, generation),
    );
    match opened {
        Ok(attached) => {
            workspace.attached.insert(pane, (generation, attached));
            Ok(Opened { tabs: respond(&app, &shared, workspace), generation: Some(generation) })
        },
        Err(error) => {
            workspace.tabs.clear(pane);
            Err(format!("cannot open {short_id}: {error}"))
        },
    }
}

#[tauri::command]
fn write_input(shared: State<'_, Shared>, pane: PaneId, data: String) -> Result<(), String> {
    let mut workspace = shared.workspace();
    let (_, attached) = workspace.attached.get_mut(&pane).ok_or("no session in this pane")?;
    attached.write(data.as_bytes()).map_err(|error| error.to_string())
}

#[tauri::command]
fn resize_pane(shared: State<'_, Shared>, pane: PaneId, columns: u16, rows: u16) -> Result<(), String> {
    let workspace = shared.workspace();
    let (_, attached) = workspace.attached.get(&pane).ok_or("no session in this pane")?;
    attached.resize(columns, rows).map_err(|error| error.to_string())
}

fn publish(app: &AppHandle, update: refresher::Update) -> bool {
    let shared = app.state::<Shared>();
    let tabs = shared.workspace().tabs.clone();
    let view = {
        let mut current = shared.view.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = view::apply(&current, update, tabs.watched());
        view::assign(&mut next, &tabs);
        next.errors.splice(0..0, shared.startup_errors.iter().cloned());
        *current = next.clone();
        next
    };
    app.emit("sessions", &view).is_ok()
}

/// Leaves no `claude attach` behind when the window closes.
fn detach_on_exit(app: &AppHandle, event: RunEvent) {
    if let RunEvent::Exit = event {
        app.state::<Shared>().workspace().attached.clear();
    }
}

/// The tabs of the projects kept in `projects.json`, and where to keep them.
fn load_projects(errors: &mut Vec<String>) -> (Tabs, Result<PathBuf, String>) {
    let path = match mailbox::data_dir() {
        Ok(directory) => directory.join("projects.json"),
        Err(error) => {
            errors.push(format!("projects unavailable: {error}"));
            return (Tabs::default(), Err(format!("projects unavailable: {error}")));
        },
    };
    match projects::load(&path) {
        Ok(projects) => (Tabs::with_projects(projects), Ok(path)),
        Err(error) => {
            errors.push(format!("projects not loaded: {error}"));
            (Tabs::default(), Err(format!("{} did not load: fix or delete it", path.display())))
        },
    }
}

fn main() -> ExitCode {
    let mut startup_errors: Vec<String> = accounts::check_launcher_version().err().map(|error| error.to_string()).into_iter().collect();
    let (restored, projects_file) = load_projects(&mut startup_errors);
    let workspace = Workspace { tabs: restored, ..Workspace::default() };
    let shared = Shared { view: Mutex::new(ListView::default()), startup_errors, workspace: Mutex::new(workspace), projects_file };
    let built = tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            current_list,
            filter_sessions,
            new_session,
            tabs,
            split_pane,
            close_pane,
            focus_pane,
            focus_step,
            resize_split,
            add_project,
            close_tab,
            tab_step,
            go_tab,
            open_session,
            write_input,
            resize_pane
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            refresher::spawn(move |update| publish(&handle, update));
            Ok(())
        })
        .build(tauri::generate_context!());
    match built {
        Ok(app) => {
            app.run(detach_on_exit);
            ExitCode::SUCCESS
        },
        Err(error) => {
            eprintln!("claude-sessions-desktop: {error}");
            ExitCode::FAILURE
        },
    }
}
