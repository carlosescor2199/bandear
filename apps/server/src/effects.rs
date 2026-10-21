//! The shared effects: one reverb and one delay ([crate::dsp::reverb], [crate::dsp::delay]) on two
//! buses every input can send to, returned to each mix at its own level, as a console's aux sends
//! and effect returns.
//!
//! ```text
//!  processed block ─┬─▶ Σ channel × reverb send ─▶ reverb ─▶ wet ─┐
//!                   └─▶ Σ channel × delay send  ─▶ delay  ─▶ wet ─┤
//!  each mix: Σ channel × gain ── + return × master × wet ─▶ limiter ─▶ clamp
//! ```
//!
//! **Sends.** Each input has a send level to each effect, 0-100% ([EffectLevels], kept with the
//! channel's [crate::StripSettings]), set only at the desk. They are taken after the channel strip
//! and before any mix's fader: the buses are shared, so one musician's fader cannot change what
//! another hears of the effect. A linked pair's two sides share their sends; each side is summed
//! into the effect's mono bus.
//!
//! **Returns.** Each mix (every musician's and the local mix) adds each effect's wet signal at its
//! own return level, 0-100%, after its channel sum and before its master limiter and the clamp at
//! its ceiling. The mix's master volume scales the returns too and its master mute silences them.
//! Engineer-only: no phone sets a return, and the phone's `mix` does not carry it.
//!
//! **Exact bypass and cost.** While no input sends to an effect and the effect holds nothing but
//! zeros (its tail has died), the effect does not run at all and no mix adds anything: the mixes
//! are bit for bit what they were before effects existed. A return at 0 adds nothing either. Each
//! effect runs once per block, whatever the number of mixes.
//!
//! **Latency.** The effects are wet returns beside the dry path: they add nothing to the dry
//! signal's latency (the reverb's pre-delay and the delay's time are the effects themselves).
//!
//! **Real time.** The effects' lines are allocated at their full size when the stage is made, off
//! the engine thread. Sends, reverb and delay designs travel through the processing stage's command
//! ring as plain numbers; a send that changes glides across one block, a return across one block of
//! each mix, and the effects glide or crossfade their own settings, so nothing clicks.

use serde::{Deserialize, Serialize};

use crate::dsp::delay::{Delay, DelayDesign, DelaySettings};
use crate::dsp::reverb::{Reverb, ReverbDesign, ReverbSettings};
use crate::mix::{MAX_MIX_CHANNELS, OUTPUT_CHANNELS};

/// One of the two shared effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    Reverb,
    Delay,
}

impl Effect {
    /// Both effects, in the order the engine keeps them.
    pub const ALL: [Effect; 2] = [Effect::Reverb, Effect::Delay];

    /// Its name in words.
    pub fn name(self) -> &'static str {
        match self {
            Effect::Reverb => "reverb",
            Effect::Delay => "delay",
        }
    }
}

/// A level to each effect, 0-100%: an input's sends, or a mix's returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EffectLevels {
    pub reverb: u8,
    pub delay: u8,
}

impl EffectLevels {
    /// The level to `effect`.
    pub fn get(&self, effect: Effect) -> u8 {
        match effect {
            Effect::Reverb => self.reverb,
            Effect::Delay => self.delay,
        }
    }

    /// These levels with `effect`'s set to `level`, held to 100%.
    pub fn with(self, effect: Effect, level: u8) -> Self {
        let level = level.min(100);
        match effect {
            Effect::Reverb => Self {
                reverb: level,
                ..self
            },
            Effect::Delay => Self {
                delay: level,
                ..self
            },
        }
    }

    /// These levels held to 100%.
    pub fn clamped(&self) -> Self {
        Self {
            reverb: self.reverb.min(100),
            delay: self.delay.min(100),
        }
    }

    /// True when nothing goes to either effect.
    pub fn is_off(&self) -> bool {
        self.reverb == 0 && self.delay == 0
    }

    /// The linear gains, by effect.
    fn gains(&self) -> [f32; 2] {
        Effect::ALL.map(|effect| f32::from(self.get(effect).min(100)) / 100.0)
    }
}

/// Both effects' settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EffectSettings {
    pub reverb: ReverbSettings,
    pub delay: DelaySettings,
}

impl EffectSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        Self {
            reverb: self.reverb.clamped(),
            delay: self.delay.clamped(),
        }
    }
}

/// What the effects gave this block, stereo interleaved, one slice per effect that ran; `None`
/// for one that did not run (no send and nothing left of its tail: silence).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Wet<'a> {
    reverb: Option<&'a [f32]>,
    delay: Option<&'a [f32]>,
}

impl<'a> Wet<'a> {
    /// No effect ran.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const NONE: Wet<'static> = Wet {
        reverb: None,
        delay: None,
    };

    /// What `effect` gave, if it ran.
    pub(crate) fn get(&self, effect: Effect) -> Option<&'a [f32]> {
        match effect {
            Effect::Reverb => self.reverb,
            Effect::Delay => self.delay,
        }
    }

    /// `reverb` and `delay` as the effects' output, for a test's block.
    #[cfg(test)]
    pub(crate) fn of(reverb: Option<&'a [f32]>, delay: Option<&'a [f32]>) -> Self {
        Self { reverb, delay }
    }
}

/// What the engine asks of the effects on the engine thread.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EffectsCommand {
    /// Input `channel` (0 is CH 1) sends `levels` from the next block (it glides there).
    Sends {
        channel: usize,
        levels: EffectLevels,
    },
    Reverb(ReverbDesign),
    Delay(DelayDesign),
}

/// The effects on the engine thread: both effects, the inputs' sends, the buses and the wet
/// buffers. Part of the processing stage ([crate::processing::ProcessingStage]).
pub(crate) struct EffectsStage {
    reverb: Reverb,
    delay: Delay,
    /// Each input's send gain to each effect, by effect then channel, and the gains the last block
    /// ended on (a send that changed glides from those).
    sends: [[f32; MAX_MIX_CHANNELS]; 2],
    previous: [[f32; MAX_MIX_CHANNELS]; 2],
    /// Each effect's mono bus, and its stereo wet output, sized for the source's blocks.
    buses: [Vec<f32>; 2],
    wet: [Vec<f32>; 2],
}

impl EffectsStage {
    /// Both effects with `settings`, silent, no input sending, every line allocated. Allocates:
    /// call it off the engine thread.
    pub(crate) fn new(settings: &EffectSettings) -> Self {
        Self {
            reverb: Reverb::new(&settings.reverb.design()),
            delay: Delay::new(&settings.delay.design()),
            sends: [[0.0; MAX_MIX_CHANNELS]; 2],
            previous: [[0.0; MAX_MIX_CHANNELS]; 2],
            buses: [Vec::new(), Vec::new()],
            wet: [Vec::new(), Vec::new()],
        }
    }

    /// The memory the effects' lines take, in floats.
    #[cfg(test)]
    pub(crate) fn floats(&self) -> usize {
        self.reverb.floats() + self.delay.floats()
    }

    /// Sizes the buses for blocks of `block_frames`, off the per-block path.
    pub(crate) fn source_changed(&mut self, block_frames: usize) {
        for bus in &mut self.buses {
            bus.clear();
            bus.resize(block_frames, 0.0);
        }
        for wet in &mut self.wet {
            wet.clear();
            wet.resize(block_frames * usize::from(OUTPUT_CHANNELS), 0.0);
        }
    }

    /// Takes a command from the engine.
    pub(crate) fn take(&mut self, command: EffectsCommand) {
        match command {
            EffectsCommand::Sends { channel, levels } => {
                if channel < MAX_MIX_CHANNELS {
                    let gains = levels.gains();
                    for (effect, gain) in gains.iter().enumerate() {
                        self.sends[effect][channel] = *gain;
                    }
                }
            }
            EffectsCommand::Reverb(design) => self.reverb.apply(&design),
            EffectsCommand::Delay(design) => self.delay.apply(&design),
        }
    }

    /// True while `effect` neither gets anything nor holds anything: it does not run.
    fn resting(&self, effect: usize, channels: usize) -> bool {
        let reach = channels.min(MAX_MIX_CHANNELS);
        let sending = (0..reach).any(|channel| {
            self.sends[effect][channel] > 0.0 || self.previous[effect][channel] > 0.0
        });
        let idle = if effect == 0 {
            self.reverb.is_idle()
        } else {
            self.delay.is_idle()
        };
        !sending && idle
    }

    /// Runs both effects on the inputs' sends from `samples` (`channels`-interleaved, after the
    /// channel strips) and answers what each gave. An effect at rest does not run and gives
    /// `None`. Allocation-free.
    pub(crate) fn process(&mut self, samples: &[f32], channels: usize) -> Wet<'_> {
        let frames = if channels == 0 {
            0
        } else {
            samples.len() / channels
        };
        let mut ran = [false; 2];
        for (effect, done) in ran.iter_mut().enumerate() {
            if frames == 0 || frames > self.buses[effect].len() || self.resting(effect, channels) {
                self.previous[effect] = self.sends[effect];
                continue;
            }
            let bus = &mut self.buses[effect][..frames];
            bus.fill(0.0);
            for channel in 0..channels.min(MAX_MIX_CHANNELS) {
                let (from, to) = (self.previous[effect][channel], self.sends[effect][channel]);
                if from == 0.0 && to == 0.0 {
                    continue;
                }
                let step = (to - from) / frames as f32;
                for (index, (sum, frame)) in bus
                    .iter_mut()
                    .zip(samples.chunks_exact(channels))
                    .enumerate()
                {
                    // The last frame of the block is on the new level, as the mixes ramp.
                    let gain = if from == to {
                        to
                    } else {
                        from + step * (index + 1) as f32
                    };
                    *sum += frame[channel] * gain;
                }
            }
            self.previous[effect] = self.sends[effect];
            let wet = &mut self.wet[effect][..frames * usize::from(OUTPUT_CHANNELS)];
            if effect == 0 {
                self.reverb.process(bus, wet);
            } else {
                self.delay.process(bus, wet);
            }
            *done = true;
        }
        let stereo = frames * usize::from(OUTPUT_CHANNELS);
        Wet {
            reverb: ran[0].then(|| &self.wet[0][..stereo]),
            delay: ran[1].then(|| &self.wet[1][..stereo]),
        }
    }
}

/// Adds `wet` into `stereo` at a gain that glides from `from` to `to` across the block (frame `i`
/// of `n` at `(i + 1) / n` of the way), or holds `to` when they are the same.
pub(crate) fn add_return(stereo: &mut [f32], wet: &[f32], from: f32, to: f32) {
    let frames = stereo.len().min(wet.len()) / usize::from(OUTPUT_CHANNELS);
    if frames == 0 {
        return;
    }
    let step = (to - from) / frames as f32;
    for (index, (out, wet)) in stereo
        .chunks_exact_mut(usize::from(OUTPUT_CHANNELS))
        .zip(wet.chunks_exact(usize::from(OUTPUT_CHANNELS)))
        .take(frames)
        .enumerate()
    {
        let gain = if from == to {
            to
        } else {
            from + step * (index + 1) as f32
        };
        out[0] += wet[0] * gain;
        out[1] += wet[1] * gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::sine;

    /// `frames` frames of `channels` inputs, CH 1 a tone and the rest silent.
    fn block(channels: usize, frames: usize) -> Vec<f32> {
        let tone = sine(440.0, 0.5, frames);
        let mut block = vec![0.0; channels * frames];
        for (frame, sample) in block.chunks_exact_mut(channels).zip(tone) {
            frame[0] = sample;
        }
        block
    }

    fn stage(frames: usize) -> EffectsStage {
        let mut stage = EffectsStage::new(&EffectSettings::default());
        stage.source_changed(frames);
        stage
    }

    #[test]
    fn with_nothing_sent_the_effects_do_not_run() {
        let mut stage = stage(256);
        let input = block(4, 256);
        for _ in 0..20 {
            let wet = stage.process(&input, 4);
            assert!(wet.get(Effect::Reverb).is_none() && wet.get(Effect::Delay).is_none());
        }
    }

    #[test]
    fn a_send_feeds_its_effect_only_and_the_tail_runs_on_after_it_closes() {
        let mut stage = stage(256);
        stage.take(EffectsCommand::Sends {
            channel: 0,
            levels: EffectLevels {
                reverb: 50,
                delay: 0,
            },
        });
        let input = block(4, 256);
        let mut heard = 0.0f32;
        for _ in 0..20 {
            let wet = stage.process(&input, 4);
            assert!(
                wet.get(Effect::Delay).is_none(),
                "nothing sent to the delay"
            );
            let reverb = wet.get(Effect::Reverb).expect("the reverb runs");
            assert_eq!(reverb.len(), 512);
            heard = heard.max(reverb.iter().fold(0.0, |peak, s| peak.max(s.abs())));
        }
        assert!(heard > 0.01, "the reverb is heard: {heard}");
        // Closed: the tail goes on, then the reverb rests.
        stage.take(EffectsCommand::Sends {
            channel: 0,
            levels: EffectLevels::default(),
        });
        assert!(
            stage.process(&input, 4).get(Effect::Reverb).is_some(),
            "the tail"
        );
        let mut rested = false;
        for _ in 0..(48_000 * 30 / 256) {
            if stage.process(&input, 4).get(Effect::Reverb).is_none() {
                rested = true;
                break;
            }
        }
        assert!(rested, "the reverb rests once its tail died");
    }

    #[test]
    fn a_send_that_changes_glides_into_the_bus() {
        let mut stage = stage(256);
        let input = vec![1.0f32; 2 * 256];
        stage.take(EffectsCommand::Sends {
            channel: 1,
            levels: EffectLevels {
                reverb: 0,
                delay: 100,
            },
        });
        stage.process(&input, 2);
        let bus = &stage.buses[1];
        assert!((bus[0] - 1.0 / 256.0).abs() < 1e-6, "{}", bus[0]);
        assert!((bus[127] - 0.5).abs() < 1e-6, "{}", bus[127]);
        assert_eq!(bus[255], 1.0, "the block ends on the new level");
        stage.process(&input, 2);
        assert!(
            stage.buses[1].iter().all(|sample| *sample == 1.0),
            "then holds it"
        );
    }

    #[test]
    fn a_return_glides_from_its_last_level_or_holds_it() {
        let wet = vec![1.0f32; 8];
        let mut stereo = vec![0.0f32; 8];
        add_return(&mut stereo, &wet, 0.0, 1.0);
        assert_eq!(stereo, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0]);
        add_return(&mut stereo, &wet, 0.5, 0.5);
        assert_eq!(stereo, [0.75, 0.75, 1.0, 1.0, 1.25, 1.25, 1.5, 1.5]);
    }

    #[test]
    fn levels_hold_to_a_hundred_and_settings_round_trip_through_json() {
        let levels = EffectLevels::default().with(Effect::Delay, 250);
        assert_eq!(
            levels,
            EffectLevels {
                reverb: 0,
                delay: 100
            }
        );
        assert!(!levels.is_off());
        assert_eq!(levels.get(Effect::Reverb), 0);
        let json = serde_json::to_string(&EffectSettings::default()).unwrap();
        assert!(
            json.contains("\"reverb\"") && json.contains("\"timeMs\""),
            "{json}"
        );
        let partial: EffectSettings =
            serde_json::from_str(r#"{"delay":{"pingPong":true}}"#).unwrap();
        assert!(partial.delay.ping_pong);
        assert_eq!(partial.reverb, ReverbSettings::default());
        assert_eq!(
            serde_json::to_string(&Effect::Reverb).unwrap(),
            "\"reverb\""
        );
    }

    #[test]
    fn running_the_effects_never_allocates_and_their_lines_are_sized_once() {
        let mut stage = stage(256);
        // The reverb's lines and the delay's two seconds a side: about 230 thousand floats.
        assert!(
            (220_000..240_000).contains(&stage.floats()),
            "{}",
            stage.floats()
        );
        let input = block(32, 256);
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for round in 0..40 {
                stage.take(EffectsCommand::Sends {
                    channel: round % 32,
                    levels: EffectLevels {
                        reverb: 40,
                        delay: 30,
                    },
                });
                if round == 20 {
                    stage.take(EffectsCommand::Delay(
                        DelaySettings {
                            time_ms: 800.0,
                            ..DelaySettings::default()
                        }
                        .design(),
                    ));
                    stage.take(EffectsCommand::Reverb(ReverbSettings::default().design()));
                }
                let wet = stage.process(&input, 32);
                let _ = (wet.get(Effect::Reverb), wet.get(Effect::Delay));
            }
        });
        assert_eq!((allocations, frees), (0, 0));
    }
}
