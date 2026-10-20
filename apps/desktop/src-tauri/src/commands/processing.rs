//! Each input's channel processing set from the desk: the commands that read and change a channel's
//! strip (the high-pass filter, the four-band equaliser, the gate and the compressor; see
//! [personal_monitoring::StripSettings]).
//!
//! - Channels are numbered from 1 here, as the desk numbers them everywhere it keeps them: CH 1 is
//!   `1`. The mixer has 32 ([MIXER_CHANNELS]); a number outside 1-32 is an `invalid` outcome and
//!   nothing changes.
//! - A change needs a running engine (an error without one, and nothing changes): the engine holds
//!   each number in its range and answers what it keeps, and a linked pair is one strip, so setting
//!   either side sets both. Every answer names each channel whose settings changed, a pair's other
//!   side included. The engine plays a change from its next block, gliding to it without a click.
//! - The processing is kept in the session ([crate::session], `processing`) off the engine lock:
//!   a change is only marked ([ProcessingSaver]) and written at most every
//!   [super::local::saver::SAVE_INTERVAL], as the local mix is, since an editor's knob sends many
//!   changes a second. Stopping the engine and quitting the app write what is still waiting. Every
//!   engine the desk starts takes the processing the session keeps (a change still waiting
//!   included), and a loaded session replaces it whole.
//! - With no engine running the reads answer what the session keeps, which the next start applies.

use std::sync::Mutex;

use personal_monitoring::{log_info, log_warn, EngineHandle, StripSettings};
use serde::Serialize;

use super::local::saver::{Kept, Saver};
use super::DesktopState;
use crate::session::{SessionProcessing, SessionStore};

/// The mixer's channels, each with its strip.
pub const MIXER_CHANNELS: usize = 32;

/// What the commands need from a running engine (channels 0-based there: 0 is CH 1).
pub(crate) trait ProcessingEngine {
    /// Sets the strip of `channel` and answers the settings kept ([EngineHandle::set_channel_processing]).
    fn set_channel_processing(
        &self,
        channel: usize,
        settings: StripSettings,
    ) -> Result<StripSettings, String>;
    /// Every channel's strip, CH 1 first ([EngineHandle::processing]).
    fn processing(&self) -> Vec<StripSettings>;
    /// Replaces every channel's strip at once ([EngineHandle::replace_processing]).
    fn replace_processing(&self, settings: &[StripSettings]) -> Result<Vec<StripSettings>, String>;
}

impl ProcessingEngine for EngineHandle {
    fn set_channel_processing(
        &self,
        channel: usize,
        settings: StripSettings,
    ) -> Result<StripSettings, String> {
        EngineHandle::set_channel_processing(self, channel, settings)
            .map_err(|error| error.to_string())
    }

    fn processing(&self) -> Vec<StripSettings> {
        EngineHandle::processing(self)
    }

    fn replace_processing(&self, settings: &[StripSettings]) -> Result<Vec<StripSettings>, String> {
        EngineHandle::replace_processing(self, settings).map_err(|error| error.to_string())
    }
}

impl Kept for SessionProcessing {
    const WHAT: &'static str = "the channel processing";
    const THREAD: &'static str = "mixlink-processing-saver";
}

/// Keeps the channel processing in the session off the engine lock (see the module documentation).
pub(crate) type ProcessingSaver = Saver<SessionProcessing>;

/// One channel's strip as the desk answers it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelProcessingDto {
    /// The channel, 1-based.
    pub channel: usize,
    /// Its strip, every number in range.
    pub settings: StripSettings,
    /// Whether the strip does anything: false for a neutral one, which the audio skips.
    pub active: bool,
}

impl ChannelProcessingDto {
    fn of(channel: usize, settings: StripSettings) -> Self {
        Self {
            channel,
            settings,
            active: !settings.is_neutral(),
        }
    }
}

/// What a change of channel processing did.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingSetDto {
    /// `set`, or `invalid` (a channel outside 1-32, or nothing to copy to) and then nothing
    /// changed.
    pub outcome: &'static str,
    /// Every channel whose strip changed, in channel order, a linked pair's other side included;
    /// empty when nothing changed.
    pub channels: Vec<ChannelProcessingDto>,
    /// The outcome in words.
    pub message: String,
}

impl ProcessingSetDto {
    fn refused(outcome: &'static str, message: String) -> Self {
        Self {
            outcome,
            channels: Vec::new(),
            message,
        }
    }
}

/// Every channel's strip, CH 1 first, for the engine to take ([ProcessingEngine::replace_processing]):
/// `kept`'s by channel number and the rest neutral. A number outside 1-32 (a hand-edited session)
/// is skipped and logged.
pub(crate) fn processing_list(kept: &SessionProcessing) -> Vec<StripSettings> {
    let mut list = vec![StripSettings::default(); MIXER_CHANNELS];
    for (&channel, &settings) in kept {
        match list.get_mut(channel.wrapping_sub(1)) {
            Some(strip) => *strip = settings,
            None => log_warn!(
                "desk",
                "the processing kept for CH {channel} is skipped: the mixer has CH \
                 1-{MIXER_CHANNELS}"
            ),
        }
    }
    list
}

/// The engine's strips (`list`, CH 1 first) as the session keeps them: by channel number, the
/// neutral ones left out.
pub(crate) fn session_processing(list: &[StripSettings]) -> SessionProcessing {
    list.iter()
        .take(MIXER_CHANNELS)
        .enumerate()
        .filter(|(_, settings)| !settings.is_neutral())
        .map(|(index, settings)| (index + 1, *settings))
        .collect()
}

/// The processing a start hands its engine: the session's, or the change marked and not written
/// yet, read with the saver paused so no write lands in between. The change stays marked and is
/// written later.
pub(crate) fn kept_processing(
    session: &SessionStore,
    saver: &ProcessingSaver,
) -> SessionProcessing {
    let paused = saver.pause();
    paused.pending().unwrap_or_else(|| session.processing())
}

/// Hands a new engine the processing `kept` holds, as every start does (see
/// [super::start_engine]), after its channel layout, so a linked pair takes its first side's.
/// Answers why the engine refused it, for the log; a new engine starts neutral, so nothing kept
/// is nothing to hand.
pub(crate) fn hand_kept_processing<E: ProcessingEngine>(
    engine: &E,
    kept: &SessionProcessing,
) -> Option<String> {
    if kept.is_empty() {
        return None;
    }
    engine
        .replace_processing(&processing_list(kept))
        .err()
        .map(|error| format!("the saved channel processing was not applied: {error}"))
}

/// Writes the processing still waiting to be kept, now: when the app quits. Never under the engine
/// lock.
pub fn flush_processing(state: &DesktopState) {
    state.processing_saver.flush();
}

/// Every channel's strip, CH 1 first: the running engine's, or with none what the session keeps.
pub fn processing(state: &DesktopState) -> Result<Vec<ChannelProcessingDto>, String> {
    all_with(&state.engine, &state.session, &state.processing_saver)
}

/// The strip of `channel` (1-based): the running engine's, or with none what the session keeps.
/// A channel outside 1-32 is an error.
pub fn channel_processing(
    state: &DesktopState,
    channel: usize,
) -> Result<ChannelProcessingDto, String> {
    check_channel(channel).map_err(|why| format!("{why}."))?;
    let all = processing(state)?;
    all.into_iter()
        .nth(channel - 1)
        .ok_or_else(|| format!("CH {channel} has no strip"))
}

/// Sets the strip of `channel` (1-based) to `settings` and answers what was done
/// ([ProcessingSetDto]): the settings kept, every number in range, on the channel and on the other
/// side of its linked pair. Kept in the session off the engine lock. No engine running, an engine
/// that is stopping or one that cannot take changes this fast is an error, and nothing changed.
pub fn set_channel_processing(
    state: &DesktopState,
    channel: usize,
    settings: StripSettings,
) -> Result<ProcessingSetDto, String> {
    let outcome = set_with(&state.engine, &state.processing_saver, channel, settings)?;
    log_info!(
        "desk",
        "set_channel_processing: CH {channel} -> {}",
        outcome.outcome
    );
    Ok(outcome)
}

/// Takes every part of the strip of `channel` (1-based) off: the channel goes back to no
/// processing, as [set_channel_processing] sets it.
pub fn reset_channel_processing(
    state: &DesktopState,
    channel: usize,
) -> Result<ProcessingSetDto, String> {
    let outcome = reset_with(&state.engine, &state.processing_saver, channel)?;
    log_info!(
        "desk",
        "reset_channel_processing: CH {channel} -> {}",
        outcome.outcome
    );
    Ok(outcome)
}

/// Copies the strip of `from` onto every channel `to` names (all 1-based), as
/// [set_channel_processing] sets each; `from` itself in `to` is left out. A channel outside 1-32,
/// or nothing left to copy to, is `invalid` and nothing changed.
pub fn copy_channel_processing(
    state: &DesktopState,
    from: usize,
    to: Vec<usize>,
) -> Result<ProcessingSetDto, String> {
    let outcome = copy_with(&state.engine, &state.processing_saver, from, &to)?;
    log_info!(
        "desk",
        "copy_channel_processing: CH {from} -> {to:?}: {}",
        outcome.outcome
    );
    Ok(outcome)
}
/// `Ok` for a channel the mixer has (1-32), else why not.
fn check_channel(channel: usize) -> Result<(), String> {
    if (1..=MIXER_CHANNELS).contains(&channel) {
        Ok(())
    } else {
        Err(format!(
            "CH {channel} is not one of the mixer's channels (CH 1-{MIXER_CHANNELS})"
        ))
    }
}

/// The invalid outcome for `channel`, or `None` when the mixer has it.
fn invalid_channel(channel: usize) -> Option<ProcessingSetDto> {
    check_channel(channel)
        .err()
        .map(|why| ProcessingSetDto::refused("invalid", format!("{why}; nothing changed.")))
}

/// "CH 3", "CH 3 and CH 4", "CH 1, CH 2 and CH 5".
fn channel_list(channels: &[ChannelProcessingDto]) -> String {
    let names: Vec<String> = channels
        .iter()
        .map(|channel| format!("CH {}", channel.channel))
        .collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "is" for one channel, "are" for more.
fn verb(channels: &[ChannelProcessingDto]) -> &'static str {
    if channels.len() == 1 {
        "is"
    } else {
        "are"
    }
}

/// [processing] over any engine slot, session and saver.
fn all_with<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    saver: &ProcessingSaver,
) -> Result<Vec<ChannelProcessingDto>, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let list = match guard.as_ref() {
        Some(engine) => engine.processing(),
        None => processing_list(&kept_processing(session, saver)),
    };
    Ok(list
        .into_iter()
        .take(MIXER_CHANNELS)
        .enumerate()
        .map(|(index, settings)| ChannelProcessingDto::of(index + 1, settings))
        .collect())
}

/// Sets every channel in `targets` (1-based, checked) to `settings` on the running engine, marks
/// what the engine keeps then for the session, and answers every channel whose strip changed (a
/// target is always named). A refusal part of the way through keeps what the engine already took
/// (it plays) and is the error.
fn apply_to<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    saver: &ProcessingSaver,
    targets: &[usize],
    settings: StripSettings,
) -> Result<Vec<ChannelProcessingDto>, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let before = engine.processing();
    let refused = targets
        .iter()
        .find_map(|&channel| engine.set_channel_processing(channel - 1, settings).err());
    let after = engine.processing();
    if after != before {
        // Marked only: the saver writes it after the engine lock is released.
        saver.mark(session_processing(&after));
    }
    if let Some(error) = refused {
        return Err(error);
    }
    Ok(after
        .iter()
        .take(MIXER_CHANNELS)
        .enumerate()
        .filter(|(index, kept)| targets.contains(&(index + 1)) || before.get(*index) != Some(*kept))
        .map(|(index, kept)| ChannelProcessingDto::of(index + 1, *kept))
        .collect())
}

/// [set_channel_processing] over any engine slot and saver.
fn set_with<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    saver: &ProcessingSaver,
    channel: usize,
    settings: StripSettings,
) -> Result<ProcessingSetDto, String> {
    if let Some(invalid) = invalid_channel(channel) {
        return Ok(invalid);
    }
    let channels = apply_to(engine, saver, &[channel], settings)?;
    let message = format!(
        "{} {} set to the new processing.",
        channel_list(&channels),
        verb(&channels)
    );
    Ok(ProcessingSetDto {
        outcome: "set",
        channels,
        message,
    })
}

/// [reset_channel_processing] over any engine slot and saver.
fn reset_with<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    saver: &ProcessingSaver,
    channel: usize,
) -> Result<ProcessingSetDto, String> {
    if let Some(invalid) = invalid_channel(channel) {
        return Ok(invalid);
    }
    let channels = apply_to(engine, saver, &[channel], StripSettings::default())?;
    let message = format!(
        "{} {} back to no processing.",
        channel_list(&channels),
        verb(&channels)
    );
    Ok(ProcessingSetDto {
        outcome: "set",
        channels,
        message,
    })
}

/// [copy_channel_processing] over any engine slot and saver.
fn copy_with<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    saver: &ProcessingSaver,
    from: usize,
    to: &[usize],
) -> Result<ProcessingSetDto, String> {
    for &channel in std::iter::once(&from).chain(to) {
        if let Some(invalid) = invalid_channel(channel) {
            return Ok(invalid);
        }
    }
    let mut targets: Vec<usize> = to
        .iter()
        .copied()
        .filter(|&channel| channel != from)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        return Ok(ProcessingSetDto::refused(
            "invalid",
            format!("There is no channel to copy CH {from}'s processing to; nothing changed."),
        ));
    }
    let settings = {
        let guard = engine
            .lock()
            .map_err(|_| "engine state lock was poisoned".to_owned())?;
        let engine = guard
            .as_ref()
            .ok_or_else(|| "the engine is not running".to_owned())?;
        engine
            .processing()
            .get(from - 1)
            .copied()
            .unwrap_or_default()
    };
    let channels = apply_to(engine, saver, &targets, settings)?;
    let message = format!(
        "CH {from}'s processing is on {} now.",
        channel_list(&channels)
    );
    Ok(ProcessingSetDto {
        outcome: "set",
        channels,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use personal_monitoring::CompressorSettings;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// An engine with 32 strips where CH 3 and CH 4 are a linked pair (setting either sets both),
    /// that holds every number in range as the engine does and can be stopping.
    struct StandIn {
        strips: Mutex<Vec<StripSettings>>,
        stopping: bool,
        /// Every list it took whole.
        replaced: Mutex<Vec<Vec<StripSettings>>>,
    }

    impl Default for StandIn {
        fn default() -> Self {
            Self {
                strips: Mutex::new(vec![StripSettings::default(); MIXER_CHANNELS]),
                stopping: false,
                replaced: Mutex::default(),
            }
        }
    }

    const PAIR: [usize; 2] = [2, 3];

    impl ProcessingEngine for StandIn {
        fn set_channel_processing(
            &self,
            channel: usize,
            settings: StripSettings,
        ) -> Result<StripSettings, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let kept = settings.clamped();
            let mut strips = self.strips.lock().expect("strips");
            if PAIR.contains(&channel) {
                for side in PAIR {
                    strips[side] = kept;
                }
            } else {
                strips[channel] = kept;
            }
            Ok(kept)
        }

        fn processing(&self) -> Vec<StripSettings> {
            self.strips.lock().expect("strips").clone()
        }

        fn replace_processing(
            &self,
            settings: &[StripSettings],
        ) -> Result<Vec<StripSettings>, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.replaced
                .lock()
                .expect("replaced")
                .push(settings.to_vec());
            *self.strips.lock().expect("strips") = settings.to_vec();
            Ok(settings.to_vec())
        }
    }

    fn test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-desktop-tests")
            .join(format!("processing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn::default()))
    }

    /// A saver with no thread of its own that writes into `session` when flushed.
    fn saver_into(session: &Arc<SessionStore>) -> ProcessingSaver {
        let session = Arc::clone(session);
        ProcessingSaver::manual(move |kept| session.set_processing(kept.clone()), |_| {})
    }

    fn strips_of(engine: &Mutex<Option<StandIn>>) -> Vec<StripSettings> {
        engine
            .lock()
            .expect("engine")
            .as_ref()
            .expect("running")
            .processing()
    }

    /// A strip with a compressor on, its threshold `threshold_db`.
    fn compressed(threshold_db: f32) -> StripSettings {
        StripSettings {
            compressor: CompressorSettings {
                enabled: true,
                threshold_db,
                ..CompressorSettings::default()
            },
            ..StripSettings::default()
        }
    }

    /// A strip whose first peak band boosts by `gain_db`.
    fn boosted(gain_db: f32) -> StripSettings {
        let mut settings = StripSettings::default();
        settings.eq.bands[1].gain_db = gain_db;
        settings
    }

    #[test]
    fn a_change_answers_the_settings_held_in_range_and_is_kept_off_the_engine_lock() {
        let session = Arc::new(SessionStore::open(&test_directory("set")));
        let saver = saver_into(&session);
        let engine = running();

        let set = set_with(&engine, &saver, 1, boosted(40.0)).expect("set");

        assert_eq!(set.outcome, "set");
        assert_eq!(set.channels.len(), 1);
        assert_eq!(set.channels[0].channel, 1);
        assert_eq!(set.channels[0].settings, boosted(18.0), "held at +18 dB");
        assert!(set.channels[0].active);
        assert_eq!(set.message, "CH 1 is set to the new processing.");
        assert_eq!(strips_of(&engine)[0], boosted(18.0));
        assert_eq!(
            session.processing(),
            SessionProcessing::new(),
            "only marked under the engine lock"
        );

        saver.flush();
        assert_eq!(
            session.processing(),
            SessionProcessing::from([(1, boosted(18.0))]),
            "written by the saver, by channel number"
        );
        let json = serde_json::to_value(&set).expect("serializes");
        assert_eq!(
            json["channels"][0]["settings"]["eq"]["bands"][1]["gainDb"],
            18.0
        );
        assert_eq!(json["channels"][0]["active"], true);
    }

    #[test]
    fn a_linked_pair_is_one_strip_and_both_sides_are_named() {
        let session = Arc::new(SessionStore::open(&test_directory("pair")));
        let saver = saver_into(&session);
        let engine = running();

        let set = set_with(&engine, &saver, 4, compressed(-30.0)).expect("set");

        let channels: Vec<usize> = set.channels.iter().map(|channel| channel.channel).collect();
        assert_eq!(
            channels,
            vec![3, 4],
            "the other side of the pair changed too"
        );
        assert_eq!(set.message, "CH 3 and CH 4 are set to the new processing.");
        saver.flush();
        assert_eq!(
            session.processing(),
            SessionProcessing::from([(3, compressed(-30.0)), (4, compressed(-30.0))])
        );

        let reset = reset_with(&engine, &saver, 3).expect("reset");
        assert_eq!(reset.outcome, "set");
        assert_eq!(reset.channels.len(), 2);
        assert!(reset.channels.iter().all(|channel| !channel.active));
        assert_eq!(reset.message, "CH 3 and CH 4 are back to no processing.");
        saver.flush();
        assert_eq!(
            session.processing(),
            SessionProcessing::new(),
            "a neutral strip is not kept"
        );
    }

    #[test]
    fn a_channel_outside_the_mixer_is_invalid_and_no_engine_is_an_error() {
        let session = Arc::new(SessionStore::open(&test_directory("invalid")));
        let saver = saver_into(&session);
        let engine = running();

        for channel in [0, 33] {
            let set = set_with(&engine, &saver, channel, compressed(-20.0)).expect("answered");
            assert_eq!(set.outcome, "invalid", "{channel}");
            assert!(set.channels.is_empty());
            assert!(set.message.contains("CH 1-32"), "{}", set.message);
            assert_eq!(
                reset_with(&engine, &saver, channel)
                    .expect("answered")
                    .outcome,
                "invalid"
            );
        }
        assert_eq!(strips_of(&engine), vec![StripSettings::default(); 32]);

        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            set_with(&stopped, &saver, 1, compressed(-20.0)).expect_err("no engine"),
            "the engine is not running"
        );
        let stopping = Mutex::new(Some(StandIn {
            stopping: true,
            ..StandIn::default()
        }));
        assert!(set_with(&stopping, &saver, 1, compressed(-20.0))
            .expect_err("stopping")
            .contains("stopping"));
        saver.flush();
        assert_eq!(
            session.processing(),
            SessionProcessing::new(),
            "nothing kept"
        );
    }

    #[test]
    fn a_copy_lands_on_every_channel_named_and_on_a_linked_pair_whole() {
        let session = Arc::new(SessionStore::open(&test_directory("copy")));
        let saver = saver_into(&session);
        let engine = running();
        set_with(&engine, &saver, 1, compressed(-25.0)).expect("set");

        let copied = copy_with(&engine, &saver, 1, &[3, 1, 8, 8]).expect("copied");

        assert_eq!(copied.outcome, "set");
        let channels: Vec<usize> = copied
            .channels
            .iter()
            .map(|channel| channel.channel)
            .collect();
        assert_eq!(
            channels,
            vec![3, 4, 8],
            "the pair whole, CH 1 itself left out, once each"
        );
        assert!(copied
            .channels
            .iter()
            .all(|channel| channel.settings == compressed(-25.0)));
        assert_eq!(
            copied.message,
            "CH 1's processing is on CH 3, CH 4 and CH 8 now."
        );
        saver.flush();
        assert_eq!(
            session.processing().keys().copied().collect::<Vec<_>>(),
            vec![1, 3, 4, 8]
        );

        let nowhere = copy_with(&engine, &saver, 1, &[1]).expect("answered");
        assert_eq!(nowhere.outcome, "invalid");
        assert!(
            nowhere.message.contains("no channel to copy"),
            "{}",
            nowhere.message
        );
        let outside = copy_with(&engine, &saver, 1, &[2, 40]).expect("answered");
        assert_eq!(outside.outcome, "invalid");
        assert_eq!(
            strips_of(&engine)[1],
            StripSettings::default(),
            "nothing changed, CH 2 included"
        );
    }

    #[test]
    fn with_no_engine_the_reads_answer_the_session_and_a_change_still_waiting() {
        let directory = test_directory("read-idle");
        let session = Arc::new(SessionStore::open(&directory));
        session
            .set_processing(SessionProcessing::from([(2, compressed(-18.0))]))
            .expect("kept");
        let saver = saver_into(&session);
        let idle: Mutex<Option<StandIn>> = Mutex::new(None);

        let all = all_with(&idle, &session, &saver).expect("read");
        assert_eq!(all.len(), 32);
        assert_eq!(
            all[0],
            ChannelProcessingDto::of(1, StripSettings::default())
        );
        assert_eq!(all[1], ChannelProcessingDto::of(2, compressed(-18.0)));
        assert!(all[1].active && !all[0].active);

        saver.mark(SessionProcessing::from([(5, boosted(3.0))]));
        let all = all_with(&idle, &session, &saver).expect("read");
        assert_eq!(all[4].settings, boosted(3.0), "the change waiting wins");
        assert_eq!(all[1].settings, StripSettings::default());

        let engine = running();
        set_with(&engine, &saver, 6, boosted(2.0)).expect("set");
        assert_eq!(
            all_with(&engine, &session, &saver).expect("read")[5].settings,
            boosted(2.0)
        );
    }

    #[test]
    fn a_start_hands_the_engine_the_kept_processing_with_a_change_still_waiting() {
        let session = Arc::new(SessionStore::open(&test_directory("start")));
        session
            .set_processing(SessionProcessing::from([
                (1, compressed(-20.0)),
                (40, boosted(1.0)),
            ]))
            .expect("kept");
        let saver = saver_into(&session);
        let engine = StandIn::default();

        let kept = kept_processing(&session, &saver);
        assert_eq!(hand_kept_processing(&engine, &kept), None);
        let took = engine.replaced.lock().expect("replaced").clone();
        assert_eq!(took.len(), 1);
        assert_eq!(took[0].len(), 32, "every channel, CH 1 first");
        assert_eq!(took[0][0], compressed(-20.0));
        assert!(
            took[0][1..]
                .iter()
                .all(|strip| *strip == StripSettings::default()),
            "the rest neutral; CH 40, which the mixer lacks, skipped"
        );

        saver.mark(SessionProcessing::from([(2, boosted(4.0))]));
        let fresh = StandIn::default();
        hand_kept_processing(&fresh, &kept_processing(&session, &saver));
        assert_eq!(
            fresh.processing()[1],
            boosted(4.0),
            "the change waiting, not the session's"
        );
        assert_eq!(fresh.processing()[0], StripSettings::default());

        let neutral = StandIn::default();
        assert_eq!(
            hand_kept_processing(&neutral, &SessionProcessing::new()),
            None
        );
        assert!(
            neutral.replaced.lock().expect("replaced").is_empty(),
            "nothing kept, nothing handed: a new engine starts neutral"
        );
        let stopping = StandIn {
            stopping: true,
            ..StandIn::default()
        };
        assert!(
            hand_kept_processing(&stopping, &SessionProcessing::from([(1, boosted(1.0))]))
                .expect("refused")
                .contains("not applied")
        );
    }

    #[test]
    fn the_engine_list_and_the_session_section_turn_into_each_other() {
        let mut list = vec![StripSettings::default(); 32];
        list[0] = compressed(-10.0);
        list[31] = boosted(-6.0);
        let kept = session_processing(&list);
        assert_eq!(
            kept,
            SessionProcessing::from([(1, compressed(-10.0)), (32, boosted(-6.0))]),
            "by channel number, the neutral strips left out"
        );
        assert_eq!(processing_list(&kept), list);
    }
}
