//! Each input's channel processing set from the desk: the commands that read and change a channel's
//! strip (the high-pass filter, the four-band equaliser, the gate and the compressor; see
//! [personal_monitoring::StripSettings]), the strip presets, and the equaliser's response curve the
//! editor draws.
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
//! - Strip presets: a few built-in ones ([factory_presets]: "Vocal", "Kick", "Bass DI", "Acoustic
//!   guitar" and "Gentle compression"), which are never written and cannot be deleted or replaced,
//!   and the engineer's own, kept by name in the session in the order they were saved
//!   (`stripPresets`). A name is 1-40 characters once trimmed and matched in any case. Saving
//!   under a built-in preset's name is refused (`invalid`), so a built-in name always means the
//!   built-in strip; saving under the name of one of the engineer's own replaces it in place
//!   (`replaced`). Presets need no engine; applying one sets a channel as
//!   [set_channel_processing] does.
//! - [eq_response] works out the equaliser's gain on a logarithmic grid from 20 Hz to 20 kHz for
//!   any settings, the high-pass filter included, on the command's thread: never the engine's.

use std::sync::Mutex;

use personal_monitoring::{
    log_info, log_warn, CompressorSettings, EngineHandle, EqBandSettings, EqSettings, GateSettings,
    HighPassSettings, StripSettings,
};
use serde::Serialize;

use super::local::saver::{Kept, Saver};
use super::DesktopState;
use crate::session::{SessionProcessing, SessionStore, SessionStripPreset};

/// The mixer's channels, each with its strip.
pub const MIXER_CHANNELS: usize = 32;
/// The longest name of a strip preset, in characters.
pub const MAX_STRIP_PRESET_NAME_LENGTH: usize = 40;
/// The points of an equaliser response when the window asks for no number.
pub const EQ_RESPONSE_POINTS: usize = 128;
/// The fewest and the most points an equaliser response has.
pub const MIN_EQ_RESPONSE_POINTS: usize = 2;
pub const MAX_EQ_RESPONSE_POINTS: usize = 1024;
/// The lowest and the highest frequency of an equaliser response, in Hz.
pub const EQ_RESPONSE_LOW_HZ: f64 = 20.0;
pub const EQ_RESPONSE_HIGH_HZ: f64 = 20_000.0;

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
    /// `set`; `invalid` (a channel outside 1-32, or nothing to copy to) or `notFound` (no strip
    /// preset by that name), and then nothing changed.
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

/// One strip preset as the desk lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripPresetDto {
    pub name: String,
    pub settings: StripSettings,
    /// Built in: it cannot be deleted, and no preset of the engineer's can take its name.
    pub factory: bool,
}

/// What a change of the strip presets did.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripPresetsSetDto {
    /// `saved`, `replaced` (one of the engineer's own with that name was replaced) or `deleted`;
    /// `invalid` (a name that is empty, too long or a built-in preset's), `factory` (a built-in
    /// preset cannot be deleted) or `notFound`, and then nothing changed.
    pub outcome: &'static str,
    /// The preset saved; null when none was.
    pub preset: Option<StripPresetDto>,
    /// Every preset now, the built-in ones first.
    pub presets: Vec<StripPresetDto>,
    /// The outcome in words.
    pub message: String,
}

/// One point of the equaliser's response.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPointDto {
    pub freq_hz: f64,
    /// The gain there, in dB.
    pub db: f64,
}

/// The built-in strip presets, in the order they are listed. Starting points, not recipes: each
/// value is held well inside its range so any of them can be pushed either way by ear.
pub fn factory_presets() -> Vec<(&'static str, StripSettings)> {
    let high_pass = |frequency_hz| HighPassSettings {
        enabled: true,
        frequency_hz,
        q: std::f32::consts::FRAC_1_SQRT_2,
    };
    let band = |frequency_hz, gain_db, q| EqBandSettings {
        enabled: true,
        frequency_hz,
        gain_db,
        q,
    };
    let compressor =
        |threshold_db, ratio, knee_db, attack_ms, release_ms, makeup_db| CompressorSettings {
            enabled: true,
            threshold_db,
            ratio,
            knee_db,
            attack_ms,
            release_ms,
            makeup_db,
        };
    vec![
        (
            "Vocal",
            StripSettings {
                eq: EqSettings {
                    high_pass: high_pass(100.0),
                    bands: [
                        band(150.0, -2.0, 0.7),
                        band(350.0, -2.0, 1.2),
                        band(3_000.0, 2.5, 1.0),
                        band(10_000.0, 2.0, 0.7),
                    ],
                },
                gate: GateSettings::default(),
                compressor: compressor(-20.0, 3.0, 6.0, 8.0, 120.0, 3.0),
            },
        ),
        (
            "Kick",
            StripSettings {
                eq: EqSettings {
                    high_pass: high_pass(35.0),
                    bands: [
                        band(60.0, 3.0, 0.7),
                        band(380.0, -5.0, 1.5),
                        band(3_500.0, 3.0, 1.4),
                        band(9_000.0, 0.0, 0.7),
                    ],
                },
                gate: GateSettings {
                    enabled: true,
                    threshold_db: -40.0,
                    range_db: 30.0,
                    attack_ms: 0.3,
                    hold_ms: 40.0,
                    release_ms: 120.0,
                },
                compressor: compressor(-14.0, 4.0, 3.0, 15.0, 90.0, 2.0),
            },
        ),
        (
            "Bass DI",
            StripSettings {
                eq: EqSettings {
                    high_pass: high_pass(30.0),
                    bands: [
                        band(80.0, 2.0, 0.7),
                        band(250.0, -3.0, 1.0),
                        band(900.0, 2.0, 1.2),
                        band(6_000.0, -3.0, 0.7),
                    ],
                },
                gate: GateSettings::default(),
                compressor: compressor(-22.0, 4.0, 6.0, 20.0, 200.0, 3.0),
            },
        ),
        (
            "Acoustic guitar",
            StripSettings {
                eq: EqSettings {
                    high_pass: high_pass(90.0),
                    bands: [
                        band(180.0, -3.0, 0.7),
                        band(800.0, -2.0, 1.5),
                        band(4_500.0, 2.0, 1.0),
                        band(11_000.0, 2.0, 0.7),
                    ],
                },
                gate: GateSettings::default(),
                compressor: compressor(-18.0, 2.5, 8.0, 12.0, 150.0, 2.0),
            },
        ),
        (
            "Gentle compression",
            StripSettings {
                compressor: compressor(-24.0, 2.0, 10.0, 20.0, 250.0, 2.0),
                ..StripSettings::default()
            },
        ),
    ]
}

/// Every channel's strip, CH 1 first, for the engine to take
/// ([ProcessingEngine::replace_processing]): `kept`'s by channel number and the rest neutral. A number outside 1-32 (a hand-edited session)
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

/// Every strip preset: the built-in ones, then the engineer's own in the order they were saved.
pub fn strip_presets(state: &DesktopState) -> Vec<StripPresetDto> {
    presets_of(&state.session)
}

/// Saves `settings` (every number held in range) as the engineer's strip preset `name` and answers
/// what was done ([StripPresetsSetDto]). Needs no engine. A save that fails is an error, and
/// nothing changed.
pub fn save_strip_preset(
    state: &DesktopState,
    name: String,
    settings: StripSettings,
) -> Result<StripPresetsSetDto, String> {
    let outcome = save_preset_with(&state.session, &name, settings)?;
    log_info!(
        "desk",
        "save_strip_preset: \"{}\" -> {}",
        name.trim(),
        outcome.outcome
    );
    Ok(outcome)
}

/// Deletes the engineer's strip preset `name` (in any case) and answers what was done; a built-in
/// one is never deleted.
pub fn delete_strip_preset(
    state: &DesktopState,
    name: String,
) -> Result<StripPresetsSetDto, String> {
    let outcome = delete_preset_with(&state.session, &name)?;
    log_info!(
        "desk",
        "delete_strip_preset: \"{}\" -> {}",
        name.trim(),
        outcome.outcome
    );
    Ok(outcome)
}

/// Puts the strip preset `name` (in any case) on `channel` (1-based), as [set_channel_processing]
/// sets it.
pub fn apply_strip_preset(
    state: &DesktopState,
    name: String,
    channel: usize,
) -> Result<ProcessingSetDto, String> {
    let outcome = apply_preset_with(
        &state.engine,
        &state.session,
        &state.processing_saver,
        &name,
        channel,
    )?;
    log_info!(
        "desk",
        "apply_strip_preset: \"{}\" on CH {channel} -> {}",
        name.trim(),
        outcome.outcome
    );
    Ok(outcome)
}

/// The response of the equaliser `settings` describes, the high-pass filter included: `points`
/// gains (128 when none is asked for, held to 2-1,024) at frequencies spaced evenly on a
/// logarithmic scale from 20 Hz to 20 kHz, both ends included. Worked out here, never on the
/// engine's thread.
pub fn eq_response(settings: &EqSettings, points: Option<usize>) -> Vec<EqPointDto> {
    let count = points
        .unwrap_or(EQ_RESPONSE_POINTS)
        .clamp(MIN_EQ_RESPONSE_POINTS, MAX_EQ_RESPONSE_POINTS);
    let span = EQ_RESPONSE_HIGH_HZ / EQ_RESPONSE_LOW_HZ;
    (0..count)
        .map(|index| {
            let freq_hz = EQ_RESPONSE_LOW_HZ * span.powf(index as f64 / (count - 1) as f64);
            EqPointDto {
                freq_hz,
                db: settings.magnitude_db(freq_hz),
            }
        })
        .collect()
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

/// Every strip preset `session` gives: the built-in ones first.
fn presets_of(session: &SessionStore) -> Vec<StripPresetDto> {
    factory_presets()
        .into_iter()
        .map(|(name, settings)| StripPresetDto {
            name: name.to_owned(),
            settings,
            factory: true,
        })
        .chain(
            session
                .strip_presets()
                .into_iter()
                .map(|kept| StripPresetDto {
                    name: kept.name,
                    settings: kept.settings.clamped(),
                    factory: false,
                }),
        )
        .collect()
}

/// Whether `one` and `other` name the same preset: the same name in any case.
fn same_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

/// [save_strip_preset] over any session.
fn save_preset_with(
    session: &SessionStore,
    name: &str,
    settings: StripSettings,
) -> Result<StripPresetsSetDto, String> {
    let name = name.trim();
    let refused = |message: String| StripPresetsSetDto {
        outcome: "invalid",
        preset: None,
        presets: presets_of(session),
        message,
    };
    if name.is_empty() {
        return Ok(refused(
            "A strip preset needs a name; nothing was saved.".to_owned(),
        ));
    }
    if name.chars().count() > MAX_STRIP_PRESET_NAME_LENGTH {
        return Ok(refused(format!(
            "A strip preset's name is at most {MAX_STRIP_PRESET_NAME_LENGTH} characters; nothing \
             was saved."
        )));
    }
    if let Some((factory, _)) = factory_presets()
        .into_iter()
        .find(|(factory, _)| same_name(factory, name))
    {
        return Ok(refused(format!(
            "\"{factory}\" is a built-in preset; save under another name. Nothing was saved."
        )));
    }
    let preset = SessionStripPreset {
        name: name.to_owned(),
        settings: settings.clamped(),
    };
    let mut presets = session.strip_presets();
    let replaced = match presets.iter_mut().find(|kept| same_name(&kept.name, name)) {
        Some(kept) => {
            *kept = preset.clone();
            true
        }
        None => {
            presets.push(preset.clone());
            false
        }
    };
    session.set_strip_presets(presets)?;
    Ok(StripPresetsSetDto {
        outcome: if replaced { "replaced" } else { "saved" },
        preset: Some(StripPresetDto {
            name: preset.name.clone(),
            settings: preset.settings,
            factory: false,
        }),
        presets: presets_of(session),
        message: if replaced {
            format!("The strip preset \"{name}\" was replaced.")
        } else {
            format!("Saved the strip preset \"{name}\".")
        },
    })
}

/// [delete_strip_preset] over any session.
fn delete_preset_with(session: &SessionStore, name: &str) -> Result<StripPresetsSetDto, String> {
    let name = name.trim();
    let answer = |outcome: &'static str, message: String| StripPresetsSetDto {
        outcome,
        preset: None,
        presets: presets_of(session),
        message,
    };
    if let Some((factory, _)) = factory_presets()
        .into_iter()
        .find(|(factory, _)| same_name(factory, name))
    {
        return Ok(answer(
            "factory",
            format!("\"{factory}\" is a built-in preset and cannot be deleted."),
        ));
    }
    let mut presets = session.strip_presets();
    let Some(position) = presets.iter().position(|kept| same_name(&kept.name, name)) else {
        return Ok(answer(
            "notFound",
            format!("There is no strip preset called \"{name}\"; nothing was deleted."),
        ));
    };
    let removed = presets.remove(position);
    session.set_strip_presets(presets)?;
    Ok(answer(
        "deleted",
        format!("Deleted the strip preset \"{}\".", removed.name),
    ))
}

/// [apply_strip_preset] over any engine slot, session and saver. The presets are read before the
/// engine lock is taken, so the session's lock is never held inside it here.
fn apply_preset_with<E: ProcessingEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    saver: &ProcessingSaver,
    name: &str,
    channel: usize,
) -> Result<ProcessingSetDto, String> {
    if let Some(invalid) = invalid_channel(channel) {
        return Ok(invalid);
    }
    let name = name.trim();
    let Some(preset) = presets_of(session)
        .into_iter()
        .find(|preset| same_name(&preset.name, name))
    else {
        return Ok(ProcessingSetDto::refused(
            "notFound",
            format!("There is no strip preset called \"{name}\"; nothing changed."),
        ));
    };
    let channels = apply_to(engine, saver, &[channel], preset.settings)?;
    let message = format!("\"{}\" is on {} now.", preset.name, channel_list(&channels));
    Ok(ProcessingSetDto {
        outcome: "set",
        channels,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn the_built_in_presets_come_first_are_in_range_and_each_does_something() {
        let session = SessionStore::open(&test_directory("factory"));
        let listed = presets_of(&session);
        let names: Vec<&str> = listed.iter().map(|preset| preset.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Vocal",
                "Kick",
                "Bass DI",
                "Acoustic guitar",
                "Gentle compression"
            ]
        );
        for preset in &listed {
            assert!(preset.factory, "{}", preset.name);
            assert_eq!(
                preset.settings.clamped(),
                preset.settings,
                "{} is in range",
                preset.name
            );
            assert!(
                !preset.settings.is_neutral(),
                "{} does something",
                preset.name
            );
        }
    }

    #[test]
    fn the_engineers_presets_are_saved_replaced_by_name_and_deleted_but_never_the_built_in_ones() {
        let directory = test_directory("presets");
        let session = SessionStore::open(&directory);

        let saved = save_preset_with(&session, "  Lead vocal ", boosted(30.0)).expect("saved");
        assert_eq!(saved.outcome, "saved");
        let preset = saved.preset.expect("the preset");
        assert_eq!(preset.name, "Lead vocal", "trimmed");
        assert_eq!(preset.settings, boosted(18.0), "held in range");
        assert!(!preset.factory);
        assert_eq!(saved.presets.len(), 6);
        assert_eq!(
            saved.presets[5].name, "Lead vocal",
            "after the built-in ones"
        );
        save_preset_with(&session, "Snare", compressed(-12.0)).expect("saved");

        let replaced = save_preset_with(&session, "LEAD VOCAL", compressed(-30.0)).expect("saved");
        assert_eq!(replaced.outcome, "replaced");
        let mine: Vec<(String, StripSettings)> = replaced
            .presets
            .iter()
            .filter(|preset| !preset.factory)
            .map(|preset| (preset.name.clone(), preset.settings))
            .collect();
        assert_eq!(
            mine,
            vec![
                ("LEAD VOCAL".to_owned(), compressed(-30.0)),
                ("Snare".to_owned(), compressed(-12.0)),
            ],
            "replaced in place, under the name as written now"
        );
        assert_eq!(
            SessionStore::open(&directory).strip_presets().len(),
            2,
            "kept in the session file"
        );

        for name in ["vocal", "Kick", "  gentle COMPRESSION "] {
            let refused = save_preset_with(&session, name, boosted(1.0)).expect("answered");
            assert_eq!(refused.outcome, "invalid", "{name}");
            assert!(refused.message.contains("built-in"), "{}", refused.message);
            let kept = delete_preset_with(&session, name).expect("answered");
            assert_eq!(kept.outcome, "factory", "{name}");
        }
        assert_eq!(
            save_preset_with(&session, "  ", boosted(1.0))
                .expect("answered")
                .outcome,
            "invalid"
        );
        let long = "x".repeat(MAX_STRIP_PRESET_NAME_LENGTH + 1);
        assert_eq!(
            save_preset_with(&session, &long, boosted(1.0))
                .expect("answered")
                .outcome,
            "invalid"
        );

        let deleted = delete_preset_with(&session, "snare").expect("deleted");
        assert_eq!(deleted.outcome, "deleted");
        assert_eq!(deleted.message, "Deleted the strip preset \"Snare\".");
        assert_eq!(deleted.presets.len(), 6);
        assert_eq!(
            delete_preset_with(&session, "snare")
                .expect("answered")
                .outcome,
            "notFound"
        );
        assert_eq!(
            presets_of(&session).len(),
            6,
            "the built-in ones are all still there"
        );
    }

    #[test]
    fn a_preset_is_applied_to_a_channel_as_a_change_is() {
        let session = Arc::new(SessionStore::open(&test_directory("apply")));
        let saver = saver_into(&session);
        let engine = running();
        save_preset_with(&session, "Snare", compressed(-12.0)).expect("saved");

        let vocal = apply_preset_with(&engine, &session, &saver, "vocal", 2).expect("applied");
        assert_eq!(vocal.outcome, "set");
        assert_eq!(vocal.message, "\"Vocal\" is on CH 2 now.");
        assert_eq!(strips_of(&engine)[1], factory_presets()[0].1);

        let snare = apply_preset_with(&engine, &session, &saver, "Snare", 3).expect("applied");
        assert_eq!(snare.channels.len(), 2, "on the whole pair");
        saver.flush();
        assert_eq!(session.processing().get(&4), Some(&compressed(-12.0)));

        let missing = apply_preset_with(&engine, &session, &saver, "Tuba", 1).expect("answered");
        assert_eq!(missing.outcome, "notFound");
        let outside = apply_preset_with(&engine, &session, &saver, "Vocal", 0).expect("answered");
        assert_eq!(outside.outcome, "invalid");
        assert_eq!(strips_of(&engine)[0], StripSettings::default());
    }

    #[test]
    fn the_equaliser_response_is_a_log_grid_with_the_filters_gains() {
        let flat = eq_response(&EqSettings::default(), None);
        assert_eq!(flat.len(), 128);
        assert_eq!(flat[0].freq_hz, 20.0);
        assert!((flat[127].freq_hz - 20_000.0).abs() < 1e-6);
        let ratio = flat[1].freq_hz / flat[0].freq_hz;
        assert!(flat
            .windows(2)
            .all(|pair| (pair[1].freq_hz / pair[0].freq_hz - ratio).abs() < 1e-9));
        assert!(
            flat.iter().all(|point| point.db == 0.0),
            "a flat equaliser is 0 dB"
        );

        let mut peak = EqSettings::default();
        peak.bands[2] = EqBandSettings {
            enabled: true,
            frequency_hz: 2_000.0,
            gain_db: 6.0,
            q: 1.0,
        };
        let points = eq_response(&peak, Some(4));
        let freqs: Vec<f64> = points.iter().map(|point| point.freq_hz).collect();
        assert!((freqs[2] - 2_000.0).abs() < 1e-6, "{freqs:?}");
        assert!(
            (points[2].db - 6.0).abs() < 0.01,
            "the peak's gain at its centre: {points:?}"
        );
        assert!(points[0].db.abs() < 0.1, "far below it nothing: {points:?}");

        let mut shelf = EqSettings::default();
        shelf.bands[0].gain_db = 6.0;
        let points = eq_response(&shelf, Some(2));
        assert!(
            (points[0].db - 6.0).abs() < 0.5,
            "under the low shelf: {points:?}"
        );
        assert!(points[1].db.abs() < 0.1, "far above it: {points:?}");

        let mut high_pass = EqSettings::default();
        high_pass.high_pass = HighPassSettings {
            enabled: true,
            frequency_hz: 200.0,
            q: std::f32::consts::FRAC_1_SQRT_2,
        };
        let points = eq_response(&high_pass, Some(4));
        assert!(
            (points[1].db + 3.01).abs() < 0.05,
            "-3 dB at the corner: {points:?}"
        );
        assert!(points[0].db < -30.0, "a decade below: {points:?}");

        assert_eq!(eq_response(&peak, Some(1)).len(), 2);
        assert_eq!(eq_response(&peak, Some(50_000)).len(), 1024);
        let json = serde_json::to_value(eq_response(&peak, Some(2))).expect("serializes");
        assert_eq!(json[0]["freqHz"], 20.0);
        assert!(json[0]["db"].is_number());
    }
}
