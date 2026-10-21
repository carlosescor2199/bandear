//! The engineer's local mix and the output it plays on: the `set_local_mix`, `output_devices` and
//! `set_local_output` commands, and what a start or a loaded session applies of the local section
//! the desk keeps (see [crate::session::SessionLocal]).
//!
//! The local mix is the engine's own mix for this computer; no phone ever hears it. The window
//! edits it with a musician's strips, without a level cap, the engineer's limit or locks, presets,
//! copies or mirrors. Every edit is applied by the engine first, under the engine lock, and then
//! kept in the session by group id, as a saved session keeps a mix, so the next start plays it
//! again. The session is written off the engine lock and at most every few hundred milliseconds
//! while a fader moves ([saver]); stopping the engine and quitting the app write what is waiting.
//!
//! The output follows the engine's rules ([EngineHandle::set_local_output]): off by default, at
//! 48 kHz, and only on a device the running driver offers ([EngineHandle::output_devices]): under
//! ASIO the capture driver's own outputs, under WASAPI any output device. A start, and a loaded
//! session, open the kept output only when the running driver offers it ([apply_kept_local]); one
//! it does not offer, or that cannot be opened, leaves the local output off and the status says
//! why (`localOutput.notice`). The desk never plays the local mix on another device in its place,
//! and the choice stays kept, so a later start on a driver that offers it opens it again.

pub(crate) mod saver;

use std::sync::Mutex;

use personal_monitoring::{
    log_info, log_warn, EngineHandle, LocalOutputRequest, LocalOutputState, LocalOutputStatus,
    MixUpdate, MixValues, OutputDevice,
};

use self::saver::LocalMixSaver;
use super::{saved_update, session_mix_of, DeskReader, DesktopState};
use crate::dto::{
    LocalOutputDto, LocalOutputRequestDto, LocalOutputSetDto, MixDto, MixUpdateRequest,
    OutputDeviceDto,
};
use crate::session::{SessionLocal, SessionMix, SessionOutput, SessionStore};

/// What the local mix and its output need from the running engine: the engine in the app, a
/// stand-in in the tests, which run without audio hardware.
pub(super) trait LocalEngine {
    /// Changes the local mix from the desk ([EngineHandle::set_local_mix]).
    fn set_local_mix(&mut self, update: &MixUpdate) -> Result<MixValues, String>;
    /// The outputs the running driver offers ([EngineHandle::output_devices]).
    fn output_devices(&self) -> Result<Vec<OutputDevice>, String>;
    /// Plays the local mix on `request`, or turns the output off with `None`
    /// ([EngineHandle::set_local_output]); a refusal leaves the output in use as it was.
    fn set_local_output(
        &mut self,
        request: Option<LocalOutputRequest>,
    ) -> Result<LocalOutputStatus, String>;
    /// Where the local mix plays now ([EngineHandle::local_output]).
    fn local_output(&self) -> LocalOutputStatus;
}

impl LocalEngine for EngineHandle {
    fn set_local_mix(&mut self, update: &MixUpdate) -> Result<MixValues, String> {
        EngineHandle::set_local_mix(self, update).map_err(|error| error.to_string())
    }

    fn output_devices(&self) -> Result<Vec<OutputDevice>, String> {
        EngineHandle::output_devices(self).map_err(|error| error.to_string())
    }

    fn set_local_output(
        &mut self,
        request: Option<LocalOutputRequest>,
    ) -> Result<LocalOutputStatus, String> {
        EngineHandle::set_local_output(self, request).map_err(|error| error.to_string())
    }

    fn local_output(&self) -> LocalOutputStatus {
        EngineHandle::local_output(self)
    }
}

/// Changes the engineer's local mix from the desk and answers the whole mix now applied.
///
/// `mix` is partial, as a musician's is for [super::set_mix]: absent fields keep their values, a
/// list replaces the values from CH 1 (or the first group) on, levels are clamped, a list longer
/// than the mix is refused. The local mix has no level cap, so `maxLevelPercent` is refused. With
/// no engine running, or one that is stopping, the change is refused and nothing changes.
///
/// The engine takes the change first, under the engine lock: the local output plays it at once and
/// the `mix-changed` event with the id `local` announces it. The mix is then only marked for the
/// session ([LocalMixSaver::mark]), by group id, and written off the engine lock at most every
/// [saver::SAVE_INTERVAL]: a fader sends many of these a second, and no disk write may hold the
/// engine (or the meters and status that read it) up. A save that fails is logged, once, not
/// answered as an error, since the change plays. Not logged otherwise.
pub fn set_local_mix(state: &DesktopState, mix: MixUpdateRequest) -> Result<MixDto, String> {
    edit_local_mix(&state.engine, &state.local_saver, mix)
}

/// Writes the local mix still waiting to be kept, now: when the app quits. Never under the engine
/// lock.
pub fn flush_local_mix(state: &DesktopState) {
    state.local_saver.flush();
}

/// The local section a start applies: the session's, with a local mix marked and not written yet
/// (a fader moved just before the start) in it, read with the saver paused so no write lands in
/// between. The marked mix stays marked and is written later.
pub(super) fn kept_local(session: &SessionStore, saver: &LocalMixSaver) -> Option<SessionLocal> {
    let paused = saver.pause();
    with_pending(session.local(), paused.pending())
}

/// The session's local section with `pending`, a local mix marked and not written yet, in it.
pub(super) fn with_pending(
    local: Option<SessionLocal>,
    pending: Option<SessionMix>,
) -> Option<SessionLocal> {
    match pending {
        Some(mix) => Some(SessionLocal {
            mix: Some(mix),
            ..local.unwrap_or_default()
        }),
        None => local,
    }
}

/// [set_local_mix] over any engine slot and saver.
fn edit_local_mix<E: LocalEngine + DeskReader>(
    engine: &Mutex<Option<E>>,
    saver: &LocalMixSaver,
    mix: MixUpdateRequest,
) -> Result<MixDto, String> {
    if mix.max_level_percent.is_some() {
        return Err(
            "the local mix has no level cap: `maxLevelPercent` belongs to a musician's phone"
                .to_owned(),
        );
    }
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let applied = engine.set_local_mix(&mix.into())?;
    let desk = engine.desk();
    // Marked only: the saver writes it after the engine lock is released.
    saver.mark(session_mix_of(&applied, &desk));
    Ok(MixDto::from(&applied))
}

/// The output devices the running driver offers for the local mix, each with its output channel
/// count: under ASIO only the capture driver's own outputs, under WASAPI every output device (see
/// [EngineHandle::output_devices]). No ASIO driver is ever loaded to list them. The list depends
/// on the running driver, so with no engine running it is refused.
pub fn output_devices(state: &DesktopState) -> Result<Vec<OutputDeviceDto>, String> {
    list_outputs(&state.engine)
}

/// [output_devices] over any engine slot.
fn list_outputs<E: LocalEngine>(engine: &Mutex<Option<E>>) -> Result<Vec<OutputDeviceDto>, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard.as_ref().ok_or_else(|| {
        "the engine is not running: the outputs offered depend on the driver it runs".to_owned()
    })?;
    Ok(engine
        .output_devices()?
        .iter()
        .map(OutputDeviceDto::from)
        .collect())
}

/// Chooses the output the local mix plays on, or none with `None`, and answers what was done
/// ([LocalOutputSetDto]). `output` names the device exactly and the first output of its stereo pair
/// (0-based); the device name must not be empty.
///
/// - With an engine running, the engine takes the choice first (the change is live: the new output
///   opens, the local mix moves to it between two blocks, and the previous one closes; none turns
///   the local output off) and only then is it kept. A device the running driver does not offer, a
///   pair past its outputs, a device not at 48 kHz or one that cannot be opened is refused: an
///   error, nothing is kept, and the output in use keeps playing. A save that fails after the
///   engine took the choice is reported in the answer, not as an error. A choice the engine took
///   clears what the status said about the kept output at start.
/// - An engine that is stopping refuses it: an error, and nothing changed.
/// - With no engine, it is kept and applies at the next start (`applied: false`), opened then only
///   when that engine's driver offers it.
pub fn set_local_output(
    state: &DesktopState,
    output: Option<LocalOutputRequestDto>,
) -> Result<LocalOutputSetDto, String> {
    let outcome = choose_output(&state.engine, &state.session, &state.local_notice, output)?;
    log_info!(
        "local",
        "set_local_output: {} applied={} saved={}",
        outcome
            .output
            .as_ref()
            .map_or_else(|| "none".to_owned(), describe),
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// [set_local_output] over any engine slot, session and notice.
pub(super) fn choose_output<E: LocalEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    notice: &Mutex<Option<String>>,
    output: Option<LocalOutputRequestDto>,
) -> Result<LocalOutputSetDto, String> {
    let kept = output.map(|output| SessionOutput {
        device: output.device,
        first_channel: output.first_channel,
    });
    if kept.as_ref().is_some_and(|output| output.device.is_empty()) {
        return Err("an output needs the name of its device".to_owned());
    }
    // One step, in the lock order every command that holds both follows: the engine's, then the
    // session's and the notice's.
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let status = match guard.as_mut() {
        // A refusal returns here, before anything is kept.
        Some(engine) => Some(engine.set_local_output(kept.as_ref().map(request_of))?),
        None => None,
    };
    let applied = status.is_some();
    if applied {
        if let Ok(mut said) = notice.lock() {
            *said = None;
        }
    }
    let save_error = match session.set_local_output(kept.clone()) {
        Ok(()) => None,
        // The engine already plays it: say that it was not kept.
        Err(error) if applied => Some(error),
        // Nothing took it, so nothing changed.
        Err(error) => return Err(error),
    };
    Ok(LocalOutputSetDto {
        output: kept,
        local_output: status.as_ref().map(LocalOutputDto::from),
        applied,
        saved: save_error.is_none(),
        save_error,
    })
}

/// What applying the kept local section to a running engine did.
pub(super) struct LocalApplied {
    /// The local mix now stored, when the kept one was pushed.
    pub(super) mix: Option<MixValues>,
    /// Why the kept local mix was not pushed (an engine that is stopping).
    pub(super) refused: Option<String>,
    /// Why the kept output was not opened; `None` when it plays, or none is kept.
    pub(super) notice: Option<String>,
}

/// Applies the kept `local` section to a running engine, as every start and a loaded session do:
/// the kept local mix first, through the desk's path, its group levels by id onto the live groups
/// (a live group it does not name keeps its level); then the kept output ([apply_output]).
pub(super) fn apply_kept_local<E: LocalEngine + DeskReader>(
    engine: &mut E,
    local: &SessionLocal,
) -> LocalApplied {
    let (mix, refused) = match &local.mix {
        Some(kept) => {
            let desk = engine.desk();
            let update = saved_update(kept, &desk.local, &desk.groups, false);
            match engine.set_local_mix(&update) {
                Ok(mix) => (Some(mix), None),
                Err(error) => (
                    None,
                    Some(format!("the saved local mix was not applied: {error}")),
                ),
            }
        }
        None => (None, None),
    };
    LocalApplied {
        mix,
        refused,
        notice: apply_output(engine, local.output.as_ref()),
    }
}

/// Puts the kept `output` in force and answers why it is not playing when it is not.
///
/// None turns the local output off. One that already plays is left as it is. One the running driver
/// offers (by its exact name, its pair within the device's outputs) is opened. One it does not
/// offer, or that cannot be opened, leaves the local output off: whatever played before stops, and
/// the local mix never plays on another device in its place.
fn apply_output<E: LocalEngine>(engine: &mut E, output: Option<&SessionOutput>) -> Option<String> {
    let current = engine.local_output();
    let Some(output) = output else {
        if current.output.is_none() {
            return None;
        }
        return engine.set_local_output(None).err().map(|error| {
            format!("The local output could not be turned off as the session keeps it: {error}.")
        });
    };
    let request = request_of(output);
    if current.output.as_ref() == Some(&request) && current.state == LocalOutputState::Running {
        return None;
    }
    let problem = match engine.output_devices() {
        Err(error) => Some(format!("the outputs could not be listed ({error})")),
        Ok(devices) if !offers(&devices, &request) => {
            Some("the running audio driver does not offer it".to_owned())
        }
        Ok(_) => engine
            .set_local_output(Some(request))
            .err()
            .map(|error| format!("it could not be opened ({error})")),
    }?;
    if engine.local_output().output.is_some() {
        // Never another device in its place.
        if let Err(error) = engine.set_local_output(None) {
            log_warn!("local", "the local output could not be turned off: {error}");
        }
    }
    Some(format!(
        "The saved local mix output, {}, was not opened: {problem}. The local mix plays nowhere \
         until you choose an output.",
        describe(output)
    ))
}

/// Whether `devices` offer `request`: the device by its exact name, with the pair within its
/// outputs.
fn offers(devices: &[OutputDevice], request: &LocalOutputRequest) -> bool {
    devices.iter().any(|device| {
        device.name == request.device
            && u32::from(request.first_channel) + 2 <= u32::from(device.channels)
    })
}

fn request_of(output: &SessionOutput) -> LocalOutputRequest {
    LocalOutputRequest {
        device: output.device.clone(),
        first_channel: output.first_channel,
    }
}

/// An output in the window's words: "Volt 4 · outputs 3/4".
fn describe(output: &SessionOutput) -> String {
    let first = u32::from(output.first_channel) + 1;
    format!("{} · outputs {first}/{}", output.device, first + 1)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::commands::DeskView;
    use crate::session::{SessionMix, SessionPresetChannel};

    /// An empty scratch config directory for one test's session.
    fn session_directory(test: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join("mixlink-local-tests").join(test);
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    const VOLT: &str = "OUTPUT 1/2 (Volt 4)";
    const VOLT_ASIO: &str = "Volt 4 ASIO";
    const SPEAKERS: &str = "Speakers (Realtek Audio)";

    fn device(name: &str, channels: u16) -> OutputDevice {
        OutputDevice {
            name: name.to_owned(),
            channels,
        }
    }

    fn output(device: &str, first_channel: u16) -> SessionOutput {
        SessionOutput {
            device: device.to_owned(),
            first_channel,
        }
    }

    fn playing(device: &str, first_channel: u16) -> LocalOutputStatus {
        LocalOutputStatus {
            output: Some(LocalOutputRequest {
                device: device.to_owned(),
                first_channel,
            }),
            state: LocalOutputState::Running,
            underrun_frames: 0,
            dropped_frames: 0,
        }
    }

    /// Stands in for a running engine's local mix and output: it merges the levels it is given,
    /// offers `offered` and opens what it offers, refusing as the engine refuses.
    #[derive(Default)]
    struct LocalStandIn {
        stopping: bool,
        inputs: usize,
        groups: Vec<String>,
        local: MixValues,
        /// Every local mix change it took, in order.
        updates: Vec<MixUpdate>,
        offered: Vec<OutputDevice>,
        /// Why listing the outputs fails, when it does.
        list_error: Option<String>,
        /// The device that cannot be opened, and why.
        broken: Option<(String, String)>,
        output: LocalOutputStatus,
        /// Every output change it was asked for, in order, with what it had taken by then.
        log: Vec<String>,
    }

    impl LocalEngine for LocalStandIn {
        fn set_local_mix(&mut self, update: &MixUpdate) -> Result<MixValues, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.log.push("set_local_mix".to_owned());
            self.updates.push(update.clone());
            let level = |value: &i32| (*value).clamp(0, 100) as u8;
            for (index, gain) in update.channel_gains.iter().flatten().enumerate() {
                self.local.channel_gains[index] = level(gain);
            }
            for (index, pan) in update.pans.iter().flatten().enumerate() {
                self.local.pans[index] = level(pan);
            }
            for (index, group) in update.group_levels.iter().flatten().enumerate() {
                self.local.group_levels[index] = level(group);
            }
            if let Some(volume) = &update.volume_percent {
                self.local.volume_percent = level(volume);
            }
            if let Some(muted) = update.muted {
                self.local.muted = muted;
            }
            self.local.revision += 1;
            Ok(self.local)
        }

        fn output_devices(&self) -> Result<Vec<OutputDevice>, String> {
            match &self.list_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.offered.clone()),
            }
        }

        fn set_local_output(
            &mut self,
            request: Option<LocalOutputRequest>,
        ) -> Result<LocalOutputStatus, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let Some(request) = request else {
                self.log.push("set_local_output none".to_owned());
                self.output = LocalOutputStatus::default();
                return Ok(self.output.clone());
            };
            self.log
                .push(format!("set_local_output {}", request.device));
            if !offers(&self.offered, &request) {
                return Err(format!(
                    "\"{}\" is not an output the engine offers",
                    request.device
                ));
            }
            if let Some((broken, why)) = &self.broken {
                if *broken == request.device {
                    return Err(why.clone());
                }
            }
            self.output = playing(&request.device, request.first_channel);
            Ok(self.output.clone())
        }

        fn local_output(&self) -> LocalOutputStatus {
            self.output.clone()
        }
    }

    impl DeskReader for LocalStandIn {
        fn desk(&self) -> DeskView {
            DeskView {
                inputs: self.inputs,
                groups: self.groups.clone(),
                musicians: Vec::new(),
                local: self.local,
                processing: Vec::new(),
            }
        }
    }

    fn running(engine: LocalStandIn) -> Mutex<Option<LocalStandIn>> {
        Mutex::new(Some(engine))
    }

    fn two_inputs_two_groups() -> LocalStandIn {
        LocalStandIn {
            inputs: 2,
            groups: vec!["kit".to_owned(), "vox".to_owned()],
            offered: vec![device(VOLT, 4), device(SPEAKERS, 2)],
            ..LocalStandIn::default()
        }
    }

    fn channel(gain: u8) -> SessionPresetChannel {
        SessionPresetChannel {
            gain,
            pan: 50,
            muted: false,
            solo: false,
        }
    }

    fn engine_of(
        engine: &Mutex<Option<LocalStandIn>>,
    ) -> std::sync::MutexGuard<'_, Option<LocalStandIn>> {
        engine.lock().expect("engine lock")
    }

    type Shared = std::sync::Arc<Mutex<Option<LocalStandIn>>>;

    /// A saver with no thread of its own that keeps every mix it writes in `session`, and notes
    /// for each write whether the engine lock was free.
    fn saver_into(
        session: &std::sync::Arc<SessionStore>,
        engine: &Shared,
        lock_free: &std::sync::Arc<Mutex<Vec<bool>>>,
    ) -> LocalMixSaver {
        let (session, engine, lock_free) = (
            std::sync::Arc::clone(session),
            std::sync::Arc::clone(engine),
            std::sync::Arc::clone(lock_free),
        );
        LocalMixSaver::manual(
            move |mix: &SessionMix| {
                lock_free
                    .lock()
                    .expect("notes")
                    .push(engine.try_lock().is_ok());
                session.set_local_mix(mix.clone())
            },
            |_| {},
        )
    }

    fn shared(engine: Option<LocalStandIn>) -> Shared {
        std::sync::Arc::new(Mutex::new(engine))
    }

    #[test]
    fn a_local_mix_change_plays_first_and_is_then_kept_by_group_id_off_the_engine_lock() {
        let session = std::sync::Arc::new(SessionStore::open(&session_directory("mix-kept")));
        let engine = shared(Some(two_inputs_two_groups()));
        let lock_free = std::sync::Arc::default();
        let saver = saver_into(&session, &engine, &lock_free);

        let mut answer = None;
        for volume in [10, 40, 85] {
            answer = Some(
                edit_local_mix(
                    &engine,
                    &saver,
                    MixUpdateRequest {
                        channel_gains: Some(vec![80, 40, 9]),
                        pans: Some(vec![50, 50, 50]),
                        group_levels: Some(vec![70, 30]),
                        volume_percent: Some(volume),
                        ..MixUpdateRequest::default()
                    },
                )
                .expect("the engine takes it"),
            );
        }

        let answer = answer.expect("answered");
        assert_eq!(answer.volume_percent, 85, "the mix now applied");
        assert_eq!(answer.revision, 3);
        assert_eq!(
            session.local(),
            None,
            "an edit only marks the mix: nothing is written under the engine lock"
        );
        saver.flush();
        assert_eq!(
            session.local(),
            Some(SessionLocal {
                fx_returns: None,
                limiter: None,
                mix: Some(SessionMix {
                    channels: vec![channel(80), channel(40)],
                    groups: BTreeMap::from([("kit".to_owned(), 70), ("vox".to_owned(), 30)]),
                    volume_percent: 85,
                    muted: false,
                }),
                output: None,
            }),
            "the last change, the device's inputs and the group levels by id, never a level cap"
        );
        assert_eq!(
            *lock_free.lock().expect("notes"),
            vec![true],
            "one write, with the engine lock free"
        );
    }

    #[test]
    fn a_local_mix_change_with_no_engine_or_one_that_stops_is_refused_and_nothing_is_kept() {
        let session = std::sync::Arc::new(SessionStore::open(&session_directory("mix-refused")));
        let idle = shared(None);
        let stopping = shared(Some(LocalStandIn {
            stopping: true,
            ..two_inputs_two_groups()
        }));
        let lock_free = std::sync::Arc::default();
        let saver = saver_into(&session, &idle, &lock_free);
        let change = || MixUpdateRequest {
            volume_percent: Some(10),
            ..MixUpdateRequest::default()
        };

        let error = edit_local_mix(&idle, &saver, change()).expect_err("refused");
        assert!(error.contains("not running"), "{error}");
        let error = edit_local_mix(&stopping, &saver, change()).expect_err("refused");
        assert!(error.contains("stopping"), "{error}");
        saver.flush();

        assert_eq!(session.local(), None, "nothing is kept");
        assert!(
            lock_free.lock().expect("notes").is_empty(),
            "nothing written"
        );
    }

    #[test]
    fn the_local_mix_has_no_level_cap() {
        let session = std::sync::Arc::new(SessionStore::open(&session_directory("mix-no-cap")));
        let engine = shared(Some(two_inputs_two_groups()));
        let lock_free = std::sync::Arc::default();
        let saver = saver_into(&session, &engine, &lock_free);

        let error = edit_local_mix(
            &engine,
            &saver,
            MixUpdateRequest {
                max_level_percent: Some(50),
                volume_percent: Some(60),
                ..MixUpdateRequest::default()
            },
        )
        .expect_err("refused");
        saver.flush();

        assert!(error.contains("no level cap"), "{error}");
        assert!(
            engine_of(&engine)
                .as_ref()
                .expect("running")
                .updates
                .is_empty(),
            "the engine took nothing"
        );
        assert_eq!(session.local(), None);
    }

    #[test]
    fn a_start_right_after_a_fader_edit_plays_the_edit_and_still_keeps_it() {
        let session =
            std::sync::Arc::new(SessionStore::open(&session_directory("start-after-edit")));
        session
            .set_local_mix(SessionMix {
                channels: vec![channel(80)],
                groups: BTreeMap::new(),
                volume_percent: 30,
                muted: false,
            })
            .expect("kept");
        session
            .set_local_output(Some(output(VOLT, 2)))
            .expect("kept");
        let running_now = shared(Some(two_inputs_two_groups()));
        let lock_free = std::sync::Arc::default();
        let saver = saver_into(&session, &running_now, &lock_free);
        edit_local_mix(
            &running_now,
            &saver,
            MixUpdateRequest {
                volume_percent: Some(85),
                ..MixUpdateRequest::default()
            },
        )
        .expect("taken");

        // The engine restarts before the saver wrote the edit.
        let kept = super::kept_local(&session, &saver).expect("a local section");
        let mut started = two_inputs_two_groups();
        let applied = apply_kept_local(&mut started, &kept);

        assert_eq!(
            started
                .updates
                .last()
                .and_then(|update| update.volume_percent),
            Some(85),
            "the new engine plays the edit, not the session's older mix"
        );
        assert_eq!(applied.notice, None);
        assert_eq!(started.output, playing(VOLT, 2), "with the kept output");
        assert_eq!(
            session
                .local()
                .and_then(|local| local.mix)
                .map(|mix| mix.volume_percent),
            Some(30),
            "not written yet"
        );
        saver.flush();
        assert_eq!(
            session
                .local()
                .and_then(|local| local.mix)
                .map(|mix| mix.volume_percent),
            Some(85),
            "and still kept afterwards"
        );
    }

    #[test]
    fn a_pending_local_mix_joins_the_kept_section_and_keeps_its_output() {
        let kept = SessionLocal {
            fx_returns: None,
            limiter: None,
            mix: None,
            output: Some(output(VOLT, 2)),
        };
        let marked = SessionMix {
            channels: vec![channel(5)],
            groups: BTreeMap::new(),
            volume_percent: 20,
            muted: true,
        };

        assert_eq!(with_pending(Some(kept.clone()), None), Some(kept.clone()));
        assert_eq!(
            with_pending(Some(kept), Some(marked.clone())),
            Some(SessionLocal {
                fx_returns: None,
                limiter: None,
                mix: Some(marked.clone()),
                output: Some(output(VOLT, 2)),
            })
        );
        assert_eq!(
            with_pending(None, Some(marked.clone())),
            Some(SessionLocal {
                fx_returns: None,
                limiter: None,
                mix: Some(marked),
                output: None,
            })
        );
        assert_eq!(with_pending(None, None), None);
    }

    #[test]
    fn the_outputs_are_listed_only_by_a_running_engine() {
        let idle: Mutex<Option<LocalStandIn>> = Mutex::new(None);
        let error = list_outputs(&idle).expect_err("no engine");
        assert!(error.contains("not running"), "{error}");

        let engine = running(two_inputs_two_groups());
        assert_eq!(
            list_outputs(&engine).expect("listed"),
            vec![
                OutputDeviceDto {
                    name: VOLT.to_owned(),
                    channels: 4,
                },
                OutputDeviceDto {
                    name: SPEAKERS.to_owned(),
                    channels: 2,
                },
            ]
        );
    }

    fn choice(device: &str, first_channel: u16) -> Option<LocalOutputRequestDto> {
        Some(LocalOutputRequestDto {
            device: device.to_owned(),
            first_channel,
        })
    }

    #[test]
    fn an_output_the_engine_takes_is_kept_and_clears_what_the_start_said() {
        let session = SessionStore::open(&session_directory("output-kept"));
        let engine = running(two_inputs_two_groups());
        let notice = Mutex::new(Some("the kept output was not opened".to_owned()));

        let outcome = choose_output(&engine, &session, &notice, choice(VOLT, 2)).expect("taken");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(outcome.output, Some(output(VOLT, 2)));
        let playing = outcome.local_output.expect("the engine's word");
        assert_eq!(playing.state, "running");
        assert_eq!(playing.device.as_deref(), Some(VOLT));
        assert_eq!(playing.first_channel, Some(2));
        assert_eq!(
            session.local().and_then(|local| local.output),
            Some(output(VOLT, 2))
        );
        assert_eq!(*notice.lock().expect("notice"), None);
    }

    #[test]
    fn an_output_the_engine_refuses_is_not_kept_and_the_one_in_use_keeps_playing() {
        let session = SessionStore::open(&session_directory("output-refused"));
        session
            .set_local_output(Some(output(SPEAKERS, 0)))
            .expect("kept");
        let engine = running(LocalStandIn {
            output: playing(SPEAKERS, 0),
            ..two_inputs_two_groups()
        });
        let notice = Mutex::new(None);

        for (device, first) in [("Headphones (USB)", 0), (VOLT, 4)] {
            let error = choose_output(&engine, &session, &notice, choice(device, first))
                .expect_err("refused");
            assert!(error.contains(device), "{error}");
        }
        let error =
            choose_output(&engine, &session, &notice, choice("", 0)).expect_err("no device named");
        assert!(error.contains("name"), "{error}");

        assert_eq!(
            session.local().and_then(|local| local.output),
            Some(output(SPEAKERS, 0)),
            "the kept output stays"
        );
        assert_eq!(
            engine_of(&engine).as_ref().expect("running").output,
            playing(SPEAKERS, 0),
            "and keeps playing"
        );
    }

    #[test]
    fn turning_the_output_off_is_taken_and_kept_and_a_stopping_engine_refuses_it() {
        let session = SessionStore::open(&session_directory("output-off"));
        session
            .set_local_output(Some(output(SPEAKERS, 0)))
            .expect("kept");
        let engine = running(LocalStandIn {
            output: playing(SPEAKERS, 0),
            ..two_inputs_two_groups()
        });
        let notice = Mutex::new(None);
        let stopping = running(LocalStandIn {
            stopping: true,
            ..two_inputs_two_groups()
        });

        let error = choose_output(&stopping, &session, &notice, None).expect_err("refused");
        assert!(error.contains("stopping"), "{error}");
        assert!(session.local().and_then(|local| local.output).is_some());

        let outcome = choose_output(&engine, &session, &notice, None).expect("taken");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(outcome.output, None);
        assert_eq!(
            outcome.local_output.expect("the engine's word").state,
            "off"
        );
        assert_eq!(session.local().and_then(|local| local.output), None);
    }

    #[test]
    fn with_no_engine_an_output_is_kept_for_the_next_start() {
        let session = SessionStore::open(&session_directory("output-idle"));
        let idle: Mutex<Option<LocalStandIn>> = Mutex::new(None);
        let notice = Mutex::new(None);

        let outcome = choose_output(&idle, &session, &notice, choice(VOLT, 0)).expect("kept");

        assert!(!outcome.applied, "it applies at the next start");
        assert!(outcome.saved);
        assert_eq!(outcome.local_output, None);
        assert_eq!(
            session.local().and_then(|local| local.output),
            Some(output(VOLT, 0))
        );
    }

    fn kept_local(output: Option<SessionOutput>) -> SessionLocal {
        SessionLocal {
            fx_returns: None,
            limiter: None,
            mix: Some(SessionMix {
                channels: vec![channel(80)],
                groups: BTreeMap::from([
                    ("vox".to_owned(), 30),
                    ("kit".to_owned(), 70),
                    ("gone".to_owned(), 5),
                ]),
                volume_percent: 85,
                muted: true,
            }),
            output,
        }
    }

    #[test]
    fn a_start_pushes_the_kept_local_mix_by_group_id_and_then_opens_the_kept_output() {
        let mut engine = LocalStandIn {
            groups: vec!["kit".to_owned(), "vox".to_owned(), "keys".to_owned()],
            ..two_inputs_two_groups()
        };
        engine.local.group_levels[2] = 55;

        let applied = apply_kept_local(&mut engine, &kept_local(Some(output(VOLT, 2))));

        assert_eq!(applied.notice, None);
        assert_eq!(applied.refused, None);
        assert_eq!(
            engine.updates,
            vec![MixUpdate {
                channel_gains: Some(vec![80]),
                pans: Some(vec![50]),
                channel_muted: Some(vec![false]),
                channel_solo: Some(vec![false]),
                group_levels: Some(vec![70, 30, 55]),
                volume_percent: Some(85),
                max_level_percent: None,
                muted: Some(true),
            }],
            "each level lands on its group by id, a group the mix does not name keeps its own, \
             and no level cap is ever pushed"
        );
        assert_eq!(applied.mix.map(|mix| mix.volume_percent), Some(85));
        assert_eq!(engine.output, playing(VOLT, 2));
        let order = vec![
            "set_local_mix".to_owned(),
            format!("set_local_output {VOLT}"),
        ];
        assert_eq!(engine.log, order, "the mix first, then the output");
    }

    #[test]
    fn a_kept_output_the_running_driver_does_not_offer_starts_off_with_a_notice_never_another_device(
    ) {
        // Under ASIO the engine offers only the capture driver's own outputs.
        let mut engine = LocalStandIn {
            offered: vec![device(VOLT_ASIO, 4)],
            ..two_inputs_two_groups()
        };

        let applied = apply_kept_local(&mut engine, &kept_local(Some(output(VOLT, 0))));

        let notice = applied.notice.expect("said");
        assert!(
            notice.contains(&format!("{VOLT} · outputs 1/2")),
            "{notice}"
        );
        assert!(notice.contains("does not offer"), "{notice}");
        assert_eq!(engine.output, LocalOutputStatus::default(), "off");
        assert!(
            !engine
                .log
                .iter()
                .any(|step| step.starts_with("set_local_output")),
            "nothing is opened in its place: {:?}",
            engine.log
        );
        assert!(applied.mix.is_some(), "the kept mix still plays");
    }

    #[test]
    fn a_kept_pair_past_the_device_or_one_that_cannot_open_starts_off_with_a_notice() {
        let mut past = two_inputs_two_groups();
        let applied = apply_kept_local(&mut past, &kept_local(Some(output(SPEAKERS, 2))));
        let notice = applied.notice.expect("said");
        assert!(notice.contains("outputs 3/4"), "{notice}");
        assert_eq!(past.output, LocalOutputStatus::default());

        let mut broken = LocalStandIn {
            broken: Some((VOLT.to_owned(), "the device runs at 44.1 kHz".to_owned())),
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut broken, &kept_local(Some(output(VOLT, 0))));
        let notice = applied.notice.expect("said");
        assert!(notice.contains("could not be opened"), "{notice}");
        assert!(notice.contains("44.1 kHz"), "{notice}");
        assert_eq!(broken.output, LocalOutputStatus::default());

        let mut unlisted = LocalStandIn {
            list_error: Some("no audio service".to_owned()),
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut unlisted, &kept_local(Some(output(VOLT, 0))));
        assert!(applied
            .notice
            .expect("said")
            .contains("could not be listed"));
    }

    #[test]
    fn a_loaded_output_already_playing_is_left_alone_another_stops_and_none_turns_it_off() {
        let mut same = LocalStandIn {
            output: playing(VOLT, 2),
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut same, &kept_local(Some(output(VOLT, 2))));
        assert_eq!(applied.notice, None);
        assert!(
            !same
                .log
                .iter()
                .any(|step| step.starts_with("set_local_output")),
            "{:?}",
            same.log
        );

        // The loaded session's output is not offered: what played before stops.
        let mut elsewhere = LocalStandIn {
            output: playing(SPEAKERS, 0),
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut elsewhere, &kept_local(Some(output(VOLT_ASIO, 0))));
        assert!(applied.notice.is_some());
        assert_eq!(elsewhere.output, LocalOutputStatus::default());

        let mut playing_now = LocalStandIn {
            output: playing(SPEAKERS, 0),
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut playing_now, &kept_local(None));
        assert_eq!(applied.notice, None);
        assert_eq!(
            playing_now.output,
            LocalOutputStatus::default(),
            "a session with no output turns it off"
        );
    }

    #[test]
    fn a_kept_section_without_a_mix_leaves_the_engines_mix_and_a_stopping_engine_is_said() {
        let mut engine = two_inputs_two_groups();
        let applied = apply_kept_local(
            &mut engine,
            &SessionLocal {
                fx_returns: None,
                limiter: None,
                mix: None,
                output: Some(output(SPEAKERS, 0)),
            },
        );
        assert!(engine.updates.is_empty(), "the engine's default mix plays");
        assert_eq!(applied.mix, None);
        assert_eq!(engine.output, playing(SPEAKERS, 0));

        let mut stopping = LocalStandIn {
            stopping: true,
            ..two_inputs_two_groups()
        };
        let applied = apply_kept_local(&mut stopping, &kept_local(None));
        assert!(applied
            .refused
            .expect("said")
            .contains("local mix was not applied"));
    }
}
