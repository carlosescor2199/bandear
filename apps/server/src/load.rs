//! How long the engine thread takes for each block, as a share of the time the block lasts.
//!
//! The engine thread must be done with every block before the next one is due: a block of 256
//! frames lasts 5.33 ms at 48 kHz, and a block that takes longer is late. A few late blocks are
//! absorbed by the capture ring; many in a row fill it, and then audio is dropped at the callback,
//! which the status only shows afterwards, as discarded packets. The load shows it coming.
//!
//! The pipeline times each block on the engine thread with the monotonic clock, from taking it
//! (the ring's pop, or the demo's or the take's fill) to the return of the last sink, and hands the
//! time to a [LoadMeter]. The meter works it out as a percentage of the block's period and
//! publishes, through atomics the status reads from any thread:
//!
//! - the last block's load;
//! - an average, smoothed over about a second of audio ([AVERAGE_FRAMES]), whatever the block size;
//! - a peak, the highest block of the last two to four seconds: the highest of the current window
//!   of [PEAK_WINDOW_FRAMES] and the one before it, so a spike stays visible for at least two
//!   seconds and is forgotten after four;
//! - the blocks that took longer than their period since the engine started.
//!
//! Measuring costs two clock reads, some arithmetic and four atomic stores per block: nothing is
//! allocated, locked or written anywhere else.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::capture::TARGET_SAMPLE_RATE;

/// The average's time constant, in frames of audio: one second.
pub(crate) const AVERAGE_FRAMES: f32 = TARGET_SAMPLE_RATE as f32;

/// The length of one peak window, in frames of audio: two seconds.
pub(crate) const PEAK_WINDOW_FRAMES: usize = 2 * TARGET_SAMPLE_RATE as usize;

/// The engine thread's load as the status reports it (see [crate::EngineStatus::load]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineLoadStatus {
    /// The last block's processing time, in percent of the time the block lasts. Over 100 the
    /// block was late.
    pub percent: f32,
    /// The same, smoothed over about a second of audio.
    pub average_percent: f32,
    /// The highest block of the last two to four seconds.
    pub peak_percent: f32,
    /// The blocks that took longer than they last, since the engine started.
    pub late_blocks: u64,
}

/// The load the engine thread publishes: written only by it, read by the status from any thread.
/// The percentages are held as the bits of an `f32`.
#[derive(Default)]
pub(crate) struct EngineLoad {
    percent: AtomicU32,
    average_percent: AtomicU32,
    peak_percent: AtomicU32,
    late_blocks: AtomicU64,
}

impl EngineLoad {
    /// The load as last published; all zero before the first block.
    pub(crate) fn status(&self) -> EngineLoadStatus {
        let read = |value: &AtomicU32| f32::from_bits(value.load(Ordering::Relaxed));
        EngineLoadStatus {
            percent: read(&self.percent),
            average_percent: read(&self.average_percent),
            peak_percent: read(&self.peak_percent),
            late_blocks: self.late_blocks.load(Ordering::Relaxed),
        }
    }
}

/// The engine thread's side of the load: turns each block's time into the published figures.
pub(crate) struct LoadMeter {
    shared: Arc<EngineLoad>,
    /// The smoothed load; `None` before the first block, which it then starts from.
    average: Option<f32>,
    /// The highest block of the current peak window, and how much audio the window has seen.
    window_peak: f32,
    window_frames: usize,
    /// The highest block of the window before it.
    previous_peak: f32,
}

impl Default for LoadMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadMeter {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(EngineLoad::default()),
            average: None,
            window_peak: 0.0,
            window_frames: 0,
            previous_peak: 0.0,
        }
    }

    /// Where the meter publishes, for the status.
    pub(crate) fn shared(&self) -> Arc<EngineLoad> {
        Arc::clone(&self.shared)
    }

    /// One block of `frames` at 48 kHz took `elapsed` to process. A block of no frames lasts no
    /// time and is not counted.
    pub(crate) fn block(&mut self, elapsed: Duration, frames: usize) {
        if frames == 0 {
            return;
        }
        let period = frames as f64 / f64::from(TARGET_SAMPLE_RATE);
        let percent = (elapsed.as_secs_f64() / period * 100.0) as f32;

        // An exponential average by audio time: each block moves it by its own length's share of
        // the time constant, so long and short blocks follow alike.
        let weight = 1.0 - (-(frames as f32) / AVERAGE_FRAMES).exp();
        let average = match self.average {
            Some(average) => average + (percent - average) * weight,
            None => percent,
        };
        self.average = Some(average);

        self.window_peak = self.window_peak.max(percent);
        self.window_frames += frames;
        let peak = self.previous_peak.max(self.window_peak);
        if self.window_frames >= PEAK_WINDOW_FRAMES {
            self.previous_peak = self.window_peak;
            self.window_peak = 0.0;
            self.window_frames = 0;
        }

        let shared = &self.shared;
        shared.percent.store(percent.to_bits(), Ordering::Relaxed);
        shared
            .average_percent
            .store(average.to_bits(), Ordering::Relaxed);
        shared.peak_percent.store(peak.to_bits(), Ordering::Relaxed);
        if percent > 100.0 {
            shared.late_blocks.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 256 frames last 5333 µs: what a block of that length that took `micros` reads.
    fn blocks(meter: &mut LoadMeter, count: usize, micros: u64) {
        for _ in 0..count {
            meter.block(Duration::from_micros(micros), 256);
        }
    }

    #[test]
    fn a_block_reads_as_its_share_of_the_time_it_lasts() {
        let mut meter = LoadMeter::new();
        let load = meter.shared();

        // A quarter of the 5333 µs of 256 frames, then half of the 1000 µs of 48 frames.
        meter.block(Duration::from_nanos(1_333_333), 256);
        assert!(
            (load.status().percent - 25.0).abs() < 0.01,
            "{:?}",
            load.status()
        );
        meter.block(Duration::from_micros(500), 48);
        let status = load.status();
        assert!((status.percent - 50.0).abs() < 0.01, "{status:?}");
        assert_eq!(status.late_blocks, 0);
    }

    #[test]
    fn the_first_block_starts_the_average_which_then_follows_within_a_second() {
        let mut meter = LoadMeter::new();
        let load = meter.shared();

        blocks(&mut meter, 1, 1_067);
        assert!((load.status().average_percent - 20.0).abs() < 0.1);

        // One second of audio (188 blocks of 256 frames) at 60%: the average has covered 63% of
        // the way, as a one-second time constant does, and keeps going.
        blocks(&mut meter, 188, 3_200);
        let average = load.status().average_percent;
        assert!((average - (20.0 + 40.0 * 0.632)).abs() < 0.5, "{average}");
        blocks(&mut meter, 4 * 188, 3_200);
        assert!((load.status().average_percent - 60.0).abs() < 0.5);
    }

    #[test]
    fn the_average_follows_by_audio_time_whatever_the_block_size() {
        let mut short = LoadMeter::new();
        let mut long = LoadMeter::new();
        short.block(Duration::ZERO, 64);
        long.block(Duration::ZERO, 1_024);

        // Half a second at 50%: 375 blocks of 64 frames and 23.4 blocks of 1024 frames.
        for _ in 0..375 {
            short.block(Duration::from_nanos(666_667), 64);
        }
        for _ in 0..23 {
            long.block(Duration::from_nanos(10_666_667), 1_024);
        }
        long.block(Duration::from_nanos(4_166_667), 400);

        let short = short.shared().status().average_percent;
        let long = long.shared().status().average_percent;
        assert!((short - long).abs() < 0.5, "{short} against {long}");
        assert!(
            (short - 50.0 * (1.0 - (-0.5f32).exp())).abs() < 0.5,
            "{short}"
        );
    }

    #[test]
    fn a_spike_is_the_peak_for_two_to_four_seconds_then_forgotten() {
        let mut meter = LoadMeter::new();
        let load = meter.shared();
        let window_blocks = PEAK_WINDOW_FRAMES / 256;

        blocks(&mut meter, 10, 533);
        blocks(&mut meter, 1, 4_800);
        blocks(&mut meter, 10, 533);
        let status = load.status();
        assert!((status.peak_percent - 90.0).abs() < 0.1, "{status:?}");
        assert!((status.percent - 10.0).abs() < 0.1, "{status:?}");

        // Through the end of the spike's window and all of the next, it is still the peak.
        blocks(&mut meter, 2 * window_blocks - 21, 533);
        assert!((load.status().peak_percent - 90.0).abs() < 0.1);

        // A window later, only the quiet blocks are left.
        blocks(&mut meter, window_blocks + 1, 533);
        assert!(
            (load.status().peak_percent - 10.0).abs() < 0.1,
            "{:?}",
            load.status()
        );
    }

    #[test]
    fn a_block_over_its_period_is_counted_late() {
        let mut meter = LoadMeter::new();
        let load = meter.shared();

        blocks(&mut meter, 3, 5_000);
        blocks(&mut meter, 2, 6_000);
        blocks(&mut meter, 1, 5_300);
        blocks(&mut meter, 1, 12_000);

        let status = load.status();
        assert_eq!(status.late_blocks, 3);
        assert!((status.percent - 225.0).abs() < 0.1, "{status:?}");
        assert!((status.peak_percent - 225.0).abs() < 0.1);
    }

    #[test]
    fn nothing_is_published_before_the_first_block_or_for_an_empty_one() {
        let mut meter = LoadMeter::new();
        let load = meter.shared();
        assert_eq!(load.status(), EngineLoadStatus::default());

        meter.block(Duration::from_millis(50), 0);

        assert_eq!(load.status(), EngineLoadStatus::default());
    }
}
