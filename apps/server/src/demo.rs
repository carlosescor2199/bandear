//! The demo source: a generated band in place of an audio interface, so the engine runs, streams to
//! phones and can be shown on a machine with no interface at all.
//!
//! ```text
//!  engine thread (one)                                                   sinks
//!  DemoReader: when the clock makes a block due, the generator writes ─▶ phones, local output,
//!  it (one voice per channel), metered as capture meters its own         recorder, meters
//! ```
//!
//! It is chosen as its own driver, [crate::AudioHost::Demo], with one device,
//! [DEMO_DEVICE_NAME]: it is never what `Auto` resolves to, so a real interface is never passed
//! over for it, and choosing it or leaving it opens, lists and loads no device and no driver (ASIO
//! included). The engine then opens no capture stream: the engine thread writes the demo's blocks
//! itself, in the layout a [DEMO_CHANNELS]-input device at 48 kHz would have, paced by the same
//! clock a virtual soundcheck uses ([crate::soundcheck::BlockPacer]): the local output's callbacks
//! when one plays, else the engine thread's timer.
//!
//! The signal. [DEMO_CHANNELS] voices of a four-bar loop at 120 BPM ([LOOP_FRAMES], 8 s): a kick
//! (a falling sine on every beat), a snare (noise and a tone on beats 2 and 4), a hi-hat (noise on
//! every eighth), a bass (the chord's root on every eighth), keys (the chord on every
//! beat), a pad (the chord an octave up, held), a vocal-ish line (a melody with vibrato and a slowly
//! sweeping tone) and room noise (pink-ish). A device with more channels repeats the voices in that
//! order. Each voice averages [DEMO_LEVEL_DBFS] RMS over the loop, unless that would take its peak
//! past [DEMO_PEAK_DBFS]: its gain is measured once per process, off the engine thread.
//!
//! Every sample is a pure function of its frame in the loop (the noise is a hash of the frame, not a
//! running generator), so the demo is the same on every run and whatever the block size, and
//! writing it allocates, locks and waits for nothing.

use std::f64::consts::TAU;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::pipeline::CaptureStats;
use crate::soundcheck::BlockPacer;

/// The demo source's one device, under its own driver ([crate::AudioHost::Demo]).
pub const DEMO_DEVICE_NAME: &str = "Demo (no interface)";

/// The inputs the demo source has: one voice each.
pub const DEMO_CHANNELS: u16 = 8;

/// The voices, in channel order; a source with more channels repeats them.
pub const DEMO_VOICES: [&str; VOICES] = [
    "Kick", "Snare", "Hi-hat", "Bass", "Keys", "Pad", "Vocal", "Room",
];

/// Each voice's average level over the loop, RMS in dBFS.
pub const DEMO_LEVEL_DBFS: f32 = -18.0;

/// No voice peaks above this, in dBFS: a voice whose average would take it past it is quieter.
pub const DEMO_PEAK_DBFS: f32 = -1.0;

const VOICES: usize = 8;

const RATE: f64 = TARGET_SAMPLE_RATE as f64;

/// One beat at 120 BPM.
const BEAT_FRAMES: u32 = TARGET_SAMPLE_RATE / 2;
const EIGHTH_FRAMES: u32 = BEAT_FRAMES / 2;
const BAR_FRAMES: u32 = 4 * BEAT_FRAMES;

/// The loop: four bars, 8 s.
pub(crate) const LOOP_FRAMES: u32 = 4 * BAR_FRAMES;

/// The chord of each bar (A minor, F, C, G), in Hz, and its bass root.
const CHORDS: [[f64; 3]; 4] = [
    [220.00, 261.63, 329.63],
    [174.61, 220.00, 261.63],
    [261.63, 329.63, 392.00],
    [196.00, 246.94, 293.66],
];
const ROOTS: [f64; 4] = [110.00, 87.31, 130.81, 98.00];

/// The vocal line: one note every two beats.
const MELODY: [f64; 8] = [
    329.63, 440.00, 440.00, 523.25, 392.00, 329.63, 293.66, 246.94,
];

/// A uniform value in [-1, 1) from the frame `t` and a `salt`: white noise with no state.
fn noise(t: u32, salt: u32) -> f32 {
    let mut h = t.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h as f32) * (1.0 / 2_147_483_648.0) - 1.0
}

/// `sin(2π cycles)`, with the whole cycles taken off first so a long phase stays exact.
fn sine(cycles: f64) -> f32 {
    (cycles.fract() * TAU).sin() as f32
}

/// `seconds` of decay with time constant `tau`.
fn decay(seconds: f64, tau: f64) -> f32 {
    (-seconds / tau).exp() as f32
}

/// A fade in over `rise` and out over `fall` seconds of a note `length` seconds long.
fn envelope(seconds: f64, length: f64, rise: f64, fall: f64) -> f32 {
    let fade_in = (seconds / rise).min(1.0);
    let fade_out = ((length - seconds) / fall).clamp(0.0, 1.0);
    (fade_in * fade_out) as f32
}

fn seconds(frames: u32) -> f64 {
    f64::from(frames) / RATE
}

/// One voice's sample at frame `t` of the loop, before its gain.
fn voice(index: usize, t: u32) -> f32 {
    let bar = (t / BAR_FRAMES) as usize % CHORDS.len();
    let beat = t / BEAT_FRAMES;
    let in_beat = seconds(t % BEAT_FRAMES);
    let in_eighth = seconds(t % EIGHTH_FRAMES);
    match index {
        // Kick: a sine falling from 120 to 50 Hz, on every beat.
        0 => {
            let cycles = 50.0 * in_beat + 70.0 * 0.03 * (1.0 - (-in_beat / 0.03).exp());
            sine(cycles) * decay(in_beat, 0.12)
        }
        // Snare: noise and a 190 Hz tone, on beats 2 and 4.
        1 => {
            if beat % 2 == 1 {
                0.8 * noise(t, 2) * decay(in_beat, 0.25)
                    + 0.5 * sine(190.0 * in_beat) * decay(in_beat, 0.12)
            } else {
                0.0
            }
        }
        // Hi-hat: white noise, on every eighth.
        2 => noise(t, 3) * decay(in_eighth, 0.05),
        // Bass: the chord's root and its octave, on every eighth.
        3 => {
            let root = ROOTS[bar];
            let tone = sine(root * in_eighth) + 0.3 * sine(2.0 * root * in_eighth);
            tone * envelope(in_eighth, seconds(EIGHTH_FRAMES), 0.005, 0.02) * decay(in_eighth, 0.2)
        }
        // Keys: the chord, struck on every beat.
        4 => {
            let chord = CHORDS[bar];
            let tone: f32 = chord
                .iter()
                .map(|&frequency| sine(frequency * in_beat) + 0.2 * sine(2.0 * frequency * in_beat))
                .sum();
            tone * envelope(in_beat, seconds(BEAT_FRAMES), 0.004, 0.02) * decay(in_beat, 0.35)
        }
        // Pad: the chord an octave up, slightly detuned, held for the bar.
        5 => {
            let in_bar = seconds(t % BAR_FRAMES);
            let chord = CHORDS[bar];
            let tone: f32 = chord
                .iter()
                .map(|&frequency| {
                    sine(2.0 * frequency * 1.003 * in_bar) + sine(2.0 * frequency * 0.997 * in_bar)
                })
                .sum();
            tone * envelope(in_bar, seconds(BAR_FRAMES), 0.3, 0.1)
        }
        // Vocal: a melody note every two beats, with vibrato, its tone sweeping slowly.
        6 => {
            let note_frames = 2 * BEAT_FRAMES;
            let note = MELODY[(t / note_frames) as usize % MELODY.len()];
            let in_note = seconds(t % note_frames);
            // 5.5 Hz vibrato of 0.6 %: the phase of a sinusoidally modulated frequency.
            let vibrato = 5.5;
            let depth = 0.006 * note / vibrato;
            let base = note * in_note + depth * (1.0 - (TAU * vibrato * in_note).cos()) / TAU;
            let sweep = seconds(t) * 0.25;
            let tone: f32 = (1..=4)
                .map(|harmonic| {
                    let weight = 0.5 + 0.5 * sine(sweep + f64::from(harmonic) * 0.25);
                    weight / harmonic as f32 * sine(f64::from(harmonic) * base)
                })
                .sum();
            tone * envelope(in_note, seconds(note_frames), 0.04, 0.06)
        }
        // Room: pink-ish noise, rows of noise held for 1, 2, 4 ... 128 frames.
        _ => (0..8).map(|row| noise(t >> row, 10 + row)).sum::<f32>() / 8.0,
    }
}

/// Each voice's gain: [DEMO_LEVEL_DBFS] RMS over the loop, held to [DEMO_PEAK_DBFS] at its peak.
/// Measured once per process (it takes the whole loop), never on the engine thread.
fn gains() -> &'static [f32; VOICES] {
    static GAINS: OnceLock<[f32; VOICES]> = OnceLock::new();
    GAINS.get_or_init(|| {
        let level = 10f64.powf(f64::from(DEMO_LEVEL_DBFS) / 20.0);
        let ceiling = 10f64.powf(f64::from(DEMO_PEAK_DBFS) / 20.0);
        std::array::from_fn(|index| {
            let (mut power, mut peak) = (0.0f64, 0.0f64);
            for t in 0..LOOP_FRAMES {
                let sample = f64::from(voice(index, t));
                power += sample * sample;
                peak = peak.max(sample.abs());
            }
            let rms = (power / f64::from(LOOP_FRAMES)).sqrt();
            if rms == 0.0 {
                return 0.0;
            }
            (level / rms).min(ceiling / peak) as f32
        })
    })
}

/// Writes the demo's frames: `channels` voices, from the loop's first frame on.
pub(crate) struct DemoGenerator {
    channels: usize,
    gains: &'static [f32; VOICES],
    /// The next frame of the loop to write.
    position: u32,
}

impl DemoGenerator {
    /// A generator of `channels` voices (at least one). Measures the voices' gains the first time
    /// in the process, so it is built off the engine thread.
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            gains: gains(),
            position: 0,
        }
    }

    /// The width of every frame written.
    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    /// Fills `block` (whole interleaved frames of [Self::channels]) with the next frames of the
    /// loop. Never allocates, locks or waits.
    pub(crate) fn fill(&mut self, block: &mut [f32]) {
        for frame in block.chunks_exact_mut(self.channels) {
            let t = self.position;
            for (channel, slot) in frame.iter_mut().enumerate() {
                let index = channel % VOICES;
                *slot = voice(index, t) * self.gains[index];
            }
            self.position = (self.position + 1) % LOOP_FRAMES;
        }
    }
}

/// The demo source on the engine thread: the generator, the clock that paces it and the capture
/// counters it counts its samples in, as a device's callback would.
pub(crate) struct DemoReader {
    generator: DemoGenerator,
    pacer: BlockPacer,
    stats: Arc<CaptureStats>,
}

impl DemoReader {
    /// A demo of `channels` voices paced by `pacer`, counting into `stats`.
    pub(crate) fn new(channels: usize, pacer: BlockPacer, stats: Arc<CaptureStats>) -> Self {
        Self {
            generator: DemoGenerator::new(channels),
            pacer,
            stats,
        }
    }

    pub(crate) fn channels(&self) -> usize {
        self.generator.channels()
    }

    /// The whole blocks of `block_frames` the clock has made due by `now`.
    pub(crate) fn due(&mut self, now: Instant, block_frames: usize) -> usize {
        self.pacer.due(now, block_frames)
    }

    /// How long until the next block of `block_frames` is due on the timer.
    pub(crate) fn wait(&self, block_frames: usize) -> Duration {
        self.pacer.wait(block_frames)
    }

    /// Writes the next block and counts its samples, as a device's callback counts its own.
    pub(crate) fn fill(&mut self, block: &mut [f32]) {
        self.generator.fill(block);
        self.stats
            .samples
            .fetch_add(block.len() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_dbfs(samples: impl Iterator<Item = f32>) -> (f32, f32) {
        let (mut power, mut peak, mut count) = (0.0f64, 0.0f32, 0u64);
        for sample in samples {
            power += f64::from(sample) * f64::from(sample);
            peak = peak.max(sample.abs());
            count += 1;
        }
        let rms = (power / count as f64).sqrt();
        ((20.0 * rms.log10()) as f32, 20.0 * peak.log10())
    }

    /// One whole loop of `channels` voices.
    fn one_loop(channels: usize) -> Vec<f32> {
        let mut generator = DemoGenerator::new(channels);
        let mut samples = vec![0.0; LOOP_FRAMES as usize * channels];
        generator.fill(&mut samples);
        samples
    }

    #[test]
    fn every_voice_averages_minus_18_dbfs_and_never_peaks_past_minus_1() {
        let channels = usize::from(DEMO_CHANNELS);
        let samples = one_loop(channels);
        for (index, name) in DEMO_VOICES.iter().enumerate() {
            let (rms, peak) = level_dbfs(samples.iter().skip(index).step_by(channels).copied());
            assert!(
                (rms - DEMO_LEVEL_DBFS).abs() <= 0.5,
                "{name} averages {rms:.2} dBFS"
            );
            assert!(
                peak <= DEMO_PEAK_DBFS + 0.01,
                "{name} peaks at {peak:.2} dBFS"
            );
        }
    }

    #[test]
    fn the_demo_is_the_same_on_every_run_and_whatever_the_block_size() {
        let channels = usize::from(DEMO_CHANNELS);
        let whole = one_loop(channels);
        assert_eq!(whole, one_loop(channels), "deterministic");

        // In engine blocks of 256 frames, and in odd ones, it is the same loop.
        for block_frames in [256, 37] {
            let mut generator = DemoGenerator::new(channels);
            let mut blocks = Vec::with_capacity(whole.len());
            let mut block = vec![0.0; block_frames * channels];
            while blocks.len() < whole.len() {
                generator.fill(&mut block);
                blocks.extend_from_slice(&block);
            }
            blocks.truncate(whole.len());
            assert_eq!(blocks, whole, "{block_frames}-frame blocks");
        }

        // The loop starts again where it began.
        let mut generator = DemoGenerator::new(channels);
        let mut first = vec![0.0; whole.len()];
        generator.fill(&mut first);
        let mut again = vec![0.0; 64 * channels];
        generator.fill(&mut again);
        assert_eq!(again, whole[..64 * channels]);
    }

    #[test]
    fn each_channel_carries_its_own_voice_and_more_channels_repeat_them() {
        let samples = one_loop(10);
        let channel = |index: usize| -> Vec<f32> {
            samples.iter().skip(index).step_by(10).copied().collect()
        };
        for a in 0..VOICES {
            for b in a + 1..VOICES {
                assert_ne!(
                    channel(a),
                    channel(b),
                    "{} and {}",
                    DEMO_VOICES[a],
                    DEMO_VOICES[b]
                );
            }
        }
        assert_eq!(channel(8), channel(0), "the ninth input is a kick again");
        assert_eq!(channel(9), channel(1));
        // A mono demo is the kick alone.
        let mono = one_loop(1);
        assert_eq!(mono, channel(0));
    }

    #[test]
    fn the_reader_counts_its_samples_as_a_capture_does() {
        let stats = Arc::new(CaptureStats::default());
        let clock = Arc::new(crate::soundcheck::PlaybackClock::new());
        let mut reader = DemoReader::new(8, BlockPacer::new(clock), Arc::clone(&stats));
        let mut block = vec![0.0; 64 * reader.channels()];
        reader.fill(&mut block);
        reader.fill(&mut block);
        assert_eq!(
            stats.samples.load(std::sync::atomic::Ordering::Relaxed),
            2 * 64 * 8
        );

        // The timer makes blocks due with the time that passes: 10 ms are 480 frames.
        let start = Instant::now();
        assert_eq!(reader.due(start, 64), 0);
        assert_eq!(reader.due(start + Duration::from_millis(10), 64), 7);
    }
}
