//! A channel's equaliser: a high-pass filter and four bands (a low shelf, two peaks and a high
//! shelf), each turned on or off on its own.
//!
//! The settings are plain data that persist with a session. [EqSettings::design] holds every
//! number inside its range and works out the filters' coefficients, off the engine thread;
//! [Eq::apply] hands them to the running filters, which glide to them. A band that is off, or a
//! shelf or peak at 0 dB, is no filter at all, so an equaliser with nothing to do leaves the samples
//! untouched. [EqSettings::magnitude_db] gives the response the desk draws.
//!
//! Ranges (a value outside is held at the nearest end; a value that is not a number takes the
//! default):
//!
//! | Setting | Range | Default |
//! |---|---|---|
//! | High-pass frequency | 20-2,000 Hz | 80 Hz, off |
//! | High-pass Q | 0.5-2 | 0.707 |
//! | Band frequency | 20-20,000 Hz | 100, 400, 2,500 and 8,000 Hz |
//! | Band gain | -18 to +18 dB | 0 dB |
//! | Band Q | 0.3-10 | 0.707 for the shelves, 1 for the peaks |

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::biquad::{Biquad, BiquadCoefficients, FilterShape};
use super::clamp_to;

pub(crate) const HIGH_PASS_FREQUENCY_HZ: RangeInclusive<f32> = 20.0..=2_000.0;
pub(crate) const HIGH_PASS_Q: RangeInclusive<f32> = 0.5..=2.0;
pub(crate) const BAND_FREQUENCY_HZ: RangeInclusive<f32> = 20.0..=20_000.0;
pub(crate) const BAND_GAIN_DB: RangeInclusive<f32> = -18.0..=18.0;
pub(crate) const BAND_Q: RangeInclusive<f32> = 0.3..=10.0;

/// The equaliser's bands, in order.
pub(crate) const EQ_BANDS: usize = 4;

/// The shape of each band, by position: a low shelf, two peaks, a high shelf.
pub(crate) const BAND_SHAPES: [FilterShape; EQ_BANDS] = [
    FilterShape::LowShelf,
    FilterShape::Peak,
    FilterShape::Peak,
    FilterShape::HighShelf,
];

const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The high-pass filter: 12 dB per octave below its frequency.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HighPassSettings {
    pub enabled: bool,
    pub frequency_hz: f32,
    pub q: f32,
}

impl Default for HighPassSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency_hz: 80.0,
            q: BUTTERWORTH_Q,
        }
    }
}

impl HighPassSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            enabled: self.enabled,
            frequency_hz: clamp_to(
                self.frequency_hz,
                &HIGH_PASS_FREQUENCY_HZ,
                default.frequency_hz,
            ),
            q: clamp_to(self.q, &HIGH_PASS_Q, default.q),
        }
    }

    /// True when the filter changes nothing: it is off.
    pub fn is_neutral(&self) -> bool {
        !self.enabled
    }

    fn design(&self) -> Option<BiquadCoefficients> {
        let settings = self.clamped();
        (!settings.is_neutral()).then(|| {
            BiquadCoefficients::design(
                FilterShape::HighPass,
                f64::from(settings.frequency_hz),
                f64::from(settings.q),
                0.0,
            )
        })
    }
}

/// One band of the equaliser; its shape comes from its position (see [BAND_SHAPES]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EqBandSettings {
    pub enabled: bool,
    pub frequency_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for EqBandSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency_hz: 1_000.0,
            gain_db: 0.0,
            q: 1.0,
        }
    }
}

impl EqBandSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            enabled: self.enabled,
            frequency_hz: clamp_to(self.frequency_hz, &BAND_FREQUENCY_HZ, default.frequency_hz),
            gain_db: clamp_to(self.gain_db, &BAND_GAIN_DB, default.gain_db),
            q: clamp_to(self.q, &BAND_Q, default.q),
        }
    }

    /// True when the band changes nothing: it is off, or at 0 dB.
    pub fn is_neutral(&self) -> bool {
        !self.enabled || self.gain_db == 0.0
    }

    fn design(&self, shape: FilterShape) -> Option<BiquadCoefficients> {
        let settings = self.clamped();
        (!settings.is_neutral()).then(|| {
            BiquadCoefficients::design(
                shape,
                f64::from(settings.frequency_hz),
                f64::from(settings.q),
                f64::from(settings.gain_db),
            )
        })
    }
}

/// The equaliser's settings: the high-pass filter and the four bands.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EqSettings {
    pub high_pass: HighPassSettings,
    pub bands: [EqBandSettings; EQ_BANDS],
}

impl Default for EqSettings {
    fn default() -> Self {
        let band = |frequency_hz, q| EqBandSettings {
            frequency_hz,
            q,
            ..EqBandSettings::default()
        };
        Self {
            high_pass: HighPassSettings::default(),
            bands: [
                band(100.0, BUTTERWORTH_Q),
                band(400.0, 1.0),
                band(2_500.0, 1.0),
                band(8_000.0, BUTTERWORTH_Q),
            ],
        }
    }
}

impl EqSettings {
    /// These settings with every number held in its range.
    pub fn clamped(&self) -> Self {
        Self {
            high_pass: self.high_pass.clamped(),
            bands: self.bands.map(|band| band.clamped()),
        }
    }

    /// True when the equaliser changes nothing.
    pub fn is_neutral(&self) -> bool {
        let settings = self.clamped();
        settings.high_pass.is_neutral() && settings.bands.iter().all(EqBandSettings::is_neutral)
    }

    /// The filters for these settings, worked out off the engine thread.
    pub(crate) fn design(&self) -> EqDesign {
        EqDesign {
            high_pass: self.high_pass.design(),
            bands: std::array::from_fn(|index| self.bands[index].design(BAND_SHAPES[index])),
        }
    }

    /// The equaliser's gain at `frequency_hz` in dB, the high-pass filter included: the response
    /// curve the desk draws. Works out the filters, so it belongs off the engine thread.
    pub fn magnitude_db(&self, frequency_hz: f64) -> f64 {
        let design = self.design();
        std::iter::once(design.high_pass)
            .chain(design.bands)
            .flatten()
            .map(|coefficients| coefficients.magnitude_db(frequency_hz))
            .sum()
    }
}

/// The equaliser's filters ready to run: `None` for each that does nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EqDesign {
    pub high_pass: Option<BiquadCoefficients>,
    pub bands: [Option<BiquadCoefficients>; EQ_BANDS],
}

impl Default for EqDesign {
    fn default() -> Self {
        Self {
            high_pass: None,
            bands: [None; EQ_BANDS],
        }
    }
}

/// The running equaliser of one channel or a linked pair.
#[derive(Clone, Debug)]
pub(crate) struct Eq {
    high_pass: Biquad,
    bands: [Biquad; EQ_BANDS],
}

impl Eq {
    /// An equaliser that does nothing, for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            high_pass: Biquad::new(channels),
            bands: std::array::from_fn(|_| Biquad::new(channels)),
        }
    }

    /// Glides every filter to `design`. Copies numbers only: safe on the engine thread.
    pub(crate) fn apply(&mut self, design: &EqDesign) {
        self.high_pass.set(design.high_pass.as_ref());
        for (band, coefficients) in self.bands.iter_mut().zip(&design.bands) {
            band.set(coefficients.as_ref());
        }
    }

    /// False when no filter touches the samples.
    pub(crate) fn is_active(&self) -> bool {
        self.high_pass.is_active() || self.bands.iter().any(Biquad::is_active)
    }

    /// Runs the high-pass filter alone (the strip runs it before the gate).
    pub(crate) fn process_high_pass(&mut self, block: &mut [f32], stride: usize, first: usize) {
        self.high_pass.process(block, stride, first);
    }

    /// Runs the four bands.
    pub(crate) fn process_bands(&mut self, block: &mut [f32], stride: usize, first: usize) {
        for band in &mut self.bands {
            band.process(block, stride, first);
        }
    }

    /// Runs the high-pass filter, then the bands.
    #[cfg(test)]
    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        self.process_high_pass(block, stride, first);
        self.process_bands(block, stride, first);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{awkward_signal, rms, same_bits, sine};
    use super::*;

    fn boosted() -> EqSettings {
        let mut settings = EqSettings::default();
        settings.high_pass.enabled = true;
        settings.high_pass.frequency_hz = 60.0;
        settings.bands[0].gain_db = 4.0;
        settings.bands[1].gain_db = -6.0;
        settings.bands[2].gain_db = 5.0;
        settings.bands[3].gain_db = -3.0;
        settings
    }

    #[test]
    fn a_flat_or_switched_off_equaliser_leaves_the_samples_bit_for_bit() {
        let mut off = boosted();
        off.high_pass.enabled = false;
        for band in &mut off.bands {
            band.enabled = false;
        }
        for settings in [EqSettings::default(), off] {
            assert!(settings.is_neutral());
            let mut eq = Eq::new(2);
            eq.apply(&settings.design());
            assert!(!eq.is_active());
            let input = awkward_signal(4_096);
            let mut output = input.clone();
            eq.process(&mut output, 2, 0);
            assert!(same_bits(&input, &output));
        }
        assert!(!boosted().is_neutral());
    }

    #[test]
    fn the_response_curve_adds_up_the_filters_that_are_on() {
        let settings = boosted();
        let design = settings.design();
        assert!(design.high_pass.is_some() && design.bands.iter().all(Option::is_some));
        let expected: f64 = std::iter::once(design.high_pass)
            .chain(design.bands)
            .flatten()
            .map(|coefficients| coefficients.magnitude_db(1_000.0))
            .sum();
        assert!((settings.magnitude_db(1_000.0) - expected).abs() < 1e-9);
        // Each peak alone gives its gain at its own frequency.
        let mut single = EqSettings::default();
        single.bands[1].gain_db = -6.0;
        assert!((single.magnitude_db(400.0) + 6.0).abs() < 0.01);
        single.bands[1].enabled = false;
        assert_eq!(single.magnitude_db(400.0), 0.0);
        // The high-pass filter alone: -3 dB at its corner with the default Q.
        let mut high_pass = EqSettings::default();
        high_pass.high_pass.enabled = true;
        assert!((high_pass.magnitude_db(80.0) + 3.01).abs() < 0.05);
        assert!(high_pass.magnitude_db(20.0) < -23.0);
    }

    #[test]
    fn a_running_equaliser_matches_its_response_curve() {
        let settings = boosted();
        let mut eq = Eq::new(1);
        eq.apply(&settings.design());
        for frequency in [100.0, 400.0, 2_500.0, 8_000.0] {
            let input = sine(frequency, 0.2, 96_000);
            let mut output = input.clone();
            for block in output.chunks_mut(128) {
                eq.process(block, 1, 0);
            }
            let tail = 48_000..96_000;
            let measured = 20.0 * (rms(&output[tail.clone()]) / rms(&input[tail])).log10();
            let curve = settings.magnitude_db(frequency);
            assert!(
                (measured - curve).abs() < 0.1,
                "{frequency} Hz: measured {measured:.3}, curve {curve:.3}"
            );
        }
    }

    #[test]
    fn applying_and_running_never_allocate() {
        let design = boosted().design();
        let flat = EqSettings::default().design();
        let mut eq = Eq::new(2);
        let mut block = sine(440.0, 0.3, 2 * 512);
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            eq.apply(&design);
            eq.process(&mut block, 2, 0);
            eq.apply(&flat);
            eq.process(&mut block, 2, 0);
        });
        assert_eq!((allocations, frees), (0, 0));
    }

    #[test]
    fn numbers_out_of_range_are_held_at_their_ends() {
        let mut wild = EqSettings::default();
        wild.high_pass.frequency_hz = 5.0;
        wild.high_pass.q = f32::NAN;
        wild.bands[0].frequency_hz = 96_000.0;
        wild.bands[1].gain_db = 40.0;
        wild.bands[2].q = 0.0;
        wild.bands[3].gain_db = f32::NEG_INFINITY;
        let clamped = wild.clamped();
        assert_eq!(clamped.high_pass.frequency_hz, 20.0);
        assert_eq!(clamped.high_pass.q, BUTTERWORTH_Q);
        assert_eq!(clamped.bands[0].frequency_hz, 20_000.0);
        assert_eq!(clamped.bands[1].gain_db, 18.0);
        assert_eq!(clamped.bands[2].q, 0.3);
        assert_eq!(clamped.bands[3].gain_db, -18.0);
        assert!(wild.magnitude_db(1_000.0).is_finite());
    }

    #[test]
    fn settings_read_back_from_json_with_missing_fields_defaulted() {
        let settings = boosted();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"highPass\"") && json.contains("\"gainDb\""));
        assert_eq!(serde_json::from_str::<EqSettings>(&json).unwrap(), settings);
        let partial: EqSettings = serde_json::from_str(r#"{"highPass":{"enabled":true}}"#).unwrap();
        assert!(partial.high_pass.enabled);
        assert_eq!(partial.high_pass.frequency_hz, 80.0);
        assert_eq!(partial.bands, EqSettings::default().bands);
    }
}
