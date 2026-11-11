//! Minimal native transport for the source-preserving vault service.
//!
//! All backend work runs away from Tauri's async/UI executor. The frontend can
//! request storage work through `vault_request` and binary playback through
//! `emulator_start`; it receives no filesystem or shell capabilities.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{
    ipc::{Channel, Response},
    Emitter, Manager, State,
};
use universal_pokemon_vault::desktop_service::DesktopService;

/// Dispatch one documented desktop API request without blocking Tauri's thread.
#[tauri::command]
async fn vault_request(
    request: Value,
    state: State<'_, Arc<Mutex<DesktopService>>>,
) -> Result<Value, String> {
    let service = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = service
            .lock()
            .map_err(|error| format!("vault service lock failed: {error}"))?;
        service
            .dispatch(request)
            .map_err(|error| format!("vault request failed: {error:#}"))
    })
    .await
    .map_err(|error| format!("vault worker failed: {error}"))?
}

/// Raw channel payloads reach JavaScript as ArrayBuffer, avoiding JSON arrays
/// for every video/audio sample. The backend bounds unacknowledged packets.
#[tauri::command(rename_all = "snake_case")]
async fn emulator_start(
    save_id: i64,
    core_path: String,
    on_frame: Channel<Response>,
    state: State<'_, Arc<Mutex<DesktopService>>>,
) -> Result<Value, String> {
    let service = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        service
            .lock()
            .map_err(|error| error.to_string())?
            .start_emulator(
                save_id,
                std::path::Path::new(&core_path),
                Box::new(move |packet| on_frame.send(Response::new(packet)).map_err(Into::into)),
            )
            .map_err(|error| format!("emulator start failed: {error:#}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Accept only the explicit local workspace override and explain all other args.
fn selected_workspace() -> Result<Option<PathBuf>, String> {
    let mut arguments = std::env::args_os().skip(1);
    let mut workspace = None;
    while let Some(argument) = arguments.next() {
        if argument == "--workspace" {
            if workspace.is_some() {
                return Err("--workspace may be specified only once".to_owned());
            }
            let Some(path) = arguments.next() else {
                return Err("--workspace requires a path".to_owned());
            };
            workspace = Some(PathBuf::from(path));
        } else {
            return Err(format!(
                "unsupported argument {:?}; supported argument: --workspace PATH",
                argument
            ));
        }
    }
    Ok(workspace)
}

/// Build the service once from the explicit workspace or Tauri app data dir.
fn main() {
    let workspace = selected_workspace().unwrap_or_else(|error| {
        eprintln!("pokemon-vault-desktop: {error}");
        std::process::exit(2);
    });

    tauri::Builder::default()
        // The picker is limited to user-selected input files; no other plugins
        // are installed, and the frontend cannot choose output destinations.
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            let workspace_path = workspace.unwrap_or(data_dir);
            std::fs::create_dir_all(&workspace_path)?;
            let service = DesktopService::open(&workspace_path)?;
            app.manage(Arc::new(Mutex::new(service)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![vault_request, emulator_start])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // This responds to the user's real Close action, never focuses
                // a window or synthesizes input. Native joining/file I/O stays
                // off the GUI executor, and failures keep the recovery UI alive.
                api.prevent_close();
                let window = window.clone();
                let service = window.state::<Arc<Mutex<DesktopService>>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        service
                            .lock()
                            .map_err(|error| error.to_string())?
                            .shutdown_emulator()
                            .map_err(|error| format!("Stop & save before closing: {error:#}"))
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => {
                            let _ = window.destroy();
                        }
                        outcome => {
                            let _ = window.emit("emulator-close-error", format!("{outcome:?}"));
                        }
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Universal Pokemon Vault desktop shell");
}
