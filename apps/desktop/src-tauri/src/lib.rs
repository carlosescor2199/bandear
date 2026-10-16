//! The MixLink desktop shell.
//!
//! This crate is deliberately thin: it owns the window and the JSON contract, and delegates every
//! engine call to [commands]. The engine lives in `personal-monitoring` and knows nothing of Tauri,
//! so the headless binary does not inherit this dependency stack.

pub mod commands;
pub mod dto;
pub mod events;
pub mod session;

use commands::DesktopState;
use dto::{EngineStateDto, InputDeviceDto, MixDto, MixUpdateRequest, StartRequest, StartSummary};
use events::EventPump;
use session::SessionStore;
use tauri::{Manager, State};

/// Fails to compile rather than at runtime if the shared state ever gains a non-`Send` field,
/// because Tauri shares managed state with its async runtime.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DesktopState>();
};

#[tauri::command]
async fn start_engine(
    state: State<'_, DesktopState>,
    config: StartRequest,
) -> Result<StartSummary, String> {
    commands::start_engine(state.inner(), config)
}

/// Whether an engine runs and its status: `{ state: "notRunning", run }` or the status with
/// `state: "running"` (see [dto::EngineStateDto]).
#[tauri::command]
async fn engine_status(state: State<'_, DesktopState>) -> Result<EngineStateDto, String> {
    commands::engine_state(state.inner())
}

#[tauri::command]
async fn stop_engine(state: State<'_, DesktopState>) -> Result<(), String> {
    commands::stop_engine(state.inner())
}

#[tauri::command]
async fn list_input_devices(host: Option<String>) -> Result<Vec<InputDeviceDto>, String> {
    commands::list_input_devices(host)
}

#[tauri::command]
async fn switch_device(
    state: State<'_, DesktopState>,
    device: String,
) -> Result<StartSummary, String> {
    commands::switch_device(state.inner(), device)
}

#[tauri::command]
async fn add_target(state: State<'_, DesktopState>, address: String) -> Result<String, String> {
    commands::add_target(state.inner(), address)
}

#[tauri::command]
async fn remove_target(state: State<'_, DesktopState>, address: String) -> Result<String, String> {
    commands::remove_target(state.inner(), address)
}

#[tauri::command]
async fn set_mix(
    state: State<'_, DesktopState>,
    address: String,
    mix: MixUpdateRequest,
) -> Result<MixDto, String> {
    commands::set_mix(state.inner(), address, mix)
}

#[tauri::command]
async fn set_musician_name(
    state: State<'_, DesktopState>,
    address: String,
    name: String,
) -> Result<(), String> {
    commands::set_musician_name(state.inner(), address, name)
}

/// Builds and runs the desktop application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // The session (musician labels today) is a desktop-app concern and lives in the app's
            // config directory, so it survives a restart without touching the engine or any client
            // protocol. Opening it never fails: a damaged file is set aside and logged.
            let directory = app.path().app_config_dir()?;
            // Every engine the window starts streams its meters, status and mix changes as events
            // (see [events]); the app handle is both where the pump reads and where it emits.
            let handle = app.handle().clone();
            app.manage(
                DesktopState::new(SessionStore::open(&directory)).with_event_pump(
                    move |events, run| {
                        EventPump::start(events, run, handle.clone(), handle.clone())
                    },
                ),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_engine,
            engine_status,
            stop_engine,
            list_input_devices,
            switch_device,
            add_target,
            remove_target,
            set_mix,
            set_musician_name
        ])
        .run(tauri::generate_context!())
        .expect("error while running the MixLink desktop shell");
}
