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
use dto::{
    ChannelRequest, ChannelsSetDto, EngineStateDto, EngineerControlsRequest,
    EngineerControlsSetDto, GroupRequest, GroupsSetDto, InputDeviceDto, MixDto, MixUpdateRequest,
    StartRequest, StartSummary,
};
use events::EventPump;
use session::{SessionChannel, SessionStore};
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

/// Replaces the engineer's groups (`[{ id?, name, channels }]`, channels 1-based, a new group with
/// no id) and answers what was done: `{ groups, applied, saved, saveError }`, each group with its id
/// (see [commands::set_groups] and [dto::GroupsSetDto]).
#[tauri::command]
async fn set_groups(
    state: State<'_, DesktopState>,
    groups: Vec<GroupRequest>,
) -> Result<GroupsSetDto, String> {
    commands::set_groups(state.inner(), groups)
}

/// Replaces the engineer's channel details (`[{ name, kind, hidden, link }]` in channel order, the
/// first entry CH 1) and answers what was done: `{ channels, applied, saved, saveError }` (see
/// [commands::set_channels] and [dto::ChannelsSetDto]).
#[tauri::command]
async fn set_channels(
    state: State<'_, DesktopState>,
    channels: Vec<ChannelRequest>,
) -> Result<ChannelsSetDto, String> {
    commands::set_channels(state.inner(), channels)
}

/// Sets the engineer's limit and locked channels for one musician (`{ limitPercent?,
/// lockedChannels? }`, the locks from CH 1 on; `clientId`, the row's, when the window knows it) and
/// answers what was done: `{ address, limitPercent, lockedChannels, keptFor, applied, saved,
/// saveError, mix }` (see [commands::set_engineer_controls] and [dto::EngineerControlsSetDto]).
#[tauri::command]
async fn set_engineer_controls(
    state: State<'_, DesktopState>,
    address: String,
    client_id: Option<String>,
    controls: EngineerControlsRequest,
) -> Result<EngineerControlsSetDto, String> {
    commands::set_engineer_controls(state.inner(), address, client_id, controls)
}

/// The engineer's whole channel table (`[{ name, kind, hidden, link }]` in channel order, details of
/// inputs the device lacks included): what the window edits and sends back with `set_channels`
/// (see [commands::channel_table]).
#[tauri::command]
async fn channel_table(state: State<'_, DesktopState>) -> Result<Vec<SessionChannel>, String> {
    commands::channel_table(state.inner())
}

/// Builds and runs the desktop application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // The session (musician labels, the engineer's channel details and groups) is a
            // desktop-app concern and
            // lives in the app's config directory, so it survives a restart without touching the
            // engine or any client protocol. Opening it never fails: a damaged file is set aside
            // and logged.
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
            set_musician_name,
            set_groups,
            set_channels,
            channel_table,
            set_engineer_controls
        ])
        .run(tauri::generate_context!())
        .expect("error while running the MixLink desktop shell");
}
