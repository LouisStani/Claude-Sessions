#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod view;

use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};

use claude_sessions_core::{accounts, refresher};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::view::ListView;

struct Shared {
    view: Mutex<ListView>,
    /// Problems found at startup, shown above every list since a refresh cannot fix them.
    startup_errors: Vec<String>,
}

#[tauri::command]
fn current_list(shared: State<'_, Shared>) -> ListView {
    shared.view.lock().unwrap_or_else(PoisonError::into_inner).clone()
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

fn main() -> ExitCode {
    let startup_errors = accounts::check_launcher_version().err().map(|error| error.to_string()).into_iter().collect();
    let shared = Shared { view: Mutex::new(ListView::default()), startup_errors };
    let result = tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![current_list])
        .setup(|app| {
            let handle = app.handle().clone();
            refresher::spawn(move |update| publish(&handle, update));
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("claude-sessions-desktop: {error}");
            ExitCode::FAILURE
        },
    }
}
