//! A channel strip: the processing of one input, or of a linked stereo pair, in a console's order:
//! the high-pass filter, the gate, the equaliser's four bands, the compressor.
//!
//! The high-pass filter comes first so rumble neither opens the gate nor drives the compressor;
//! the gate comes before the equaliser so a boost does not open it on noise; the compressor comes
//! last so it answers to the channel as it is heard.
//!
//! [StripSettings] is plain data that persists with a session and in strip presets. Its
//! [StripSettings::design] holds every number in its range (the ranges are listed in [super::eq]
//! and [super::dynamics]) and works everything out off the engine thread; [StripProcessor::apply]
//! takes the result on the engine thread, copying numbers only. A neutral strip
//! ([StripSettings::is_neutral]) leaves the samples bit for bit, as each of its parts does. A
//! working strip takes a sample that is not a finite number (NaN or an infinity) as silence and
//! counts it ([StripProcessor::non_finite_samples]), and a filter whose state still overflows starts
//! again from silence, so one bad sample never stays in the strip.

use serde::{Deserialize, Serialize};

use super::dynamics::{
    Compressor, CompressorDesign, CompressorSettings, Gate, GateDesign, GateSettings,
};
use super::eq::{Eq, EqDesign, EqSettings};
use super::MAX_CHANNELS;

/// A channel strip's settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StripSettings {
    /// The high-pass filter and the four bands.
    pub eq: EqSettings,
    pub gate: GateSettings,
    pub compressor: CompressorSettings,
}

impl StripSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        Self {
            eq: self.eq.clamped(),
            gate: self.gate.clamped(),
            compressor: self.compressor.clamped(),
        }
    }

    /// True when the strip changes nothing: every part is off or flat.
    pub fn is_neutral(&self) -> bool {
        self.eq.is_neutral() && self.gate.is_neutral() && self.compressor.is_neutral()
    }

    /// The strip worked out for the engine thread.
    pub(crate) fn design(&self) -> StripDesign {
        StripDesign {
            eq: self.eq.design(),
            gate: self.gate.design(),
            compressor: self.compressor.design(),
        }
    }
}

/// A strip's settings worked out for the engine thread: plain numbers, nothing to allocate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct StripDesign {
    eq: EqDesign,
    gate: GateDesign,
    compressor: CompressorDesign,
}

/// The running strip of one input or a linked pair.
#[derive(Clone, Debug)]
pub(crate) struct StripProcessor {
    eq: Eq,
    gate: Gate,
    compressor: Compressor,
    channels: usize,
    /// Samples that were not finite numbers, processed as silence.
    non_finite: u64,
}

impl StripProcessor {
    /// A strip that does nothing, for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        Self {
            eq: Eq::new(channels),
            gate: Gate::new(channels),
            compressor: Compressor::new(channels),
            channels,
            non_finite: 0,
        }
    }

    /// The channels it processes: 1, or 2 for a linked pair.
    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    /// Takes `design`; every part glides or moves to it without a click.
    pub(crate) fn apply(&mut self, design: &StripDesign) {
        self.eq.apply(&design.eq);
        self.gate.apply(&design.gate);
        self.compressor.apply(&design.compressor);
    }

    /// False when the samples go through untouched.
    pub(crate) fn is_active(&self) -> bool {
        self.eq.is_active() || self.gate.is_active() || self.compressor.is_active()
    }

    /// The gate's gain reduction now, in dB.
    pub(crate) fn gate_reduction_db(&self) -> f32 {
        self.gate.gain_reduction_db()
    }

    /// The compressor's gain reduction now, in dB.
    pub(crate) fn compressor_reduction_db(&self) -> f32 {
        self.compressor.gain_reduction_db()
    }

    /// The samples that were not finite numbers (NaN or an infinity), processed as silence.
    pub(crate) fn non_finite_samples(&self) -> u64 {
        self.non_finite
    }

    /// Processes channels `first..first + channels` of each `stride`-wide frame of `block`, in
    /// place, leaving the other channels alone.
    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        if !self.is_active() {
            return;
        }
        // NaN or an infinity would stay in the filters and detectors for good.
        self.non_finite += super::sanitize(block, stride, first, self.channels);
        self.eq.process_high_pass(block, stride, first);
        self.gate.process(block, stride, first);
        self.eq.process_bands(block, stride, first);
        self.compressor.process(block, stride, first);
    }

    /// Processes frames holding just this strip's channels.
    pub(crate) fn process_frames(&mut self, frames: &mut [f32]) {
        self.process(frames, self.channels, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::db_to_gain;
    use super::super::tests::{awkward_signal, rms, same_bits, sine};
    use super::*;

    fn working() -> StripSettings {
        let mut settings = StripSettings::default();
        settings.eq.high_pass.enabled = true;
        settings.eq.bands[1].gain_db = -4.0;
        settings.eq.bands[3].gain_db = 3.0;
        settings.gate.enabled = true;
        settings.compressor.enabled = true;
        settings.compressor.makeup_db = 2.0;
        settings
    }

    #[test]
    fn a_neutral_strip_leaves_the_samples_bit_for_bit() {
        let settings = StripSettings::default();
        assert!(settings.is_neutral());
        for channels in [1, 2] {
            let mut strip = StripProcessor::new(channels);
            strip.apply(&settings.design());
            assert!(!strip.is_active());
            let input = awkward_signal(8_192);
            let mut output = input.clone();
            strip.process_frames(&mut output);
            assert!(same_bits(&input, &output));
        }
        assert!(!working().is_neutral());
    }

    #[test]
    fn a_strip_turned_off_glides_out_then_leaves_the_samples_alone() {
        let mut strip = StripProcessor::new(2);
        strip.apply(&working().design());
        let mut warm = sine(300.0, 0.7, 2 * 24_000);
        strip.process_frames(&mut warm);
        assert!(strip.compressor_reduction_db() > 1.0);
        strip.apply(&StripSettings::default().design());
        let mut tail = sine(300.0, 0.7, 2 * 24_000);
        strip.process_frames(&mut tail);
        assert!(!strip.is_active());
        let input = awkward_signal(4_096);
        let mut output = input.clone();
        strip.process_frames(&mut output);
        assert!(same_bits(&input, &output));
    }

    #[test]
    fn a_strip_touches_only_its_own_channels_of_a_wide_block() {
        let mut strip = StripProcessor::new(2);
        strip.apply(&working().design());
        let input = awkward_signal(8 * 1_024);
        let mut block = input.clone();
        strip.process(&mut block, 8, 3);
        for (index, (out, original)) in block.iter().zip(&input).enumerate() {
            let channel = index % 8;
            if channel == 3 || channel == 4 {
                continue;
            }
            assert_eq!(out.to_bits(), original.to_bits(), "channel {channel}");
        }
        assert!(!same_bits(&input, &block));
    }

    #[test]
    fn the_high_pass_filter_comes_before_the_gate() {
        // Loud 30 Hz rumble under a quiet tone: with the high-pass filter at 200 Hz in front, the
        // gate hears only the quiet tone and closes.
        let mut settings = StripSettings::default();
        settings.eq.high_pass.enabled = true;
        settings.eq.high_pass.frequency_hz = 200.0;
        settings.gate = GateSettings {
            enabled: true,
            threshold_db: -30.0,
            range_db: 30.0,
            attack_ms: 0.5,
            hold_ms: 10.0,
            release_ms: 20.0,
        };
        let mut strip = StripProcessor::new(1);
        strip.apply(&settings.design());
        let mut signal: Vec<f32> = sine(30.0, 0.3, 48_000)
            .into_iter()
            .zip(sine(1_000.0, 0.005, 48_000))
            .map(|(rumble, tone)| rumble + tone)
            .collect();
        strip.process_frames(&mut signal);
        assert!(strip.gate_reduction_db() > 29.0);
    }

    #[test]
    fn the_equaliser_comes_after_the_gate_and_before_the_compressor() {
        // A -50 dBFS tone under a -45 dB gate threshold, boosted 18 dB by a peak: the gate,
        // before the boost, still closes.
        let mut gated = StripSettings::default();
        gated.eq.bands[2].frequency_hz = 1_000.0;
        gated.eq.bands[2].gain_db = 18.0;
        gated.gate.enabled = true;
        gated.gate.threshold_db = -45.0;
        let mut strip = StripProcessor::new(1);
        strip.apply(&gated.design());
        strip.process_frames(&mut sine(1_000.0, db_to_gain(-50.0), 48_000));
        assert!(strip.gate_reduction_db() > 39.0);

        // A -20 dBFS tone boosted 12 dB into a compressor at -20 dB, 4:1, hard knee: the
        // compressor hears the boosted tone, 12 dB over, and takes 9 dB off.
        let mut compressed = StripSettings::default();
        compressed.eq.bands[2].frequency_hz = 1_000.0;
        compressed.eq.bands[2].gain_db = 12.0;
        compressed.compressor = CompressorSettings {
            enabled: true,
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 0.0,
            attack_ms: 1.0,
            release_ms: 500.0,
            makeup_db: 0.0,
        };
        let mut strip = StripProcessor::new(1);
        strip.apply(&compressed.design());
        let mut signal = sine(1_000.0, 0.1, 96_000);
        strip.process_frames(&mut signal);
        assert!((strip.compressor_reduction_db() - 9.0).abs() < 0.3);
        let out_db = 20.0 * (rms(&signal[48_000..]) * std::f64::consts::SQRT_2).log10();
        assert!(
            (out_db - (-20.0 + 12.0 / 4.0)).abs() < 0.3,
            "out {out_db:.2} dBFS"
        );
    }

    /// NaN and infinities reaching a working strip go on as silence and are counted; nothing in
    /// the strip keeps them, so the next block comes out as from a strip that never saw them.
    #[test]
    fn a_strip_fed_non_finite_samples_counts_them_and_recovers() {
        let mut strip = StripProcessor::new(2);
        strip.apply(&working().design());
        let mut warm = sine(300.0, 0.7, 2 * 24_000);
        strip.process_frames(&mut warm);
        let mut poisoned = sine(300.0, 0.7, 2 * 1_024);
        for (index, sample) in poisoned.iter_mut().enumerate().step_by(37) {
            *sample = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][index % 3];
        }
        let count = poisoned.iter().filter(|s| !s.is_finite()).count() as u64;
        strip.process_frames(&mut poisoned);
        assert!(poisoned.iter().all(|s| s.is_finite()));
        assert_eq!(strip.non_finite_samples(), count);
        assert!(strip.compressor_reduction_db().is_finite());
        assert!(strip.gate_reduction_db().is_finite());
        let mut after = sine(300.0, 0.7, 2 * 24_000);
        strip.process_frames(&mut after);
        assert!(after.iter().all(|s| s.is_finite()));
        // Still the working strip: compressed and heard.
        assert!(strip.compressor_reduction_db() > 1.0);
        assert!(rms(&after[2 * 12_000..]) > 0.1);
    }

    #[test]
    fn applying_and_running_a_strip_never_allocates() {
        let designs = [working().design(), StripSettings::default().design()];
        let mut strip = StripProcessor::new(2);
        let mut block = sine(220.0, 0.8, 32 * 256);
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for design in &designs {
                strip.apply(design);
                for _ in 0..4 {
                    strip.process(&mut block, 32, 6);
                }
                let _ = (strip.gate_reduction_db(), strip.compressor_reduction_db());
            }
        });
        assert_eq!((allocations, frees), (0, 0));
    }

    #[test]
    fn settings_round_trip_through_json_and_missing_parts_take_their_defaults() {
        let settings = working();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"compressor\"") && json.contains("\"makeupDb\""));
        assert_eq!(
            serde_json::from_str::<StripSettings>(&json).unwrap(),
            settings
        );
        let partial: StripSettings =
            serde_json::from_str(r#"{"gate":{"enabled":true,"thresholdDb":-35}}"#).unwrap();
        assert!(partial.gate.enabled);
        assert_eq!(partial.gate.threshold_db, -35.0);
        assert_eq!(partial.gate.range_db, GateSettings::default().range_db);
        assert_eq!(partial.eq, EqSettings::default());
        assert_eq!(partial.compressor, CompressorSettings::default());
    }

    #[test]
    fn a_strip_holds_every_number_in_its_range() {
        let mut wild = working();
        wild.eq.bands[0].gain_db = 99.0;
        wild.gate.threshold_db = f32::NAN;
        wild.compressor.ratio = 1_000.0;
        let clamped = wild.clamped();
        assert_eq!(clamped.eq.bands[0].gain_db, 18.0);
        assert_eq!(
            clamped.gate.threshold_db,
            GateSettings::default().threshold_db
        );
        assert_eq!(clamped.compressor.ratio, 20.0);
        assert_eq!(wild.design(), clamped.design());
    }
}
