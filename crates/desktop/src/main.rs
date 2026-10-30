#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pane;
mod view;

use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};

use claude_sessions_core::attach::attach_command;
use claude_sessions_core::{accounts, refresher};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use crate::pane::Pane;
use crate::view::ListView;

/// The session shown on the right. `generation` changes with every session opened, so the
/// page can ignore the end of a session it no longer shows.
#[derive(Default)]
struct PaneSlot {
    generation: u64,
    pane: Option<Pane>,
}

struct Shared {
    view: Mutex<ListView>,
    /// Problems found at startup, shown above every list since a refresh cannot fix them.
    startup_errors: Vec<String>,
    slot: Mutex<PaneSlot>,
}

impl Shared {
    fn slot(&self) -> std::sync::MutexGuard<'_, PaneSlot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[tauri::command]
fn current_list(shared: State<'_, Shared>) -> ListView {
    shared.view.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Replaces the session on the right; the one shown before is detached, not stopped.
#[tauri::command]
fn open_session(
    app: AppHandle,
    shared: State<'_, Shared>,
    account: String,
    short_id: String,
    columns: u16,
    rows: u16,
    output: Channel<InvokeResponseBody>,
) -> Result<u64, String> {
    let mut slot = shared.slot();
    slot.pane = None;
    slot.generation += 1;
    let generation = slot.generation;
    let pane = Pane::open(
        &attach_command(&account, &short_id),
        columns,
        rows,
        move |bytes| {
            // Fails only once the page has dropped this channel for a newer session.
            let _ = output.send(InvokeResponseBody::Raw(bytes.to_vec()));
        },
        move || {
            let _ = app.emit("pane-ended", generation);
        },
    )
    .map_err(|error| format!("cannot open {short_id}: {error}"))?;
    slot.pane = Some(pane);
    Ok(generation)
}

#[tauri::command]
fn write_input(shared: State<'_, Shared>, data: String) -> Result<(), String> {
    let mut slot = shared.slot();
    let pane = slot.pane.as_mut().ok_or("no session is open")?;
    pane.write(data.as_bytes()).map_err(|error| error.to_string())
}

#[tauri::command]
fn resize_pane(shared: State<'_, Shared>, columns: u16, rows: u16) -> Result<(), String> {
    let slot = shared.slot();
    let pane = slot.pane.as_ref().ok_or("no session is open")?;
    pane.resize(columns, rows).map_err(|error| error.to_string())
}

fn publish(app: &AppHandle, update: refresher::Update) -> bool {
    let shared = app.state::<Shared>();
    let view = {
        let mut current = shared.view.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = view::apply(&current, update);
        next.errors.splice(0..0, shared.startup_errors.iter().cloned());
        *current = next.clone();
        next
    };
    app.emit("sessions", &view).is_ok()
}

/// Leaves no `claude attach` behind when the window closes.
fn detach_on_exit(app: &AppHandle, event: RunEvent) {
    if let RunEvent::Exit = event {
        app.state::<Shared>().slot().pane = None;
    }
}

fn main() -> ExitCode {
    let startup_errors = accounts::check_launcher_version().err().map(|error| error.to_string()).into_iter().collect();
    let shared = Shared { view: Mutex::new(ListView::default()), startup_errors, slot: Mutex::default() };
    let built = tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![current_list, open_session, write_input, resize_pane])
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
