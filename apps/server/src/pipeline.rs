//! The real-time path from a capture callback to the sinks.
//!
//! ```text
//!  capture callback (driver thread)       engine thread (one)              sinks
//!  convert to f32, decimate to 48 kHz, ─▶ pull whole blocks from the  ─▶  network (phones); later
//!  meter, push into the stream's ring     current ring, number them        the local output and
//!                                                                          the recorder
//! ```
//!
//! The callback never allocates, locks or panics: its decimator, scratch buffer and ring are all
//! allocated when the stream is built, frames the ring has no room for are dropped and counted, and
//! it wakes the engine thread with [Thread::unpark], an atomic store plus a wake call that never
//! waits.
//!
//! Buffering and latency: the ring is a hand-over, not a delay. In normal operation every callback
//! wakes the engine thread, which takes every whole block out at once. When a callback is exactly
//! one block long (an ASIO or other fixed-size driver given the requested buffer, which is how the
//! block size is chosen, see [block_frames]) each callback completes one block and the ring adds
//! only the wake-up time. When the driver delivers other lengths (WASAPI's own period), frames wait
//! for their block to fill: less than one block (5.3 ms at the default 256 frames). The ring's
//! capacity (at least 0.5 s) only comes into play when the engine thread falls behind; it then
//! absorbs the stall instead of losing audio, and what still does not fit is dropped at the
//! callback and counted.
//!
//! Device switches never interleave two devices: every capture stream has its own ring and that
//! ring's single producer (an rtrb `Producer` cannot be cloned), and the engine thread reads exactly
//! one ring at a time, moving to a new stream's ring between two blocks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::decimate::Decimator;
use crate::engine::{LevelMeter, METER_IDLE_FRAMES, METER_IDLE_WAIT};

/// The longest block the engine thread hands the sinks: 8192 frames, 171 ms. A stereo packet of it
/// is 32,788 bytes, well inside one UDP datagram and the phone's receive buffer.
pub(crate) const MAX_BLOCK_FRAMES: usize = 8_192;

/// The block used when the driver runs at its own buffer size: the engine's default request.
pub(crate) const DEFAULT_BLOCK_FRAMES: usize = 256;

/// The smallest capture ring, in frames: half a second at 48 kHz.
const RING_MIN_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 2;

/// The fewest blocks a capture ring holds, so a long block still leaves room for the next callback.
const RING_MIN_BLOCKS: usize = 4;

/// The output frames a callback decimates per pass into its scratch buffer. A longer callback is
/// handled in several passes, so its length never needs a larger buffer.
const SCRATCH_FRAMES: usize = 4_096;

/// A sample format a capture callback hands over, converted to f32 in [-1, 1].
///
/// Integers are scaled by a power of two (2^15 for 16-bit, 2^31 for 32-bit), which is exact: a
/// 16-bit sample converts back to itself, and a 24-bit sample in a 32-bit word (what ASIO drivers
/// deliver) fits the f32 mantissa whole, so nothing below the top 16 bits is lost any more.
pub(crate) trait CaptureSample: Copy + Send + 'static {
    fn to_f32(self) -> f32;
}

impl CaptureSample for f32 {
    /// Out-of-range input is clamped, as the 16-bit path always did; NaN becomes silence.
    fn to_f32(self) -> f32 {
        if self.is_nan() {
            0.0
        } else {
            self.clamp(-1.0, 1.0)
        }
    }
}

impl CaptureSample for i16 {
    fn to_f32(self) -> f32 {
        f32::from(self) * (1.0 / 32_768.0)
    }
}

impl CaptureSample for u16 {
    fn to_f32(self) -> f32 {
        (f32::from(self) - 32_768.0) * (1.0 / 32_768.0)
    }
}

impl CaptureSample for i32 {
    fn to_f32(self) -> f32 {
        self as f32 * (1.0 / 2_147_483_648.0)
    }
}

/// The capture counters the status reports.
#[derive(Default)]
pub(crate) struct CaptureStats {
    /// Samples delivered at 48 kHz (frames times channels), dropped ones included.
    pub(crate) samples: AtomicU64,
    /// Frames a ring had no room for.
    pub(crate) dropped_frames: AtomicU64,
    /// Callback passes dropped whole because their frames did not fit: each is audio the phones
    /// never get, which the status counts with the packets the network failed to send.
    pub(crate) dropped_blocks: AtomicU64,
}

/// The shared state a capture callback writes to: the input meters, the counters and the engine
/// thread to wake.
#[derive(Clone)]
pub(crate) struct CaptureLinks {
    pub(crate) levels: Arc<LevelMeter>,
    pub(crate) stats: Arc<CaptureStats>,
    pub(crate) wake: Option<Thread>,
}

/// The layout of a source's frames: how many channels each frame has and how many frames make the
/// block the engine thread hands the sinks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceFormat {
    pub(crate) channels: usize,
    pub(crate) block_frames: usize,
}

/// The engine block for a capture opened with `buffer_frames` (at the device rate) at `ratio`
/// times 48 kHz: the callback's length after decimation, so a driver that honours the request
/// completes exactly one block per callback. The device default (`None`) uses
/// [DEFAULT_BLOCK_FRAMES]. Always within `1..=`[MAX_BLOCK_FRAMES].
pub(crate) fn block_frames(buffer_frames: Option<u32>, ratio: usize) -> usize {
    let frames = match buffer_frames {
        Some(frames) => frames as usize / ratio.max(1),
        None => DEFAULT_BLOCK_FRAMES,
    };
    frames.clamp(1, MAX_BLOCK_FRAMES)
}

/// The capture callback's half of a stream: everything it needs, allocated up front.
pub(crate) struct CaptureWriter {
    producer: Producer<f32>,
    decimator: Decimator,
    scratch: Box<[f32]>,
    channels: usize,
    links: CaptureLinks,
}

/// The engine thread's half of a stream: the ring it reads and the layout of its frames.
pub(crate) struct SourceReader {
    consumer: Consumer<f32>,
    format: SourceFormat,
}

impl CaptureWriter {
    /// Builds both halves of a capture stream at `source_sample_rate` with `format`'s channels.
    ///
    /// The ring holds the longer of half a second and [RING_MIN_BLOCKS] blocks.
    pub(crate) fn new(
        source_sample_rate: u32,
        format: SourceFormat,
        links: CaptureLinks,
    ) -> Result<(Self, SourceReader), String> {
        let decimator = Decimator::new(source_sample_rate, format.channels)?;
        let ring_frames = RING_MIN_FRAMES.max(RING_MIN_BLOCKS * format.block_frames);
        let (producer, consumer) = RingBuffer::new(ring_frames * format.channels);
        let writer = Self {
            producer,
            decimator,
            scratch: vec![0.0; SCRATCH_FRAMES * format.channels].into_boxed_slice(),
            channels: format.channels,
            links,
        };
        Ok((writer, SourceReader { consumer, format }))
    }

    /// Takes one device buffer: converts it, decimates it to 48 kHz, meters it and hands it to the
    /// engine thread. Called from the audio callback: it never allocates, locks or panics.
    ///
    /// Each pass of up to [SCRATCH_FRAMES] output frames goes into the ring whole or not at all,
    /// so a full ring drops audio in callback-sized pieces and never splits a frame.
    pub(crate) fn write<T: CaptureSample>(&mut self, data: &[T]) {
        let pass = SCRATCH_FRAMES * self.decimator.ratio() * self.channels;
        for input in data.chunks(pass) {
            let frames = self.decimator.process(input, T::to_f32, &mut self.scratch);
            let samples = &self.scratch[..frames * self.channels];
            self.links
                .stats
                .samples
                .fetch_add(samples.len() as u64, Ordering::Relaxed);
            self.links.levels.observe(samples, self.channels);
            if self.producer.push_entire_slice(samples).is_err() {
                self.links
                    .stats
                    .dropped_frames
                    .fetch_add(frames as u64, Ordering::Relaxed);
                self.links
                    .stats
                    .dropped_blocks
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        if let Some(engine) = &self.links.wake {
            engine.unpark();
        }
    }
}

/// One block of capture: `channels`-interleaved f32 frames at 48 kHz, numbered in order.
pub(crate) struct Block<'a> {
    pub(crate) samples: &'a [f32],
    pub(crate) channels: usize,
    /// The block's number: consecutive from the engine's start, across device switches.
    pub(crate) sequence: u64,
}

/// Something the engine thread hands every block to: the phones now, the local output and the
/// recorder later.
///
/// Sinks are given to the engine thread when it starts and run on it, one after the other, for
/// every block. A sink that needs time for slow work (a disk, a second device) hands the block on
/// through its own bounded ring rather than doing it here, and drops what does not fit. After the
/// first blocks a sink must not allocate: [Sink::source_changed] is where buffers are sized.
pub(crate) trait Sink: Send {
    /// One block, in order.
    fn process(&mut self, block: &Block<'_>);

    /// `frames` of audio time passed that no block covered (the capture stalled or stopped): a
    /// sink with meters lets them fall by that much silence.
    fn silence(&mut self, _frames: usize) {}

    /// A new source takes over before its first block, possibly with another layout.
    fn source_changed(&mut self, _format: SourceFormat) {}
}

/// What the engine asks of a running engine thread.
pub(crate) enum PipelineCommand {
    /// Read this stream's ring from the next block on; the previous ring is dropped with any
    /// frames still in it.
    Source(SourceReader),
}

/// The engine thread's state: the ring it reads, the block it fills and the sinks it feeds.
pub(crate) struct Pipeline {
    source: Option<SourceReader>,
    block: Vec<f32>,
    sequence: u64,
    sinks: Vec<Box<dyn Sink>>,
    levels: Arc<LevelMeter>,
    /// When the last block arrived (or the meters last fell for the wait).
    last_audio: Instant,
}

impl Pipeline {
    pub(crate) fn new(sinks: Vec<Box<dyn Sink>>, levels: Arc<LevelMeter>) -> Self {
        Self {
            source: None,
            block: Vec::new(),
            sequence: 0,
            sinks,
            levels,
            last_audio: Instant::now(),
        }
    }

    /// Reads `source` from now on. The block buffer and every sink are sized for its layout here,
    /// off the per-block path.
    pub(crate) fn set_source(&mut self, source: SourceReader) {
        let format = source.format;
        self.block.clear();
        self.block
            .resize(format.block_frames * format.channels, 0.0);
        for sink in &mut self.sinks {
            sink.source_changed(format);
        }
        self.source = Some(source);
    }

    fn apply(&mut self, command: PipelineCommand) {
        match command {
            PipelineCommand::Source(source) => self.set_source(source),
        }
    }

    /// Hands every whole block waiting in the ring to the sinks, in order; returns how many.
    pub(crate) fn pump(&mut self) -> usize {
        let Some(source) = self.source.as_mut() else {
            return 0;
        };
        let channels = source.format.channels;
        let length = self.block.len();
        let mut blocks = 0;
        while length > 0 && source.consumer.slots() >= length {
            if source.consumer.pop_entire_slice(&mut self.block).is_err() {
                break;
            }
            let block = Block {
                samples: &self.block,
                channels,
                sequence: self.sequence,
            };
            for sink in &mut self.sinks {
                sink.process(&block);
            }
            self.sequence = self.sequence.wrapping_add(1);
            blocks += 1;
        }
        if blocks > 0 {
            self.last_audio = Instant::now();
        }
        blocks
    }

    /// Lets every meter fall by [METER_IDLE_FRAMES] when no block came for [METER_IDLE_WAIT]: a
    /// capture that stopped delivering then reads as silence instead of freezing on its last level.
    pub(crate) fn tick(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_audio) >= METER_IDLE_WAIT {
            self.levels.fall(METER_IDLE_FRAMES);
            for sink in &mut self.sinks {
                sink.silence(METER_IDLE_FRAMES);
            }
            self.last_audio = now;
        }
    }

    /// How long the engine thread may sleep before [Self::tick] has something to do.
    fn wait(&self, now: Instant) -> Duration {
        METER_IDLE_WAIT.saturating_sub(now.saturating_duration_since(self.last_audio))
    }
}

/// The running engine thread.
pub(crate) struct PipelineHandle {
    commands: Sender<PipelineCommand>,
    thread: JoinHandle<()>,
}

impl PipelineHandle {
    /// Starts the engine thread on `pipeline`.
    ///
    /// The thread sleeps until a capture callback wakes it (or until its meters are due to fall),
    /// takes every whole block, and stops when the handle is stopped.
    pub(crate) fn spawn(pipeline: Pipeline) -> std::io::Result<Self> {
        let (commands, receiver) = channel();
        let thread = thread::Builder::new()
            .name("engine".to_owned())
            .spawn(move || run(pipeline, receiver))?;
        Ok(Self { commands, thread })
    }

    /// The thread a capture callback wakes after pushing frames.
    pub(crate) fn waker(&self) -> Thread {
        self.thread.thread().clone()
    }

    /// Moves the engine thread to `source` between two blocks.
    pub(crate) fn set_source(&self, source: SourceReader) -> Result<(), String> {
        self.commands
            .send(PipelineCommand::Source(source))
            .map_err(|_| "the engine thread has stopped".to_owned())?;
        self.thread.thread().unpark();
        Ok(())
    }

    /// Stops the engine thread and waits for it.
    pub(crate) fn stop(self) -> Result<(), String> {
        drop(self.commands);
        self.thread.thread().unpark();
        self.thread
            .join()
            .map_err(|_| "the engine thread panicked".to_owned())
    }
}

fn run(mut pipeline: Pipeline, commands: Receiver<PipelineCommand>) {
    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => pipeline.apply(command),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        pipeline.pump();
        let now = Instant::now();
        pipeline.tick(now);
        thread::park_timeout(pipeline.wait(now));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Rust heap allocations made on a watched thread, counted by the test binary's allocator.
    ///
    /// The counter is per thread, so the tests that run in parallel never count each other's
    /// allocations; only the code inside [allocations::count] is watched.
    pub(crate) mod allocations {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        struct CountingAllocator;

        thread_local! {
            static WATCHING: Cell<bool> = const { Cell::new(false) };
            static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
            static FREES: Cell<u64> = const { Cell::new(0) };
        }

        fn note(counter: &'static std::thread::LocalKey<Cell<u64>>) {
            if WATCHING.try_with(Cell::get).unwrap_or(false) {
                let _ = counter.try_with(|count| count.set(count.get() + 1));
            }
        }

        // SAFETY: every call is passed on unchanged to the system allocator.
        unsafe impl GlobalAlloc for CountingAllocator {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                note(&ALLOCATIONS);
                System.alloc(layout)
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                note(&ALLOCATIONS);
                System.alloc_zeroed(layout)
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                note(&ALLOCATIONS);
                System.realloc(ptr, layout, new_size)
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                note(&FREES);
                System.dealloc(ptr, layout)
            }
        }

        #[global_allocator]
        static ALLOCATOR: CountingAllocator = CountingAllocator;

        /// Runs `work` and returns the allocations and frees it made on this thread.
        pub(crate) fn count(work: impl FnOnce()) -> (u64, u64) {
            ALLOCATIONS.with(|count| count.set(0));
            FREES.with(|count| count.set(0));
            WATCHING.with(|watching| watching.set(true));
            work();
            WATCHING.with(|watching| watching.set(false));
            (ALLOCATIONS.with(Cell::get), FREES.with(Cell::get))
        }
    }

    pub(crate) fn links() -> CaptureLinks {
        CaptureLinks {
            levels: Arc::new(LevelMeter::new()),
            stats: Arc::new(CaptureStats::default()),
            wake: None,
        }
    }

    pub(crate) fn source(
        channels: usize,
        block_frames: usize,
        links: CaptureLinks,
    ) -> (CaptureWriter, SourceReader) {
        CaptureWriter::new(
            TARGET_SAMPLE_RATE,
            SourceFormat {
                channels,
                block_frames,
            },
            links,
        )
        .expect("a 48 kHz source builds")
    }

    /// What a sink saw of one block: its number, layout and first sample, and whether every sample
    /// was that one.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct Seen {
        pub(crate) sequence: u64,
        pub(crate) channels: usize,
        pub(crate) frames: usize,
        pub(crate) value: f32,
        pub(crate) uniform: bool,
    }

    /// A sink that records each block's summary into a list fixed in size up front, so it never
    /// allocates on the block path.
    pub(crate) struct Recorder {
        pub(crate) seen: Arc<Mutex<Vec<Seen>>>,
        pub(crate) silence: Arc<AtomicU64>,
    }

    impl Recorder {
        pub(crate) fn new() -> Self {
            Self {
                seen: Arc::new(Mutex::new(Vec::with_capacity(4_096))),
                silence: Arc::new(AtomicU64::new(0)),
            }
        }
    }

    impl Sink for Recorder {
        fn process(&mut self, block: &Block<'_>) {
            let value = block.samples.first().copied().unwrap_or(0.0);
            let mut seen = self.seen.lock().unwrap();
            if seen.len() < seen.capacity() {
                seen.push(Seen {
                    sequence: block.sequence,
                    channels: block.channels,
                    frames: block.samples.len() / block.channels,
                    value,
                    uniform: block.samples.iter().all(|sample| *sample == value),
                });
            }
        }

        fn silence(&mut self, frames: usize) {
            self.silence.fetch_add(frames as u64, Ordering::Relaxed);
        }
    }

    #[test]
    fn integer_samples_convert_exactly_and_keep_24_bit_resolution() {
        assert_eq!(i16::MIN.to_f32(), -1.0);
        assert_eq!(16_384i16.to_f32(), 0.5);
        assert_eq!(i16::MAX.to_f32(), 32_767.0 / 32_768.0);
        assert_eq!(0u16.to_f32(), -1.0);
        assert_eq!(32_768u16.to_f32(), 0.0);
        assert_eq!(i32::MIN.to_f32(), -1.0);
        // The largest and the smallest step of 24-bit audio, left-justified in a 32-bit word.
        assert_eq!((8_388_607i32 << 8).to_f32(), 8_388_607.0 / 8_388_608.0);
        assert_eq!((1i32 << 8).to_f32(), 1.0 / 8_388_608.0);
        // The 16-bit path kept only the top half: these two differed by nothing there.
        assert_ne!((1i32 << 8).to_f32(), 0.0);
    }

    #[test]
    fn float_samples_are_clamped_and_nan_is_silence() {
        assert_eq!(0.25f32.to_f32(), 0.25);
        assert_eq!(1.5f32.to_f32(), 1.0);
        assert_eq!((-3.0f32).to_f32(), -1.0);
        assert_eq!(f32::NAN.to_f32(), 0.0);
    }

    #[test]
    fn the_block_is_the_requested_callback_at_48_khz() {
        assert_eq!(block_frames(Some(256), 1), 256);
        assert_eq!(block_frames(Some(256), 2), 128);
        assert_eq!(block_frames(Some(1_024), 4), 256);
        assert_eq!(block_frames(None, 1), DEFAULT_BLOCK_FRAMES);
        assert_eq!(block_frames(Some(48_000), 1), MAX_BLOCK_FRAMES);
        assert_eq!(block_frames(Some(1), 4), 1);
    }

    #[test]
    fn the_ring_holds_half_a_second_or_four_blocks() {
        let (writer, _reader) = source(2, 256, links());
        assert_eq!(writer.producer.buffer().capacity(), 24_000 * 2);

        let (writer, _reader) = source(3, 8_192, links());
        assert_eq!(writer.producer.buffer().capacity(), 4 * 8_192 * 3);
    }

    #[test]
    fn a_callback_reaches_the_sinks_as_numbered_blocks_of_float_frames() {
        let links = links();
        let (mut writer, reader) = source(2, 4, links.clone());
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], Arc::clone(&links.levels));
        pipeline.set_source(reader);

        // Ten frames of 16-bit stereo at half scale: two whole blocks and two frames left over.
        writer.write(&[16_384i16; 20]);
        assert_eq!(pipeline.pump(), 2);
        // Two more frames complete the third block.
        writer.write(&[16_384i16; 4]);
        assert_eq!(pipeline.pump(), 1);

        let seen = seen.lock().unwrap();
        assert_eq!(
            seen.iter().map(|block| block.sequence).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(seen
            .iter()
            .all(|block| block.channels == 2 && block.frames == 4 && block.value == 0.5));
        assert_eq!(links.stats.samples.load(Ordering::Relaxed), 24);
        assert_eq!(links.levels.read(2), vec![50, 50]);
    }

    #[test]
    fn a_96_khz_callback_is_decimated_before_it_is_metered_and_queued() {
        let links = links();
        let (mut writer, reader) = CaptureWriter::new(
            96_000,
            SourceFormat {
                channels: 1,
                block_frames: 128,
            },
            links.clone(),
        )
        .unwrap();
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], Arc::clone(&links.levels));
        pipeline.set_source(reader);

        writer.write(&[0.5f32; 256]);

        assert_eq!(pipeline.pump(), 1);
        assert_eq!(seen.lock().unwrap()[0].frames, 128);
        assert_eq!(links.stats.samples.load(Ordering::Relaxed), 128);
    }

    #[test]
    fn a_full_ring_drops_whole_callbacks_and_counts_them() {
        let links = links();
        let (mut writer, reader) = source(1, 1_000, links.clone());

        // 30 callbacks of 1000 frames into a 24,000-frame ring nobody reads.
        for _ in 0..30 {
            writer.write(&[0.1f32; 1_000]);
        }

        assert_eq!(links.stats.dropped_frames.load(Ordering::Relaxed), 6_000);
        assert_eq!(links.stats.dropped_blocks.load(Ordering::Relaxed), 6);
        assert_eq!(links.stats.samples.load(Ordering::Relaxed), 30_000);
        assert_eq!(reader.consumer.slots(), 24_000);
    }

    #[test]
    fn the_numbering_continues_across_a_source_switch() {
        let levels = Arc::new(LevelMeter::new());
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], Arc::clone(&levels));

        let (mut first, reader) = source(2, 2, links());
        pipeline.set_source(reader);
        first.write(&[0.25f32; 8]);
        pipeline.pump();
        let (mut second, reader) = source(1, 2, links());
        pipeline.set_source(reader);
        second.write(&[-0.5f32; 4]);
        pipeline.pump();

        let seen = seen.lock().unwrap();
        assert_eq!(
            seen.iter()
                .map(|block| (block.sequence, block.channels, block.value))
                .collect::<Vec<_>>(),
            [(0, 2, 0.25), (1, 2, 0.25), (2, 1, -0.5), (3, 1, -0.5)]
        );
    }

    /// Two capture streams run at once during a switch, as they do while the new device starts
    /// and the old one is being closed. Each has its own ring, and the engine thread reads one at
    /// a time, so the sinks see the old device's blocks, then only the new device's, and never a
    /// block that holds both.
    #[test]
    fn a_switch_never_interleaves_two_devices() {
        let levels = Arc::new(LevelMeter::new());
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let handle = PipelineHandle::spawn(Pipeline::new(vec![Box::new(recorder)], levels))
            .expect("the engine thread starts");
        let wake = CaptureLinks {
            wake: Some(handle.waker()),
            ..links()
        };
        let (old, old_reader) = source(2, 64, wake.clone());
        let (new, new_reader) = source(2, 64, wake);
        handle.set_source(old_reader).unwrap();

        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let produce = |mut writer: CaptureWriter, value: f32| {
            let running = Arc::clone(&running);
            thread::spawn(move || {
                let buffer = [value; 2 * 48];
                while running.load(Ordering::Relaxed) {
                    writer.write(&buffer);
                    thread::sleep(Duration::from_micros(500));
                }
            })
        };
        let old_device = produce(old, 0.25);
        let new_device = produce(new, -0.5);
        thread::sleep(Duration::from_millis(60));
        handle.set_source(new_reader).unwrap();
        thread::sleep(Duration::from_millis(60));
        running.store(false, Ordering::Relaxed);
        old_device.join().unwrap();
        new_device.join().unwrap();
        handle.stop().unwrap();

        let seen = seen.lock().unwrap();
        let values: Vec<f32> = seen.iter().map(|block| block.value).collect();
        assert!(
            seen.iter().all(|block| block.uniform),
            "a block mixed two devices"
        );
        let switch = values
            .iter()
            .position(|value| *value == -0.5)
            .expect("the new device was heard");
        assert!(switch > 0, "the old device was heard first");
        assert!(values[..switch].iter().all(|value| *value == 0.25));
        assert!(
            values[switch..].iter().all(|value| *value == -0.5),
            "the old device came back after the switch: {values:?}"
        );
        assert!(seen
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1));
    }

    #[test]
    fn the_capture_callback_and_the_block_path_never_allocate() {
        let links = links();
        let (mut writer, reader) = CaptureWriter::new(
            96_000,
            SourceFormat {
                channels: 2,
                block_frames: 128,
            },
            links.clone(),
        )
        .unwrap();
        let recorder = Recorder::new();
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], links.levels);
        pipeline.set_source(reader);
        // 24-bit audio in 32-bit words at 96 kHz: the costliest conversion path.
        let callback: Vec<i32> = (0..2 * 256).map(|index| (index * 4_096) << 8).collect();
        writer.write(&callback);
        pipeline.pump();

        let (allocations, frees) = allocations::count(|| {
            for _ in 0..2_000 {
                writer.write(&callback);
                pipeline.pump();
                pipeline.tick(Instant::now());
            }
        });

        assert_eq!((allocations, frees), (0, 0));
    }

    #[test]
    fn a_stopped_engine_thread_ends_promptly() {
        let handle = PipelineHandle::spawn(Pipeline::new(Vec::new(), Arc::new(LevelMeter::new())))
            .expect("the engine thread starts");
        let started = Instant::now();

        handle.stop().unwrap();

        assert!(started.elapsed() < Duration::from_millis(50));
    }
}
