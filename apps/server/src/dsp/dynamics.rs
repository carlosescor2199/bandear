//! Dynamics: a gate, a compressor and a peak limiter, for one channel or a linked stereo pair. None
//! looks ahead, so none adds latency.
//!
//! **Detectors.** A linked pair has one detector, fed the louder of the two channels each frame
//! (`max(|L|, |R|)`), and one gain for both, so the image does not shift. The compressor and the
//! limiter detect the sample peak; the gate follows a peak envelope that falls by 10 ms
//! ([GATE_DETECTOR_MS]), so a low note does not chatter at its zero crossings. Peak rather than RMS
//! detection: it needs no averaging window, catches transients a monitor mix must not let through,
//! and the attack and release shape the rest.
//!
//! **Compressor.** Feed-forward, its gain computed in decibels from the detected level with a
//! soft knee (Giannoulis, Massberg and Reiss's formulation), then smoothed in decibels by the attack
//! when the reduction grows and the release when it shrinks.
//!
//! **Gate.** Open (no attenuation) while the envelope is at or above the threshold and for the hold
//! time after it falls below, then closing by `range` dB with the release; it opens with the attack.
//!
//! **Limiter.** Its gain drops at once to what keeps the sample at the ceiling (an attack of zero,
//! possible without lookahead) and recovers with the release. A final clip at the ceiling catches
//! the last bit of rounding, so `|out| <= ceiling` always holds.
//!
//! **Not a number.** The limiter takes a sample that is not a finite number (NaN or an infinity)
//! as silence and counts it, so its output always holds under the ceiling; a strip does the same
//! at its input (see [super::strip]).
//!
//! Each reports its gain reduction in dB as a positive number (6.0 is 6 dB quieter), an f32 the
//! engine can publish through an atomic as its bits. Turned off while working, each glides back to
//! no reduction (a makeup gain glides back to 0 dB) before it stops touching the samples.
//!
//! Ranges (a value outside is held at the nearest end; a value that is not a number takes the
//! default):
//!
//! | Setting | Gate | Compressor | Limiter |
//! |---|---|---|---|
//! | Threshold / ceiling | -80 to 0 dBFS (-50) | -60 to 0 dBFS (-18) | -24 to 0 dBFS (-1) |
//! | Range | 0-80 dB (40) | | |
//! | Ratio | | 1-20 (3) | |
//! | Knee | | 0-24 dB (6) | |
//! | Attack | 0.05-50 ms (0.5) | 0.1-200 ms (10) | instant |
//! | Hold | 0-2,000 ms (50) | | |
//! | Release | 5-4,000 ms (150) | 10-2,000 ms (150) | 1-1,000 ms (50) |
//! | Makeup | | 0-24 dB (0) | |

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::{
    channel_frames, clamp_to, db_to_gain, flush_denormal, gain_to_db, time_coefficient, Glide,
    GLIDE_FRAMES, SAMPLE_RATE,
};

pub(crate) const GATE_THRESHOLD_DB: RangeInclusive<f32> = -80.0..=0.0;
pub(crate) const GATE_RANGE_DB: RangeInclusive<f32> = 0.0..=80.0;
pub(crate) const GATE_ATTACK_MS: RangeInclusive<f32> = 0.05..=50.0;
pub(crate) const GATE_HOLD_MS: RangeInclusive<f32> = 0.0..=2_000.0;
pub(crate) const GATE_RELEASE_MS: RangeInclusive<f32> = 5.0..=4_000.0;

pub(crate) const COMPRESSOR_THRESHOLD_DB: RangeInclusive<f32> = -60.0..=0.0;
pub(crate) const COMPRESSOR_RATIO: RangeInclusive<f32> = 1.0..=20.0;
pub(crate) const COMPRESSOR_KNEE_DB: RangeInclusive<f32> = 0.0..=24.0;
pub(crate) const COMPRESSOR_ATTACK_MS: RangeInclusive<f32> = 0.1..=200.0;
pub(crate) const COMPRESSOR_RELEASE_MS: RangeInclusive<f32> = 10.0..=2_000.0;
pub(crate) const COMPRESSOR_MAKEUP_DB: RangeInclusive<f32> = 0.0..=24.0;

pub(crate) const LIMITER_CEILING_DB: RangeInclusive<f32> = -24.0..=0.0;
pub(crate) const LIMITER_RELEASE_MS: RangeInclusive<f32> = 1.0..=1_000.0;

/// How fast the gate's peak envelope falls.
pub(crate) const GATE_DETECTOR_MS: f32 = 10.0;

/// How fast a processor turned off while working lets its reduction go.
const SWITCH_OFF_MS: f32 = 20.0;

/// A reduction this close to its target (in dB) is the target: below a thousandth of a decibel.
const SETTLED_DB: f32 = 1e-3;

/// The louder of the channels of a frame.
#[inline]
fn frame_peak(frame: &[f32]) -> f32 {
    frame
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

/// `gain` applied to every sample of the frame, nothing done for a gain of exactly 1.
#[inline]
fn apply_gain(frame: &mut [f32], gain: f32) {
    if gain != 1.0 {
        for sample in frame {
            *sample *= gain;
        }
    }
}

/// One step of a one-pole smoother from `value` towards `target` with `coefficient`, landing on
/// the target once within [SETTLED_DB].
#[inline]
fn smooth_db(value: f32, target: f32, coefficient: f32) -> f32 {
    let next = target + coefficient * (value - target);
    if (next - target).abs() < SETTLED_DB {
        target
    } else {
        next
    }
}

// ---------------------------------------------------------------------------------------------
// Gate

/// The gate's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GateSettings {
    pub enabled: bool,
    /// The level, in dBFS, at or above which the gate is open.
    pub threshold_db: f32,
    /// How far the closed gate turns the channel down, in dB.
    pub range_db: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
}

impl Default for GateSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -50.0,
            range_db: 40.0,
            attack_ms: 0.5,
            hold_ms: 50.0,
            release_ms: 150.0,
        }
    }
}

impl GateSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            enabled: self.enabled,
            threshold_db: clamp_to(self.threshold_db, &GATE_THRESHOLD_DB, default.threshold_db),
            range_db: clamp_to(self.range_db, &GATE_RANGE_DB, default.range_db),
            attack_ms: clamp_to(self.attack_ms, &GATE_ATTACK_MS, default.attack_ms),
            hold_ms: clamp_to(self.hold_ms, &GATE_HOLD_MS, default.hold_ms),
            release_ms: clamp_to(self.release_ms, &GATE_RELEASE_MS, default.release_ms),
        }
    }

    /// True when the gate changes nothing: it is off, or closes by 0 dB.
    pub fn is_neutral(&self) -> bool {
        let settings = self.clamped();
        !settings.enabled || settings.range_db == 0.0
    }

    /// The gate's numbers for the engine thread.
    pub(crate) fn design(&self) -> GateDesign {
        let settings = self.clamped();
        GateDesign {
            enabled: !settings.is_neutral(),
            threshold: db_to_gain(settings.threshold_db),
            range_db: settings.range_db,
            closed_gain: db_to_gain(-settings.range_db),
            attack: time_coefficient(settings.attack_ms),
            hold_frames: (f64::from(settings.hold_ms) * 1e-3 * SAMPLE_RATE).round() as u32,
            release: time_coefficient(settings.release_ms),
            detector: time_coefficient(GATE_DETECTOR_MS),
        }
    }
}

/// The gate's settings worked out for the engine thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GateDesign {
    enabled: bool,
    threshold: f32,
    range_db: f32,
    closed_gain: f32,
    attack: f32,
    hold_frames: u32,
    release: f32,
    detector: f32,
}

impl Default for GateDesign {
    fn default() -> Self {
        GateSettings::default().design()
    }
}

/// A running gate.
#[derive(Clone, Debug)]
pub(crate) struct Gate {
    design: GateDesign,
    channels: usize,
    envelope: f32,
    hold: u32,
    /// The gain in dB: 0 open, `-range_db` closed.
    gain_db: f32,
    active: bool,
}

impl Gate {
    /// An open gate that does nothing, for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            design: GateDesign::default(),
            channels: channels.clamp(1, super::MAX_CHANNELS),
            envelope: 0.0,
            hold: 0,
            gain_db: 0.0,
            active: false,
        }
    }

    /// Takes `design`; the gain moves from where it is with the attack or the release.
    pub(crate) fn apply(&mut self, design: &GateDesign) {
        self.design = *design;
        if design.enabled && !self.active {
            // Starts open: a gate switched on fades a quiet channel out, never cuts it.
            self.active = true;
            self.envelope = 0.0;
            self.hold = design.hold_frames;
            self.gain_db = 0.0;
        }
    }

    /// False when the samples go through untouched.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// How far the gate turns the channel down now, in dB (0 open).
    pub(crate) fn gain_reduction_db(&self) -> f32 {
        if self.active {
            -self.gain_db
        } else {
            0.0
        }
    }

    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        if !self.active {
            return;
        }
        let design = self.design;
        for frame in channel_frames(block, stride, first, self.channels) {
            let target = if design.enabled {
                let peak = frame_peak(frame);
                self.envelope = if peak > self.envelope {
                    peak
                } else {
                    flush_denormal(self.envelope * design.detector)
                };
                if self.envelope >= design.threshold {
                    self.hold = design.hold_frames;
                    0.0
                } else if self.hold > 0 {
                    self.hold -= 1;
                    0.0
                } else {
                    -design.range_db
                }
            } else {
                0.0
            };
            // Turned off, it opens with its attack, as it would for a signal.
            let coefficient = if !design.enabled || target > self.gain_db {
                design.attack
            } else {
                design.release
            };
            self.gain_db = smooth_db(self.gain_db, target, coefficient);
            let gain = if self.gain_db == 0.0 {
                1.0
            } else if self.gain_db == -design.range_db {
                design.closed_gain
            } else {
                db_to_gain(self.gain_db)
            };
            apply_gain(frame, gain);
        }
        if !design.enabled && self.gain_db == 0.0 {
            self.active = false;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Compressor

/// The compressor's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressorSettings {
    pub enabled: bool,
    /// The level, in dBFS, above which the compressor turns the channel down.
    pub threshold_db: f32,
    /// Decibels in for each decibel out above the threshold.
    pub ratio: f32,
    /// The width of the soft knee around the threshold, in dB (0 is a hard knee).
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// A gain added after the compression, in dB.
    pub makeup_db: f32,
}

impl Default for CompressorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -18.0,
            ratio: 3.0,
            knee_db: 6.0,
            attack_ms: 10.0,
            release_ms: 150.0,
            makeup_db: 0.0,
        }
    }
}

impl CompressorSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            enabled: self.enabled,
            threshold_db: clamp_to(
                self.threshold_db,
                &COMPRESSOR_THRESHOLD_DB,
                default.threshold_db,
            ),
            ratio: clamp_to(self.ratio, &COMPRESSOR_RATIO, default.ratio),
            knee_db: clamp_to(self.knee_db, &COMPRESSOR_KNEE_DB, default.knee_db),
            attack_ms: clamp_to(self.attack_ms, &COMPRESSOR_ATTACK_MS, default.attack_ms),
            release_ms: clamp_to(self.release_ms, &COMPRESSOR_RELEASE_MS, default.release_ms),
            makeup_db: clamp_to(self.makeup_db, &COMPRESSOR_MAKEUP_DB, default.makeup_db),
        }
    }

    /// True when the compressor changes nothing: it is off, or at a ratio of 1 with no makeup.
    pub fn is_neutral(&self) -> bool {
        let settings = self.clamped();
        !settings.enabled || (settings.ratio == 1.0 && settings.makeup_db == 0.0)
    }

    /// The compressor's numbers for the engine thread.
    pub(crate) fn design(&self) -> CompressorDesign {
        let settings = self.clamped();
        let enabled = !settings.is_neutral();
        CompressorDesign {
            enabled,
            threshold_db: settings.threshold_db,
            slope: 1.0 - 1.0 / settings.ratio,
            knee_db: settings.knee_db,
            knee_start: db_to_gain(settings.threshold_db - settings.knee_db / 2.0),
            attack: time_coefficient(settings.attack_ms),
            release: time_coefficient(if enabled {
                settings.release_ms
            } else {
                SWITCH_OFF_MS
            }),
            makeup_db: if enabled { settings.makeup_db } else { 0.0 },
        }
    }
}

/// The compressor's settings worked out for the engine thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CompressorDesign {
    enabled: bool,
    threshold_db: f32,
    /// `1 - 1 / ratio`: the share of each decibel over the threshold taken away.
    slope: f32,
    knee_db: f32,
    /// The linear level where the knee starts: below it, nothing to work out.
    knee_start: f32,
    attack: f32,
    release: f32,
    makeup_db: f32,
}

impl Default for CompressorDesign {
    fn default() -> Self {
        CompressorSettings::default().design()
    }
}

impl CompressorDesign {
    /// The static curve: how many dB a steady level of `level_db` dBFS is turned down.
    pub(crate) fn reduction_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        if 2.0 * over <= -self.knee_db {
            0.0
        } else if 2.0 * over < self.knee_db {
            let into = over + self.knee_db / 2.0;
            self.slope * into * into / (2.0 * self.knee_db)
        } else {
            self.slope * over
        }
    }
}

/// A running compressor.
#[derive(Clone, Debug)]
pub(crate) struct Compressor {
    design: CompressorDesign,
    channels: usize,
    /// The smoothed reduction in dB, 0 or more.
    reduction_db: f32,
    makeup_db: Glide,
    active: bool,
}

impl Compressor {
    /// A compressor that does nothing, for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            design: CompressorDesign::default(),
            channels: channels.clamp(1, super::MAX_CHANNELS),
            reduction_db: 0.0,
            makeup_db: Glide::new(0.0),
            active: false,
        }
    }

    /// Takes `design`: the reduction moves with the attack or the release, the makeup glides.
    pub(crate) fn apply(&mut self, design: &CompressorDesign) {
        self.design = *design;
        self.makeup_db.set(design.makeup_db, GLIDE_FRAMES);
        if design.enabled {
            self.active = true;
        }
    }

    /// False when the samples go through untouched.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// How far the compressor turns the channel down now, in dB, before the makeup.
    pub(crate) fn gain_reduction_db(&self) -> f32 {
        self.reduction_db
    }

    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        if !self.active {
            return;
        }
        let design = self.design;
        for frame in channel_frames(block, stride, first, self.channels) {
            let peak = frame_peak(frame);
            let target = if design.enabled && peak > design.knee_start {
                design.reduction_db(gain_to_db(peak))
            } else {
                0.0
            };
            let coefficient = if target > self.reduction_db {
                design.attack
            } else {
                design.release
            };
            self.reduction_db = smooth_db(self.reduction_db, target, coefficient);
            let gain_db = self.makeup_db.next() - self.reduction_db;
            if gain_db != 0.0 {
                apply_gain(frame, db_to_gain(gain_db));
            }
        }
        if !design.enabled && self.reduction_db == 0.0 && self.makeup_db.is_settled() {
            self.active = false;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Limiter

/// The limiter's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LimiterSettings {
    pub enabled: bool,
    /// The highest peak let through, in dBFS.
    pub ceiling_db: f32,
    pub release_ms: f32,
}

impl Default for LimiterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            ceiling_db: -1.0,
            release_ms: 50.0,
        }
    }
}

impl LimiterSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            enabled: self.enabled,
            ceiling_db: clamp_to(self.ceiling_db, &LIMITER_CEILING_DB, default.ceiling_db),
            release_ms: clamp_to(self.release_ms, &LIMITER_RELEASE_MS, default.release_ms),
        }
    }

    /// True when the limiter changes nothing: it is off.
    pub fn is_neutral(&self) -> bool {
        !self.enabled
    }

    /// The limiter's numbers for the engine thread.
    pub(crate) fn design(&self) -> LimiterDesign {
        let settings = self.clamped();
        LimiterDesign {
            enabled: settings.enabled,
            ceiling: db_to_gain(settings.ceiling_db),
            release: time_coefficient(if settings.enabled {
                settings.release_ms
            } else {
                SWITCH_OFF_MS
            }),
        }
    }
}

/// The limiter's settings worked out for the engine thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LimiterDesign {
    enabled: bool,
    /// The linear ceiling, at most 1.
    ceiling: f32,
    release: f32,
}

impl Default for LimiterDesign {
    fn default() -> Self {
        LimiterSettings::default().design()
    }
}

/// A running peak limiter.
#[derive(Clone, Debug)]
pub(crate) struct Limiter {
    design: LimiterDesign,
    channels: usize,
    /// How far below 1 the linear gain is (the gain is `1 - attenuation`). Kept as the distance,
    /// not the gain: near 1, the last steps of the release are finer than an f32 can tell apart
    /// from 1, and the gain would stop short of it.
    attenuation: f32,
    /// Samples that were not finite numbers, sent on as silence.
    non_finite: u64,
    active: bool,
}

impl Limiter {
    /// A limiter that does nothing, for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            design: LimiterDesign::default(),
            channels: channels.clamp(1, super::MAX_CHANNELS),
            attenuation: 0.0,
            non_finite: 0,
            active: false,
        }
    }

    /// Takes `design`; the gain recovers from where it is with the release.
    pub(crate) fn apply(&mut self, design: &LimiterDesign) {
        self.design = *design;
        if design.enabled {
            self.active = true;
        }
    }

    /// False when the samples go through untouched.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// The samples that were not finite numbers (NaN or an infinity), sent on as silence.
    pub(crate) fn non_finite_samples(&self) -> u64 {
        self.non_finite
    }

    /// How far the limiter turns the signal down now, in dB. Works out a logarithm: read it once
    /// a block, not per sample.
    pub(crate) fn gain_reduction_db(&self) -> f32 {
        if self.attenuation <= 0.0 {
            0.0
        } else {
            -gain_to_db(1.0 - self.attenuation)
        }
    }

    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        if !self.active {
            return;
        }
        let design = self.design;
        for frame in channel_frames(block, stride, first, self.channels) {
            // NaN would slip past the peak and the clip, and an infinity would turn the gain into
            // NaN: either goes on as silence.
            for sample in frame.iter_mut() {
                if !sample.is_finite() {
                    *sample = 0.0;
                    self.non_finite += 1;
                }
            }
            // The release: the attenuation falls away, gone once below a millionth.
            let released = design.release * self.attenuation;
            self.attenuation = if released < 1e-6 { 0.0 } else { released };
            if design.enabled {
                // The attack, at once: just what keeps this frame at the ceiling.
                let peak = frame_peak(frame);
                if peak * (1.0 - self.attenuation) > design.ceiling {
                    self.attenuation = 1.0 - design.ceiling / peak;
                }
                apply_gain(frame, 1.0 - self.attenuation);
                // The safety clip: only the rounding of the gain can reach it.
                for sample in frame.iter_mut() {
                    *sample = sample.clamp(-design.ceiling, design.ceiling);
                }
            } else {
                apply_gain(frame, 1.0 - self.attenuation);
            }
        }
        if !design.enabled && self.attenuation == 0.0 {
            self.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{awkward_signal, same_bits, sine};
    use super::*;

    fn gate_settings() -> GateSettings {
        GateSettings {
            enabled: true,
            threshold_db: -40.0,
            range_db: 30.0,
            attack_ms: 0.5,
            hold_ms: 10.0,
            release_ms: 20.0,
        }
    }

    fn compressor_settings(threshold_db: f32, ratio: f32, knee_db: f32) -> CompressorSettings {
        CompressorSettings {
            enabled: true,
            threshold_db,
            ratio,
            knee_db,
            attack_ms: 1.0,
            release_ms: 20.0,
            makeup_db: 0.0,
        }
    }

    fn limiter_settings() -> LimiterSettings {
        LimiterSettings {
            enabled: true,
            ceiling_db: -1.0,
            release_ms: 50.0,
        }
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
    }

    fn largest_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0f32, |step, pair| step.max((pair[1] - pair[0]).abs()))
    }

    #[test]
    fn neutral_or_switched_off_dynamics_leave_the_samples_bit_for_bit() {
        let input = awkward_signal(8_192);
        let gates = [
            GateSettings::default(),
            GateSettings {
                range_db: 0.0,
                ..gate_settings()
            },
        ];
        for settings in gates {
            assert!(settings.is_neutral());
            let mut gate = Gate::new(2);
            gate.apply(&settings.design());
            let mut output = input.clone();
            gate.process(&mut output, 2, 0);
            assert!(!gate.is_active() && same_bits(&input, &output));
        }
        let compressors = [
            CompressorSettings::default(),
            CompressorSettings {
                ratio: 1.0,
                ..compressor_settings(-30.0, 4.0, 6.0)
            },
        ];
        for settings in compressors {
            assert!(settings.is_neutral());
            let mut compressor = Compressor::new(2);
            compressor.apply(&settings.design());
            let mut output = input.clone();
            compressor.process(&mut output, 2, 0);
            assert!(!compressor.is_active() && same_bits(&input, &output));
        }
        let mut limiter = Limiter::new(2);
        limiter.apply(&LimiterSettings::default().design());
        let mut output = input.clone();
        limiter.process(&mut output, 2, 0);
        assert!(!limiter.is_active() && same_bits(&input, &output));
    }

    #[test]
    fn switched_off_while_working_each_glides_out_then_leaves_the_samples_alone() {
        let hot = sine(200.0, 0.9, 2 * 24_000);
        let input = awkward_signal(4_096);
        let mut gate = Gate::new(2);
        let mut compressor = Compressor::new(2);
        let mut limiter = Limiter::new(2);
        gate.apply(&gate_settings().design());
        compressor.apply(
            &CompressorSettings {
                makeup_db: 6.0,
                ..compressor_settings(-30.0, 4.0, 6.0)
            }
            .design(),
        );
        limiter.apply(
            &LimiterSettings {
                ceiling_db: -12.0,
                ..limiter_settings()
            }
            .design(),
        );
        let mut quiet = sine(200.0, 0.0005, 2 * 24_000);
        gate.process(&mut quiet, 2, 0);
        let mut warm = hot.clone();
        compressor.process(&mut warm, 2, 0);
        limiter.process(&mut hot.clone(), 2, 0);
        assert!(gate.gain_reduction_db() > 29.0);
        assert!(compressor.gain_reduction_db() > 5.0);
        assert!(limiter.gain_reduction_db() > 5.0);

        gate.apply(&GateSettings::default().design());
        compressor.apply(&CompressorSettings::default().design());
        limiter.apply(&LimiterSettings::default().design());
        let mut tail = hot.clone();
        gate.process(&mut tail, 2, 0);
        compressor.process(&mut tail, 2, 0);
        limiter.process(&mut tail, 2, 0);
        assert!(!gate.is_active(), "{gate:?}");
        assert!(!compressor.is_active(), "{compressor:?}");
        assert!(!limiter.is_active(), "{limiter:?}");
        let mut output = input.clone();
        gate.process(&mut output, 2, 0);
        compressor.process(&mut output, 2, 0);
        limiter.process(&mut output, 2, 0);
        assert!(same_bits(&input, &output));
    }

    #[test]
    fn the_gate_closes_by_its_range_below_the_threshold_and_opens_above() {
        let mut gate = Gate::new(1);
        gate.apply(&gate_settings().design());
        // -6 dBFS, far above the -40 dB threshold: open, untouched.
        let loud = sine(1_000.0, 0.5, 24_000);
        let mut output = loud.clone();
        gate.process(&mut output, 1, 0);
        assert_eq!(gate.gain_reduction_db(), 0.0);
        assert!(same_bits(&loud[4_800..], &output[4_800..]));
        // -60 dBFS: after the hold and the release, 30 dB down.
        let quiet = sine(1_000.0, 0.001, 24_000);
        let mut output = quiet.clone();
        gate.process(&mut output, 1, 0);
        assert!((gate.gain_reduction_db() - 30.0).abs() < 1e-3);
        let expected = db_to_gain(-30.0);
        for (out, input) in output[18_000..].iter().zip(&quiet[18_000..]) {
            assert!((out - input * expected).abs() <= 1e-9);
        }
        // Loud again: open within a couple of milliseconds.
        let mut output = loud.clone();
        gate.process(&mut output, 1, 0);
        assert_eq!(gate.gain_reduction_db(), 0.0);
        assert!(same_bits(&loud[480..], &output[480..]));
    }

    #[test]
    fn the_gate_holds_open_before_it_releases() {
        let mut gate = Gate::new(1);
        gate.apply(
            &GateSettings {
                hold_ms: 100.0,
                ..gate_settings()
            }
            .design(),
        );
        let mut loud = sine(1_000.0, 0.5, 4_800);
        gate.process(&mut loud, 1, 0);
        // 50 ms of silence: still inside the 100 ms hold (and the detector's 10 ms fall).
        let mut silence = vec![0.0f32; 2_400];
        gate.process(&mut silence, 1, 0);
        assert_eq!(gate.gain_reduction_db(), 0.0);
        let mut silence = vec![0.0f32; 9_600];
        gate.process(&mut silence, 1, 0);
        assert!(gate.gain_reduction_db() > 29.0);
    }

    #[test]
    fn the_compressor_follows_its_static_curve() {
        // Steady (DC) levels, so the detector sees exactly the level.
        let cases = [
            // threshold, ratio, knee, input dBFS, expected output dBFS
            (-20.0, 4.0, 0.0, -30.0, -30.0),
            (-20.0, 4.0, 0.0, -6.0, -20.0 + 14.0 / 4.0),
            (-20.0, 20.0, 0.0, -2.0, -20.0 + 18.0 / 20.0),
            (-20.0, 1.0, 0.0, -2.0, -2.0),
            // The soft knee: at the threshold, (1 - 1/4) * 10 / 8 = 0.9375 dB down.
            (-20.0, 4.0, 10.0, -20.0, -20.9375),
            (-20.0, 4.0, 10.0, -26.0, -26.0),
            (-20.0, 4.0, 10.0, -10.0, -20.0 + 10.0 / 4.0),
        ];
        for (threshold, ratio, knee, input_db, expected_db) in cases {
            let settings = compressor_settings(threshold, ratio, knee);
            let mut compressor = Compressor::new(1);
            compressor.apply(&settings.design());
            let level = db_to_gain(input_db);
            let mut signal = vec![level; 48_000];
            compressor.process(&mut signal, 1, 0);
            let output_db = gain_to_db(*signal.last().unwrap());
            assert!(
                (output_db - expected_db).abs() < 0.02,
                "{threshold} dB, {ratio}:1, knee {knee} dB, in {input_db} dBFS: out \
                 {output_db:.3}, expected {expected_db:.3}"
            );
            let reduction = compressor.gain_reduction_db();
            assert!((reduction - (input_db - expected_db)).abs() < 0.02);
        }
    }

    #[test]
    fn a_linked_pair_shares_one_detector_on_the_louder_side() {
        let settings = compressor_settings(-20.0, 4.0, 0.0);
        let mut compressor = Compressor::new(2);
        compressor.apply(&settings.design());
        // Left at -6 dBFS, right at -30 dBFS: both turned down by the left's 10.5 dB.
        let (left, right) = (db_to_gain(-6.0), db_to_gain(-30.0));
        let mut block: Vec<f32> = (0..48_000).flat_map(|_| [left, right]).collect();
        compressor.process(&mut block, 2, 0);
        let last = &block[block.len() - 2..];
        assert!((gain_to_db(last[0]) - (-16.5)).abs() < 0.02);
        assert!((gain_to_db(last[1]) - (-40.5)).abs() < 0.02);
    }

    #[test]
    fn the_limiter_never_lets_a_hot_signal_past_its_ceiling() {
        let settings = limiter_settings();
        let ceiling = db_to_gain(settings.ceiling_db);
        let mut limiter = Limiter::new(2);
        limiter.apply(&settings.design());
        // A +6 dBFS tone with spikes up to +20 dBFS on either side.
        let mut signal: Vec<f32> = sine(1_000.0, 2.0, 48_000)
            .into_iter()
            .zip(sine(330.0, 1.5, 48_000))
            .flat_map(|(left, right)| [left, right])
            .collect();
        for (index, sample) in signal.iter_mut().enumerate().step_by(997) {
            *sample = if index % 2 == 0 { 10.0 } else { -10.0 };
        }
        for block in signal.chunks_mut(2 * 256) {
            limiter.process(block, 2, 0);
            assert!(peak(block) <= ceiling, "a peak of {}", peak(block));
        }
        assert!(limiter.gain_reduction_db() > 6.0);
        // It works by gain, not by the clip: the tone comes out a tone, at the ceiling.
        assert!(peak(&signal[48_000..]) > ceiling * 0.95);
    }

    /// NaN or an infinity reaching the limiter goes out as silence, counted: the output stays a
    /// number inside the ceiling, and the gain is not poisoned, so the signal after it is limited
    /// as before.
    #[test]
    fn the_limiter_turns_non_finite_samples_into_silence_and_stays_under_its_ceiling() {
        let settings = limiter_settings();
        let ceiling = db_to_gain(settings.ceiling_db);
        let mut limiter = Limiter::new(2);
        limiter.apply(&settings.design());
        let mut signal: Vec<f32> = sine(1_000.0, 2.0, 2 * 4_800);
        for (index, sample) in signal.iter_mut().enumerate().step_by(101) {
            *sample = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][index % 3];
        }
        let poisoned = signal.iter().filter(|s| !s.is_finite()).count() as u64;
        for block in signal.chunks_mut(2 * 256) {
            limiter.process(block, 2, 0);
            assert!(
                block.iter().all(|s| s.is_finite() && s.abs() <= ceiling),
                "{block:?}"
            );
        }
        assert_eq!(limiter.non_finite_samples(), poisoned);
        assert!(limiter.gain_reduction_db().is_finite());
        let mut after = sine(1_000.0, 2.0, 2 * 4_800);
        limiter.process(&mut after, 2, 0);
        assert!(after.iter().all(|s| s.is_finite() && s.abs() <= ceiling));
        assert!(peak(&after[4_800..]) > ceiling * 0.95);
    }

    #[test]
    fn a_signal_under_the_ceiling_goes_through_the_limiter_unchanged() {
        let mut limiter = Limiter::new(1);
        limiter.apply(&limiter_settings().design());
        let input = sine(1_000.0, 0.5, 4_800);
        let mut output = input.clone();
        limiter.process(&mut output, 1, 0);
        assert!(same_bits(&input, &output));
        assert_eq!(limiter.gain_reduction_db(), 0.0);
    }

    #[test]
    fn a_makeup_change_glides_instead_of_jumping() {
        // A 100 Hz tone at 0.25 under a ratio-1 compressor whose makeup goes from 0 to +12 dB:
        // no step between two samples is larger than the louder tone's own.
        let mut compressor = Compressor::new(1);
        compressor.apply(
            &CompressorSettings {
                ratio: 1.0,
                makeup_db: 0.5,
                ..compressor_settings(-20.0, 1.0, 0.0)
            }
            .design(),
        );
        let mut signal = sine(100.0, 0.25, 48_000);
        // The change lands on a crest of the tone, where a jump would be largest.
        compressor.process(&mut signal[..24_120], 1, 0);
        compressor.apply(
            &CompressorSettings {
                ratio: 1.0,
                makeup_db: 12.0,
                ..compressor_settings(-20.0, 1.0, 0.0)
            }
            .design(),
        );
        compressor.process(&mut signal[24_120..], 1, 0);
        let moving = largest_step(&signal[23_000..30_000]);
        let settled = largest_step(&signal[40_000..]);
        assert!(
            moving <= settled * 1.05,
            "moving {moving}, settled {settled}"
        );
    }

    #[test]
    fn the_gate_fades_its_gain_instead_of_switching_it() {
        // A tone dropping from -6 dBFS to -50 dBFS: the gate's gain falls with its 20 ms release
        // (under 1.5 dB per 32 frames), then rises with its 0.5 ms attack when the tone returns
        // (under 3 dB per 2 frames): never the 30 dB switch at once.
        fn steps(gate: &mut Gate, signal: &mut [f32], frames: usize) -> f32 {
            let mut previous = gate.gain_reduction_db();
            let mut largest = 0.0f32;
            for block in signal.chunks_mut(frames) {
                gate.process(block, 1, 0);
                let reduction = gate.gain_reduction_db();
                largest = largest.max((reduction - previous).abs());
                previous = reduction;
            }
            largest
        }
        let mut gate = Gate::new(1);
        gate.apply(&gate_settings().design());
        gate.process(&mut sine(100.0, 0.5, 24_000), 1, 0);
        assert!(steps(&mut gate, &mut sine(100.0, 0.003, 24_000), 32) < 1.5);
        assert!(gate.gain_reduction_db() > 29.0);
        assert!(steps(&mut gate, &mut sine(100.0, 0.5, 4_800), 2) < 3.0);
        assert_eq!(gate.gain_reduction_db(), 0.0);
    }

    #[test]
    fn detectors_never_decay_into_subnormals() {
        let mut gate = Gate::new(1);
        gate.apply(&gate_settings().design());
        let mut compressor = Compressor::new(1);
        compressor.apply(&compressor_settings(-20.0, 4.0, 6.0).design());
        let mut signal = sine(50.0, 0.8, 4_800);
        signal.resize(48_000 * 4, 0.0);
        for block in signal.chunks_mut(256) {
            gate.process(block, 1, 0);
            compressor.process(block, 1, 0);
            assert!(gate.envelope == 0.0 || gate.envelope.is_normal());
            assert!(block.iter().all(|s| *s == 0.0 || s.is_normal()));
        }
        assert_eq!(gate.envelope, 0.0);
        assert_eq!(compressor.gain_reduction_db(), 0.0);
    }

    #[test]
    fn applying_and_running_never_allocate() {
        let designs = (
            gate_settings().design(),
            compressor_settings(-20.0, 4.0, 6.0).design(),
            limiter_settings().design(),
        );
        let mut gate = Gate::new(2);
        let mut compressor = Compressor::new(2);
        let mut limiter = Limiter::new(2);
        let mut block = sine(440.0, 1.5, 2 * 512);
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            gate.apply(&designs.0);
            compressor.apply(&designs.1);
            limiter.apply(&designs.2);
            gate.process(&mut block, 2, 0);
            compressor.process(&mut block, 2, 0);
            limiter.process(&mut block, 2, 0);
            let _ = (
                gate.gain_reduction_db(),
                compressor.gain_reduction_db(),
                limiter.gain_reduction_db(),
            );
        });
        assert_eq!((allocations, frees), (0, 0));
    }

    #[test]
    fn numbers_out_of_range_are_held_at_their_ends() {
        let gate = GateSettings {
            threshold_db: -200.0,
            range_db: f32::NAN,
            attack_ms: 0.0,
            hold_ms: 1e9,
            release_ms: -5.0,
            enabled: true,
        }
        .clamped();
        assert_eq!(
            (gate.threshold_db, gate.range_db, gate.attack_ms),
            (-80.0, 40.0, 0.05)
        );
        assert_eq!((gate.hold_ms, gate.release_ms), (2_000.0, 5.0));
        let compressor = CompressorSettings {
            threshold_db: 6.0,
            ratio: 0.5,
            knee_db: 100.0,
            attack_ms: f32::INFINITY,
            release_ms: 0.0,
            makeup_db: -3.0,
            enabled: true,
        }
        .clamped();
        assert_eq!(
            (
                compressor.threshold_db,
                compressor.ratio,
                compressor.knee_db
            ),
            (0.0, 1.0, 24.0)
        );
        assert_eq!(
            (
                compressor.attack_ms,
                compressor.release_ms,
                compressor.makeup_db
            ),
            (200.0, 10.0, 0.0)
        );
        let limiter = LimiterSettings {
            ceiling_db: 3.0,
            release_ms: 0.0,
            enabled: true,
        }
        .clamped();
        assert_eq!((limiter.ceiling_db, limiter.release_ms), (0.0, 1.0));
    }
}
