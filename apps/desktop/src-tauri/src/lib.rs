//! The MixLink desktop shell.
//!
//! This crate is deliberately thin: it owns the window and the JSON contract, and delegates every
//! engine call to [commands]. The engine lives in `personal-monitoring` and knows nothing of Tauri,
//! so the headless binary does not inherit this dependency stack.

pub mod commands;
pub mod dto;
pub mod events;
pub mod logfile;
pub mod session;
pub mod sessions;

use commands::DesktopState;
use dto::{
    AudioSettingsRequest, ChannelRequest, ChannelsSetDto, DiskEstimateDto, EngineStateDto,
    EngineerControlsRequest, EngineerControlsSetDto, GroupRequest, GroupsSetDto, InputDeviceDto,
    LocalOutputRequestDto, LocalOutputSetDto, MirrorRequest, MirrorSetDto, MixDto,
    MixUpdateRequest, MixesSetDto, OutputDeviceDto, PresetsDto, RecordingSettingsDto,
    RecordingSettingsRequest, RecordingStartedDto, RecordingStoppedDto, SessionLoadedDto,
    SessionSavedDto, SoundcheckDto, StartRequest, StartSummary, TakeMarkerDto, TakesDto,
};
use events::EventPump;
use logfile::LogLocation;
use personal_monitoring::{log_info, log_warn};
use session::{SessionAudio, SessionChannel, SessionPreset, SessionStore};
use sessions::SnapshotSummary;
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

/// Changes the engineer's local mix (`mix` as for `set_mix`, without `maxLevelPercent`: the local
/// mix has no level cap) and answers the whole mix now applied; it is kept in the session for the
/// next start (see [commands::local::set_local_mix]).
#[tauri::command]
async fn set_local_mix(
    state: State<'_, DesktopState>,
    mix: MixUpdateRequest,
) -> Result<MixDto, String> {
    commands::local::set_local_mix(state.inner(), mix)
}

/// The output devices the running driver offers for the local mix, `[{ name, channels }]` (see
/// [commands::local::output_devices]).
#[tauri::command]
async fn output_devices(state: State<'_, DesktopState>) -> Result<Vec<OutputDeviceDto>, String> {
    commands::local::output_devices(state.inner())
}

/// Plays the local mix on `output` (`{ device, firstChannel }`, the pair's first output 0-based) or
/// turns it off with null, and answers `{ output, localOutput, applied, saved, saveError }` (see
/// [commands::local::set_local_output]).
#[tauri::command]
async fn set_local_output(
    state: State<'_, DesktopState>,
    output: Option<LocalOutputRequestDto>,
) -> Result<LocalOutputSetDto, String> {
    commands::local::set_local_output(state.inner(), output)
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

/// The audio input the desk last ran with (`{ host, device, bufferFrames, asioDriver }`, or null
/// before any was kept), which the window reads when it opens and builds every start from (see
/// [commands::audio_settings]).
#[tauri::command]
async fn audio_settings(state: State<'_, DesktopState>) -> Result<Option<SessionAudio>, String> {
    Ok(commands::audio_settings(state.inner()))
}

/// Keeps the audio input the running engine started or switched with (`{ host, device,
/// bufferFrames, asioDriver }`) for every later start, and answers it as kept (see
/// [commands::set_audio_settings]).
#[tauri::command]
async fn set_audio_settings(
    state: State<'_, DesktopState>,
    settings: AudioSettingsRequest,
) -> Result<SessionAudio, String> {
    commands::set_audio_settings(state.inner(), settings)
}

/// The engineer's mix presets (`[{ id, name, mix: { channels: [{ gain, pan, muted, solo }],
/// groups: { <groupId>: level } } }]`, in the order they were saved; see [commands::presets]).
#[tauri::command]
async fn presets(state: State<'_, DesktopState>) -> Result<Vec<SessionPreset>, String> {
    Ok(commands::presets(state.inner()))
}

/// Saves the mix of the musician at `address` as a new preset called `name` and answers `{ preset,
/// presets }` (see [commands::save_preset]).
#[tauri::command]
async fn save_preset(
    state: State<'_, DesktopState>,
    address: String,
    name: String,
) -> Result<PresetsDto, String> {
    commands::save_preset(state.inner(), address, name)
}

/// Renames the preset `id` and answers `{ preset, presets }` (see [commands::rename_preset]).
#[tauri::command]
async fn rename_preset(
    state: State<'_, DesktopState>,
    id: String,
    name: String,
) -> Result<PresetsDto, String> {
    commands::rename_preset(state.inner(), id, name)
}

/// Deletes the preset `id` and answers `{ preset: null, presets }` (see [commands::delete_preset]).
#[tauri::command]
async fn delete_preset(state: State<'_, DesktopState>, id: String) -> Result<PresetsDto, String> {
    commands::delete_preset(state.inner(), id)
}

/// Puts the preset `id` on the mix of every musician `musicians` names and answers `{ mixes: [{
/// address, mix }] }` (see [commands::apply_preset]).
#[tauri::command]
async fn apply_preset(
    state: State<'_, DesktopState>,
    id: String,
    musicians: Vec<String>,
) -> Result<MixesSetDto, String> {
    commands::apply_preset(state.inner(), id, musicians)
}

/// Copies the mix of the musician at `from` onto every musician `to` names and answers `{ mixes }`
/// (see [commands::copy_mix]).
#[tauri::command]
async fn copy_mix(
    state: State<'_, DesktopState>,
    from: String,
    to: Vec<String>,
) -> Result<MixesSetDto, String> {
    commands::copy_mix(state.inner(), from, to)
}

/// Makes the mix of the musician at `address` (`clientId`, the row's, when the window knows it)
/// follow `leader` (`{ address, clientId?, name }`), or stops it with null, and answers `{ address,
/// leader, name, keptFor, applied, saved, saveError, mix }` (see [commands::set_mirror]).
#[tauri::command]
async fn set_mirror(
    state: State<'_, DesktopState>,
    address: String,
    client_id: Option<String>,
    leader: Option<MirrorRequest>,
) -> Result<MirrorSetDto, String> {
    commands::set_mirror(state.inner(), address, client_id, leader)
}

/// What went wrong or changed when the desk opened its session, or null (see
/// [commands::session_notice]).
#[tauri::command]
async fn session_notice(state: State<'_, DesktopState>) -> Result<Option<String>, String> {
    Ok(commands::session_notice(state.inner()))
}

/// The engineer's saved sessions, the newest first: `[{ id, name, savedAt, mixes, problem }]` (see
/// [commands::sessions]).
#[tauri::command]
async fn sessions(state: State<'_, DesktopState>) -> Result<Vec<SnapshotSummary>, String> {
    commands::sessions(state.inner())
}

/// Saves the desk's session, with every musician's mix when an engine runs, as `name` and answers
/// `{ session, engineRunning }` (see [commands::save_session]).
#[tauri::command]
async fn save_session(
    state: State<'_, DesktopState>,
    name: String,
) -> Result<SessionSavedDto, String> {
    commands::save_session(state.inner(), name)
}

/// Renames the saved session `id` and answers it as listed (see [commands::rename_session]).
#[tauri::command]
async fn rename_session(
    state: State<'_, DesktopState>,
    id: String,
    name: String,
) -> Result<SnapshotSummary, String> {
    commands::rename_session(state.inner(), id, name)
}

/// Deletes the saved session `id` (see [commands::delete_session]).
#[tauri::command]
async fn delete_session(state: State<'_, DesktopState>, id: String) -> Result<(), String> {
    commands::delete_session(state.inner(), id)
}

/// Loads the saved session `id` in place of the desk's and, with an engine running, applies it
/// live; answers `{ session, applied, saved, saveError, mixes, skipped, refused, damaged, audio,
/// audioLoaded, audioSkipped, presets }` (see [commands::load_session]).
#[tauri::command]
async fn load_session(
    state: State<'_, DesktopState>,
    id: String,
) -> Result<SessionLoadedDto, String> {
    commands::load_session(state.inner(), id)
}

/// How this desk records takes: `{ folder, defaultFolder, folderIsDefault, format, includeLocal,
/// armed, notice }` (see [commands::recording::recording_settings]).
#[tauri::command]
async fn recording_settings(
    state: State<'_, DesktopState>,
) -> Result<RecordingSettingsDto, String> {
    commands::recording::recording_settings(state.inner())
}

/// Keeps `settings` (`{ folder, format, includeLocal, armed }`, the folder null for the default and
/// the inputs 1-based) and answers them as kept (see [commands::recording::set_recording_settings]).
#[tauri::command]
async fn set_recording_settings(
    state: State<'_, DesktopState>,
    settings: RecordingSettingsRequest,
) -> Result<RecordingSettingsDto, String> {
    commands::recording::set_recording_settings(state.inner(), settings)
}

/// Starts recording a take called `takeName` with the kept settings (`utcOffsetMinutes`, the
/// window's offset from UTC, names its folder in local time) and answers `{ recording, folder,
/// armed, skipped, includeLocal, format }` (see [commands::recording::start_recording]).
#[tauri::command]
async fn start_recording(
    state: State<'_, DesktopState>,
    take_name: String,
    utc_offset_minutes: Option<i32>,
) -> Result<RecordingStartedDto, String> {
    commands::recording::start_recording(state.inner(), take_name, utc_offset_minutes)
}

/// Stops the take and answers `{ finishing, take, message }`: the finished take, or `finishing`
/// while its files are still being finished (ask again; see
/// [commands::recording::stop_recording]).
#[tauri::command]
async fn stop_recording(state: State<'_, DesktopState>) -> Result<RecordingStoppedDto, String> {
    commands::recording::stop_recording(state.inner())
}

/// Marks the take being recorded with `name` and answers `{ frame, seconds, name }` (see
/// [commands::recording::add_marker]).
#[tauri::command]
async fn add_marker(state: State<'_, DesktopState>, name: String) -> Result<TakeMarkerDto, String> {
    commands::recording::add_marker(state.inner(), name)
}

/// What a take with the kept settings would take on the disk: `{ folder, bytesPerSecond, freeBytes,
/// minutesLeft, problem }` (see [commands::recording::record_estimate]).
#[tauri::command]
async fn record_estimate(state: State<'_, DesktopState>) -> Result<DiskEstimateDto, String> {
    commands::recording::record_estimate(state.inner())
}

/// The takes in the recordings folder, the newest first: `{ folder, takes, unreadable, missing }`
/// (see [commands::recording::list_takes]).
#[tauri::command]
async fn list_takes(state: State<'_, DesktopState>) -> Result<TakesDto, String> {
    commands::recording::list_takes(state.inner())
}

/// Opens the take folder `folder` in the system's file manager; only a take folder inside the
/// recordings folder (see [commands::recording::open_take_folder]).
#[tauri::command]
async fn open_take_folder(state: State<'_, DesktopState>, folder: String) -> Result<(), String> {
    commands::recording::open_take_folder(state.inner(), folder)
}

/// Plays the take in `takeFolder` (a take folder of the recordings folder) in place of the live
/// inputs, from `startFrame` (0 when absent), looping when `looping`, and answers the soundcheck
/// `{ state, reason, takeFolder, takeName, positionFrames, positionSeconds, lengthFrames,
/// lengthSeconds, looping, underrunFrames, clock }` (see [commands::soundcheck::start_soundcheck]).
/// The takes list's Play is this command: a take is heard through the engine, never on its own.
#[tauri::command]
async fn start_soundcheck(
    state: State<'_, DesktopState>,
    take_folder: String,
    start_frame: Option<u64>,
    looping: Option<bool>,
) -> Result<SoundcheckDto, String> {
    commands::soundcheck::start_soundcheck(state.inner(), take_folder, start_frame, looping)
}

/// Pauses the soundcheck's take and answers the soundcheck.
#[tauri::command]
async fn pause_soundcheck(state: State<'_, DesktopState>) -> Result<SoundcheckDto, String> {
    commands::soundcheck::pause_soundcheck(state.inner())
}

/// Plays the soundcheck's take on (from its start when it stopped at its end).
#[tauri::command]
async fn resume_soundcheck(state: State<'_, DesktopState>) -> Result<SoundcheckDto, String> {
    commands::soundcheck::resume_soundcheck(state.inner())
}

/// Moves the soundcheck's take to `frame` and answers the soundcheck.
#[tauri::command]
async fn seek_soundcheck(
    state: State<'_, DesktopState>,
    frame: u64,
) -> Result<SoundcheckDto, String> {
    commands::soundcheck::seek_soundcheck(state.inner(), frame)
}

/// Whether the soundcheck's take starts again at its end.
#[tauri::command]
async fn set_soundcheck_loop(
    state: State<'_, DesktopState>,
    looping: bool,
) -> Result<SoundcheckDto, String> {
    commands::soundcheck::set_soundcheck_loop(state.inner(), looping)
}

/// Stops the soundcheck and goes back to the live inputs.
#[tauri::command]
async fn stop_soundcheck(state: State<'_, DesktopState>) -> Result<SoundcheckDto, String> {
    commands::soundcheck::stop_soundcheck(state.inner())
}

/// Where the log is: `{ directory, file }`, the folder and the current `mixlink.log` (see
/// [logfile]); an error when the app could not set its log file up and logs to stderr only.
#[tauri::command]
async fn log_location() -> Result<LogLocation, String> {
    logfile::location().ok_or_else(|| "the log file is not set up: lines go to stderr".to_owned())
}

/// Builds and runs the desktop application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // First, so every later line (the engine's too) reaches the log file: a release build
            // has no console. Without a log folder the lines stay on stderr.
            match app.path().app_log_dir() {
                Ok(log_directory) => {
                    let log_file = logfile::install(log_directory);
                    log_info!(
                        "app",
                        "MixLink desktop {} on {} {}; log file \"{}\"",
                        app.package_info().version,
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        log_file.display()
                    );
                }
                Err(error) => log_warn!("app", "no log folder ({error}); logging to stderr only"),
            }
            // The session (musician labels, the engineer's channel details, groups and controls,
            // the audio input, and the local mix and its output) is a desktop-app concern and
            // lives in the app's config directory, so it survives a restart without touching the
            // engine or any client protocol. Opening it never fails: a damaged file is set aside
            // and logged. The engineer's named sessions live beside it, in `sessions`.
            let directory = app.path().app_config_dir()?;
            // Takes go to the user's Music folder + MixLink\Recordings by default; when it cannot
            // be found, to the app's own data folder, and the recording settings say so. Never
            // inside the config folder, which the data folder is on macOS: then the next place.
            let recordings = commands::recording::default_recordings_folder(
                app.path().audio_dir().ok(),
                app.path().app_local_data_dir().ok(),
                &directory,
            );
            if let Some(notice) = &recordings.notice {
                log_warn!("app", "{notice}");
            }
            // Every engine the window starts streams its meters, status and mix changes as events
            // (see [events]); the app handle is both where the pump reads and where it emits.
            let handle = app.handle().clone();
            app.manage(
                DesktopState::new(SessionStore::open(&directory))
                    .with_recordings_folder(recordings)
                    .with_event_pump(move |events, run| {
                        EventPump::start(events, run, handle.clone(), handle.clone())
                    }),
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
            set_local_mix,
            output_devices,
            set_local_output,
            set_musician_name,
            set_groups,
            set_channels,
            channel_table,
            set_engineer_controls,
            audio_settings,
            set_audio_settings,
            presets,
            save_preset,
            rename_preset,
            delete_preset,
            apply_preset,
            copy_mix,
            set_mirror,
            session_notice,
            sessions,
            save_session,
            rename_session,
            delete_session,
            load_session,
            recording_settings,
            set_recording_settings,
            start_recording,
            stop_recording,
            add_marker,
            record_estimate,
            list_takes,
            open_take_folder,
            start_soundcheck,
            pause_soundcheck,
            resume_soundcheck,
            seek_soundcheck,
            set_soundcheck_loop,
            stop_soundcheck,
            log_location
        ])
        .build(tauri::generate_context!())
        .expect("error while building the MixLink desktop shell")
        .run(|app, event| {
            let state = app.try_state::<DesktopState>();
            on_run_event(state.as_deref(), &event);
        });
}

/// What the app does as it runs, given its state once managed: on exit, the local mix still
/// waiting to be kept (it is kept off the engine lock, a few times a second at most) is written
/// before the app goes, and then the log is flushed so its last lines stay.
fn on_run_event(state: Option<&DesktopState>, event: &tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event {
        if let Some(state) = state {
            commands::local::flush_local_mix(state);
        }
        log_info!("app", "exiting");
        personal_monitoring::logging::flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionMix;

    fn mix(volume_percent: u8) -> SessionMix {
        SessionMix {
            channels: Vec::new(),
            groups: Default::default(),
            volume_percent,
            muted: false,
        }
    }

    fn kept_volume(state: &DesktopState) -> Option<u8> {
        state
            .kept_local()
            .and_then(|local| local.mix)
            .map(|mix| mix.volume_percent)
    }

    #[test]
    fn only_quitting_the_app_writes_the_local_mix_still_waiting() {
        let directory = std::env::temp_dir()
            .join("mixlink-run-event-tests")
            .join("exit");
        let _ = std::fs::remove_dir_all(&directory);
        // The state's saver writes only when flushed, so no interval can write the mix behind the
        // test's back however the threads are scheduled: whatever reaches the session was written
        // by the run event.
        let _manual = commands::local::saver::manual_savers();
        let state = DesktopState::new(SessionStore::open(&directory));

        state.mark_local_mix(mix(10));
        on_run_event(Some(&state), &tauri::RunEvent::Ready);
        assert_eq!(
            kept_volume(&state),
            None,
            "nothing is written on another event"
        );
        on_run_event(Some(&state), &tauri::RunEvent::Exit);
        assert_eq!(kept_volume(&state), Some(10), "written on exit");

        state.mark_local_mix(mix(20));
        on_run_event(Some(&state), &tauri::RunEvent::Ready);
        assert_eq!(
            kept_volume(&state),
            Some(10),
            "nothing is written on another event"
        );
        on_run_event(Some(&state), &tauri::RunEvent::Exit);
        assert_eq!(
            kept_volume(&state),
            Some(20),
            "the latest mix is written on exit"
        );
        on_run_event(None, &tauri::RunEvent::Exit);
    }
}
