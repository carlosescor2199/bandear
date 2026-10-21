//! The virtual soundcheck from the desk: the `start_soundcheck`, `pause_soundcheck`,
//! `resume_soundcheck`, `seek_soundcheck`, `set_soundcheck_loop` and `stop_soundcheck` commands
//! over the engine's soundcheck ([EngineHandle::start_soundcheck] and the rest).
//!
//! A soundcheck plays a recorded take back into the mixer in place of the live inputs: the phones,
//! the local output and the meters hear the take as they would hear the band, so mixes can be set
//! with nobody playing. The capture device stays open, set aside, and stopping the soundcheck goes
//! back to it at once ("back to live inputs"). There is no separate "listen only" playback: the
//! takes list's Play is a soundcheck start, and so is the Record screen's Play, so a take is always
//! heard through the engine, on the local output and on every phone.
//!
//! Only a take folder directly inside the recordings folder, with its `take.json`, is played,
//! checked on the disk first as `open_take_folder` checks it. The engine refuses a take whose
//! tracks do not fit the device's inputs now, a take at another rate than 48 kHz, a start while a
//! take records, and recording or a device switch while a soundcheck plays; each refusal is said in
//! words ([words]). Every answer is the soundcheck's status (see [SoundcheckDto]), which the
//! engine's status carries too.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Mutex;

use personal_monitoring::log_info;

use personal_monitoring::{EngineHandle, SoundcheckError, SoundcheckRequest, SoundcheckStatus};

use super::recording::{places, take_in_recordings, Places};
use super::DesktopState;
use crate::dto::SoundcheckDto;
use crate::session::SessionStore;

/// What the soundcheck needs from the running engine: the engine in the app, a stand-in in the
/// tests, which run without audio hardware. Refusals are the engine's own, typed.
pub(super) trait SoundcheckEngine {
    fn start_soundcheck(
        &mut self,
        request: SoundcheckRequest,
    ) -> Result<SoundcheckStatus, SoundcheckError>;
    fn pause_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError>;
    fn resume_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError>;
    fn seek_soundcheck(&self, frame: u64) -> Result<SoundcheckStatus, SoundcheckError>;
    fn set_soundcheck_loop(&self, looping: bool) -> Result<SoundcheckStatus, SoundcheckError>;
    fn stop_soundcheck(&mut self) -> Result<SoundcheckStatus, SoundcheckError>;
}

/// The engine's refusal as its [SoundcheckError]; any other error (none is expected) is said as
/// the soundcheck being unavailable, with its words.
fn typed(error: Box<dyn Error>) -> SoundcheckError {
    match error.downcast::<SoundcheckError>() {
        Ok(refusal) => *refusal,
        Err(other) => SoundcheckError::Unavailable {
            reason: other.to_string(),
        },
    }
}

impl SoundcheckEngine for EngineHandle {
    fn start_soundcheck(
        &mut self,
        request: SoundcheckRequest,
    ) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::start_soundcheck(self, request).map_err(typed)
    }

    fn pause_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::pause_soundcheck(self).map_err(typed)
    }

    fn resume_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::resume_soundcheck(self).map_err(typed)
    }

    fn seek_soundcheck(&self, frame: u64) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::seek_soundcheck(self, frame).map_err(typed)
    }

    fn set_soundcheck_loop(&self, looping: bool) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::set_soundcheck_loop(self, looping).map_err(typed)
    }

    fn stop_soundcheck(&mut self) -> Result<SoundcheckStatus, SoundcheckError> {
        EngineHandle::stop_soundcheck(self).map_err(typed)
    }
}

/// The rate every take plays at.
const TAKE_SAMPLE_RATE: u32 = 48_000;

/// `frames` of a take in seconds, as the refusals say them: "2.5 s".
fn seconds(frames: u64) -> String {
    format!("{:.1} s", frames as f64 / f64::from(TAKE_SAMPLE_RATE))
}

/// The words the window says for `refusal`, one per kind (a sentence without its full stop, the
/// way every command's error is said).
pub fn refusal_words(refusal: &SoundcheckError) -> String {
    match refusal {
        SoundcheckError::EngineStopping => {
            "the engine is stopping: start it again to soundcheck a take".to_owned()
        }
        SoundcheckError::Recording => {
            "a take is recording: stop the recording before the soundcheck".to_owned()
        }
        SoundcheckError::AlreadyPlaying => {
            "a soundcheck is already playing a take: go back to the live inputs first".to_owned()
        }
        SoundcheckError::NotPlaying => "no soundcheck is playing".to_owned(),
        SoundcheckError::RecordingRefused => {
            "a soundcheck is playing a take: go back to the live inputs before recording (a take \
             never records playback)"
                .to_owned()
        }
        SoundcheckError::SwitchRefused => {
            "a soundcheck is playing a take: go back to the live inputs before switching the \
             input device"
                .to_owned()
        }
        SoundcheckError::DemoSource => {
            "the engine runs on the demo source: a take plays in place of an interface's inputs, \
             so choose an interface's driver first"
                .to_owned()
        }
        SoundcheckError::TakeUnreadable { reason } => {
            format!("the take cannot be read: {reason}")
        }
        SoundcheckError::TrackUnreadable { file, reason } => {
            format!("the take's file \"{file}\" cannot be played: {reason}")
        }
        SoundcheckError::SampleRate {
            file: None,
            sample_rate,
        } => format!(
            "the take was recorded at {sample_rate} Hz; only {TAKE_SAMPLE_RATE} Hz takes play \
             (there is no resampler)"
        ),
        SoundcheckError::SampleRate {
            file: Some(file),
            sample_rate,
        } => format!(
            "the take's file \"{file}\" is at {sample_rate} Hz; only {TAKE_SAMPLE_RATE} Hz files \
             play (there is no resampler)"
        ),
        SoundcheckError::Layout { reason } => format!(
            "the take does not fit this device's inputs now: {reason}; play it on the device and \
             with the linked pairs it was recorded with"
        ),
        SoundcheckError::Empty => "the take has no input audio to play".to_owned(),
        SoundcheckError::PastEnd { frame, length } => format!(
            "{} is past the end of the take ({})",
            seconds(*frame),
            seconds(*length)
        ),
        SoundcheckError::Failed { reason } => format!(
            "the soundcheck stopped playing: {reason}; go back to the live inputs to stop it"
        ),
        SoundcheckError::Unavailable { reason } => {
            format!("the soundcheck could not start: {reason}")
        }
    }
}

/// The words for an engine error: a soundcheck refusal as [refusal_words] says it, anything else
/// as it is.
pub fn words(error: &(dyn Error + 'static)) -> String {
    match error.downcast_ref::<SoundcheckError>() {
        Some(refusal) => refusal_words(refusal),
        None => error.to_string(),
    }
}

/// Plays the take in `take_folder` (a take folder of the recordings folder) in place of the live
/// inputs, from `start_frame` (its start when none), looping when asked, and answers the
/// soundcheck. An error, and the inputs left live, with no engine running, a folder that is not a
/// take of the recordings folder, or the engine's refusal (see the module documentation).
pub fn start_soundcheck(
    state: &DesktopState,
    take_folder: String,
    start_frame: Option<u64>,
    looping: Option<bool>,
) -> Result<SoundcheckDto, String> {
    start_with(
        &state.engine,
        &state.session,
        places(state),
        &take_folder,
        start_frame.unwrap_or(0),
        looping.unwrap_or(false),
    )
}

/// Pauses the take: silence, the position held. An error when no take plays.
pub fn pause_soundcheck(state: &DesktopState) -> Result<SoundcheckDto, String> {
    transport_with(&state.engine, Transport::Pause)
}

/// Plays on; a take at its end (stopped there, or paused and moved there) starts again from its
/// first frame.
pub fn resume_soundcheck(state: &DesktopState) -> Result<SoundcheckDto, String> {
    transport_with(&state.engine, Transport::Resume)
}

/// Moves the take to `frame` (the take's end at most): a paused take stays paused there, a take
/// that stopped at its end plays on from there.
pub fn seek_soundcheck(state: &DesktopState, frame: u64) -> Result<SoundcheckDto, String> {
    transport_with(&state.engine, Transport::Seek(frame))
}

/// Whether the take starts again from its first frame at its end.
pub fn set_soundcheck_loop(state: &DesktopState, looping: bool) -> Result<SoundcheckDto, String> {
    transport_with(&state.engine, Transport::Loop(looping))
}

/// A change of how the take plays, made with the engine shared (no `&mut`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport {
    Pause,
    Resume,
    Seek(u64),
    Loop(bool),
}

impl Transport {
    fn apply<E: SoundcheckEngine>(self, engine: &E) -> Result<SoundcheckStatus, SoundcheckError> {
        match self {
            Transport::Pause => engine.pause_soundcheck(),
            Transport::Resume => engine.resume_soundcheck(),
            Transport::Seek(frame) => engine.seek_soundcheck(frame),
            Transport::Loop(looping) => engine.set_soundcheck_loop(looping),
        }
    }
}

/// Stops the take and goes back to the live inputs at once, and answers the soundcheck (off).
pub fn stop_soundcheck(state: &DesktopState) -> Result<SoundcheckDto, String> {
    stop_with(&state.engine)
}

/// [start_soundcheck] over any engine slot, session and places.
fn start_with<E: SoundcheckEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    places: Places<'_>,
    take_folder: &str,
    start_frame: u64,
    looping: bool,
) -> Result<SoundcheckDto, String> {
    start_checked(
        engine,
        || take_in_recordings(session, places, take_folder),
        start_frame,
        looping,
    )
}

/// [start_with] with the take folder checked by `check`, which runs without the engine's lock.
fn start_checked<E: SoundcheckEngine>(
    engine: &Mutex<Option<E>>,
    check: impl FnOnce() -> Result<PathBuf, String>,
    start_frame: u64,
    looping: bool,
) -> Result<SoundcheckDto, String> {
    const NOT_RUNNING: &str = "the engine is not running: start it to soundcheck a take";
    if engine_lock(engine)?.is_none() {
        return Err(NOT_RUNNING.to_owned());
    }
    // The folder is checked on the disk (and the session read) before the engine's lock is taken,
    // so a slow disk never holds up the meters, the status or the transport. The engine's own load
    // of the take (take.json and each file's header, `EngineHandle::start_soundcheck`) runs on this
    // thread too, under the lock: it needs the engine's channel layout as it is when the take
    // starts.
    let take = check()?;
    let mut guard = engine_lock(engine)?;
    let engine = guard.as_mut().ok_or_else(|| NOT_RUNNING.to_owned())?;
    let request = SoundcheckRequest {
        take_folder: take,
        start_frame,
        looping,
    };
    let status = engine
        .start_soundcheck(request.clone())
        .map_err(|refusal| refusal_words(&refusal))?;
    log_info!(
        "soundcheck",
        "start_soundcheck: \"{}\" from frame {start_frame}, looping={looping}",
        request.take_folder.display()
    );
    Ok(SoundcheckDto::from(&status))
}

/// A transport command over any engine slot.
fn transport_with<E: SoundcheckEngine>(
    engine: &Mutex<Option<E>>,
    change: Transport,
) -> Result<SoundcheckDto, String> {
    let guard = engine_lock(engine)?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running, so no soundcheck is playing".to_owned())?;
    let status = change
        .apply(engine)
        .map_err(|refusal| refusal_words(&refusal))?;
    log_info!("soundcheck", "soundcheck {change:?}");
    Ok(SoundcheckDto::from(&status))
}

/// [stop_soundcheck] over any engine slot.
fn stop_with<E: SoundcheckEngine>(engine: &Mutex<Option<E>>) -> Result<SoundcheckDto, String> {
    let mut guard = engine_lock(engine)?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| "the engine is not running, so no soundcheck is playing".to_owned())?;
    let status = engine
        .stop_soundcheck()
        .map_err(|refusal| refusal_words(&refusal))?;
    log_info!("soundcheck", "stop_soundcheck: back to the live inputs");
    Ok(SoundcheckDto::from(&status))
}

fn engine_lock<E>(
    engine: &Mutex<Option<E>>,
) -> Result<std::sync::MutexGuard<'_, Option<E>>, String> {
    engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::commands::recording::RecordingsFolder;
    use crate::session::SessionRecording;
    use personal_monitoring::{SoundcheckClock, SoundcheckState};

    /// An empty scratch directory for one test.
    fn scratch(test: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bandear-soundcheck-tests")
            .join(test);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        directory
    }

    /// A desk for one test: its config folder (with the session in it) and its default recordings
    /// folder.
    struct Desk {
        root: PathBuf,
        config: PathBuf,
        session: SessionStore,
        default: RecordingsFolder,
    }

    impl Desk {
        fn new(test: &str) -> Self {
            let root = scratch(test);
            let config = root.join("config");
            let session = SessionStore::open(&config);
            let default = RecordingsFolder {
                folder: root.join("Music").join("BandEar").join("Recordings"),
                notice: None,
            };
            Self {
                root,
                config,
                session,
                default,
            }
        }

        fn places(&self) -> Places<'_> {
            Places {
                default: &self.default,
                config: &self.config,
            }
        }
    }

    /// A take folder called `name` in `folder`, with a `take.json`.
    fn take_folder(folder: &Path, name: &str) -> PathBuf {
        let take = folder.join(name);
        fs::create_dir_all(&take).expect("take folder");
        fs::write(take.join("take.json"), r#"{ "version": 1 }"#).expect("take.json");
        take
    }

    /// Stands in for a running engine's soundcheck: it takes what it is asked, as the engine does,
    /// and refuses what the test set.
    #[derive(Default)]
    struct SoundcheckStandIn {
        status: SoundcheckStatus,
        /// Every start it took, in order.
        requests: Vec<SoundcheckRequest>,
        /// Every transport change it took, in order.
        changes: Mutex<Vec<String>>,
        /// Why it refuses every call, when it does.
        refuse: Option<SoundcheckError>,
    }

    impl SoundcheckStandIn {
        fn refused(&self) -> Result<(), SoundcheckError> {
            match &self.refuse {
                Some(refusal) => Err(refusal.clone()),
                None => Ok(()),
            }
        }

        fn change(&self, what: String) -> Result<SoundcheckStatus, SoundcheckError> {
            self.refused()?;
            self.changes.lock().expect("changes").push(what);
            Ok(self.status.clone())
        }
    }

    impl SoundcheckEngine for SoundcheckStandIn {
        fn start_soundcheck(
            &mut self,
            request: SoundcheckRequest,
        ) -> Result<SoundcheckStatus, SoundcheckError> {
            self.refused()?;
            self.status = SoundcheckStatus {
                state: SoundcheckState::Playing,
                take_folder: Some(request.take_folder.clone()),
                take_name: Some("Show".to_owned()),
                position_frames: request.start_frame,
                length_frames: 480_000,
                looping: request.looping,
                underrun_frames: 0,
                clock: SoundcheckClock::Timer,
            };
            self.requests.push(request);
            Ok(self.status.clone())
        }

        fn pause_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError> {
            self.change("pause".to_owned())
        }

        fn resume_soundcheck(&self) -> Result<SoundcheckStatus, SoundcheckError> {
            self.change("resume".to_owned())
        }

        fn seek_soundcheck(&self, frame: u64) -> Result<SoundcheckStatus, SoundcheckError> {
            self.change(format!("seek {frame}"))
        }

        fn set_soundcheck_loop(&self, looping: bool) -> Result<SoundcheckStatus, SoundcheckError> {
            self.change(format!("loop {looping}"))
        }

        fn stop_soundcheck(&mut self) -> Result<SoundcheckStatus, SoundcheckError> {
            self.refused()?;
            self.status = SoundcheckStatus::default();
            self.changes
                .lock()
                .expect("changes")
                .push("stop".to_owned());
            Ok(self.status.clone())
        }
    }

    fn running(engine: SoundcheckStandIn) -> Mutex<Option<SoundcheckStandIn>> {
        Mutex::new(Some(engine))
    }

    fn requests_of(engine: &Mutex<Option<SoundcheckStandIn>>) -> Vec<SoundcheckRequest> {
        engine
            .lock()
            .expect("engine")
            .as_ref()
            .map(|engine| engine.requests.clone())
            .unwrap_or_default()
    }

    fn changes_of(engine: &Mutex<Option<SoundcheckStandIn>>) -> Vec<String> {
        engine
            .lock()
            .expect("engine")
            .as_ref()
            .map(|engine| engine.changes.lock().expect("changes").clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_take_of_the_recordings_folder_plays_from_its_frame_looping_as_asked() {
        let desk = Desk::new("start");
        let take = take_folder(&desk.default.folder, "2026-10-16_190000_Show, take 2");
        let engine = running(SoundcheckStandIn::default());

        let answer = start_with(
            &engine,
            &desk.session,
            desk.places(),
            &format!("  {}  ", take.display()),
            96_000,
            true,
        )
        .expect("the take plays");

        assert_eq!(
            requests_of(&engine),
            vec![SoundcheckRequest {
                take_folder: desk.default.folder.join("2026-10-16_190000_Show, take 2"),
                start_frame: 96_000,
                looping: true,
            }]
        );
        assert_eq!(answer.state, "playing");
        assert_eq!(answer.take_name.as_deref(), Some("Show"));
        assert_eq!(answer.position_frames, 96_000);
        assert_eq!(answer.position_seconds, 2.0);
        assert!(answer.looping);
    }

    #[test]
    fn a_take_of_a_kept_recordings_folder_plays_too() {
        let desk = Desk::new("kept-folder");
        let kept = desk.root.join("Takes");
        desk.session
            .set_recording(SessionRecording {
                folder: Some(kept.display().to_string()),
                ..SessionRecording::default()
            })
            .expect("kept");
        let take = take_folder(&kept, "2026-10-16_190000_Show");
        let engine = running(SoundcheckStandIn::default());

        start_with(
            &engine,
            &desk.session,
            desk.places(),
            &take.display().to_string(),
            0,
            false,
        )
        .expect("the take plays");

        assert_eq!(requests_of(&engine)[0].take_folder, take);
    }

    #[test]
    fn only_a_take_folder_inside_the_recordings_folder_is_played() {
        let desk = Desk::new("refused-folders");
        let folder = desk.default.folder.clone();
        let take = take_folder(&folder, "2026-10-16_190000_Show");
        let elsewhere = take_folder(&desk.root, "2026-10-16_190000_Elsewhere");
        let plain = folder.join("Not a take");
        fs::create_dir_all(&plain).expect("plain folder");
        let engine = running(SoundcheckStandIn::default());

        for refused in [
            elsewhere.display().to_string(),
            folder.display().to_string(),
            plain.display().to_string(),
            take.join("..").display().to_string(),
            take.join("..")
                .join("..")
                .join("2026-10-16_190000_Elsewhere")
                .display()
                .to_string(),
            "2026-10-16_190000_Show".to_owned(),
            folder.join("missing").display().to_string(),
            String::new(),
        ] {
            let error = start_with(&engine, &desk.session, desk.places(), &refused, 0, false)
                .expect_err("refused");
            assert!(error.contains("not a take folder"), "{refused}: {error}");
        }
        assert!(requests_of(&engine).is_empty(), "nothing was played");
    }

    #[test]
    fn a_soundcheck_with_no_engine_running_is_refused() {
        let desk = Desk::new("no-engine");
        let take = take_folder(&desk.default.folder, "2026-10-16_190000_Show");
        let engine: Mutex<Option<SoundcheckStandIn>> = Mutex::new(None);

        let error = start_with(
            &engine,
            &desk.session,
            desk.places(),
            &take.display().to_string(),
            0,
            false,
        )
        .expect_err("no engine");
        assert!(error.contains("the engine is not running"), "{error}");
        for error in [
            transport_with(&engine, Transport::Pause),
            stop_with(&engine),
        ] {
            let error = error.expect_err("no engine");
            assert!(error.contains("the engine is not running"), "{error}");
        }
    }

    /// The take folder is checked on the disk with the engine's lock free (the meters, the status
    /// and the transport go on meanwhile), and only then is the take started under it; a folder
    /// refused leaves the engine untouched, and with no engine nothing is checked.
    #[test]
    fn the_take_folder_is_checked_without_holding_the_engine_lock() {
        let engine = running(SoundcheckStandIn::default());
        let folder = PathBuf::from(r"D:\Recordings\2026-10-16_190000_Show");
        let lock_free = std::cell::Cell::new(None);

        start_checked(
            &engine,
            || {
                lock_free.set(Some(engine.try_lock().is_ok()));
                Ok(folder.clone())
            },
            0,
            false,
        )
        .expect("the take plays");

        assert_eq!(lock_free.get(), Some(true), "the engine's lock was free");
        assert_eq!(
            requests_of(&engine),
            vec![SoundcheckRequest {
                take_folder: folder,
                start_frame: 0,
                looping: false,
            }]
        );

        let untouched = running(SoundcheckStandIn::default());
        let error = start_checked(&untouched, || Err("not a take folder".to_owned()), 0, false)
            .expect_err("refused");
        assert_eq!(error, "not a take folder");
        assert!(requests_of(&untouched).is_empty());

        let none: Mutex<Option<SoundcheckStandIn>> = Mutex::new(None);
        let error =
            start_checked(&none, || panic!("nothing is checked"), 0, false).expect_err("no engine");
        assert!(error.contains("the engine is not running"), "{error}");
    }

    #[test]
    fn every_refusal_of_the_engine_is_said_in_words() {
        let desk = Desk::new("refusals");
        let take = take_folder(&desk.default.folder, "2026-10-16_190000_Show");
        for (refusal, said) in [
            (SoundcheckError::EngineStopping, "the engine is stopping"),
            (
                SoundcheckError::Recording,
                "a take is recording: stop the recording before the soundcheck",
            ),
            (
                SoundcheckError::AlreadyPlaying,
                "a soundcheck is already playing a take",
            ),
            (SoundcheckError::NotPlaying, "no soundcheck is playing"),
            (
                SoundcheckError::RecordingRefused,
                "go back to the live inputs before recording",
            ),
            (
                SoundcheckError::SwitchRefused,
                "go back to the live inputs before switching the input device",
            ),
            (
                SoundcheckError::TakeUnreadable {
                    reason: "take.json is not JSON".to_owned(),
                },
                "the take cannot be read: take.json is not JSON",
            ),
            (
                SoundcheckError::TrackUnreadable {
                    file: "01 Kick.wav".to_owned(),
                    reason: "it is missing".to_owned(),
                },
                "the take's file \"01 Kick.wav\" cannot be played: it is missing",
            ),
            (
                SoundcheckError::SampleRate {
                    file: None,
                    sample_rate: 44_100,
                },
                "the take was recorded at 44100 Hz",
            ),
            (
                SoundcheckError::SampleRate {
                    file: Some("01 Kick.wav".to_owned()),
                    sample_rate: 96_000,
                },
                "\"01 Kick.wav\" is at 96000 Hz",
            ),
            (
                SoundcheckError::Layout {
                    reason: "CH 9 is past the device's 8 inputs".to_owned(),
                },
                "the take does not fit this device's inputs now: CH 9 is past the device's 8 inputs",
            ),
            (SoundcheckError::Empty, "the take has no input audio to play"),
            (
                SoundcheckError::PastEnd {
                    frame: 96_000,
                    length: 48_000,
                },
                "2.0 s is past the end of the take (1.0 s)",
            ),
            (
                SoundcheckError::Failed {
                    reason: "reading \"01 Kick.wav\" failed".to_owned(),
                },
                "the soundcheck stopped playing: reading \"01 Kick.wav\" failed",
            ),
            (
                SoundcheckError::Unavailable {
                    reason: "no disk thread".to_owned(),
                },
                "the soundcheck could not start: no disk thread",
            ),
        ] {
            let engine = running(SoundcheckStandIn {
                refuse: Some(refusal.clone()),
                ..SoundcheckStandIn::default()
            });
            let error = start_with(
                &engine,
                &desk.session,
                desk.places(),
                &take.display().to_string(),
                0,
                false,
            )
            .expect_err("refused");
            assert!(error.contains(said), "{refusal:?}: {error}");
            assert_eq!(words(&refusal), error, "the same words wherever it is said");
        }
    }

    #[test]
    fn an_engine_error_that_is_no_refusal_is_said_as_it_is() {
        let error: Box<dyn Error> = "the engine thread has ended".into();
        assert_eq!(words(&*error), "the engine thread has ended");
        assert_eq!(
            typed(error),
            SoundcheckError::Unavailable {
                reason: "the engine thread has ended".to_owned()
            }
        );
        let refusal: Box<dyn Error> = Box::new(SoundcheckError::NotPlaying);
        assert_eq!(typed(refusal), SoundcheckError::NotPlaying);
    }

    #[test]
    fn the_transport_reaches_the_engine_and_answers_the_soundcheck() {
        let engine = running(SoundcheckStandIn {
            status: SoundcheckStatus {
                state: SoundcheckState::Paused,
                position_frames: 24_000,
                length_frames: 48_000,
                ..SoundcheckStatus::default()
            },
            ..SoundcheckStandIn::default()
        });

        let paused = transport_with(&engine, Transport::Pause).expect("paused");
        assert_eq!(paused.state, "paused");
        assert_eq!(paused.position_seconds, 0.5);
        transport_with(&engine, Transport::Resume).expect("resumed");
        transport_with(&engine, Transport::Seek(12_000)).expect("moved");
        transport_with(&engine, Transport::Loop(true)).expect("looping");
        let stopped = stop_with(&engine).expect("stopped");

        assert_eq!(
            changes_of(&engine),
            vec!["pause", "resume", "seek 12000", "loop true", "stop"]
        );
        assert_eq!(stopped.state, "off");
        assert_eq!(stopped.take_folder, None);
    }

    #[test]
    fn a_transport_change_with_nothing_playing_is_said_in_words() {
        let engine = running(SoundcheckStandIn {
            refuse: Some(SoundcheckError::NotPlaying),
            ..SoundcheckStandIn::default()
        });

        let error = transport_with(&engine, Transport::Seek(0)).expect_err("nothing plays");
        assert_eq!(error, "no soundcheck is playing");
        let error = stop_with(&engine).expect_err("nothing plays");
        assert_eq!(error, "no soundcheck is playing");
    }
}
