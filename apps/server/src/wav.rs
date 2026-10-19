//! WAV and RF64 files for recording takes and playing them back.
//!
//! # Layout
//!
//! [WavWriter] always writes the same 104-byte header, then the audio:
//!
//! | offset | bytes | content                                                              |
//! |--------|-------|----------------------------------------------------------------------|
//! | 0      | 12    | `RIFF`, the size of the rest of the file, `WAVE`                     |
//! | 12     | 36    | `JUNK` with 28 zero bytes: the room a `ds64` chunk needs              |
//! | 48     | 48    | `fmt ` with 40 bytes of WAVE_FORMAT_EXTENSIBLE                        |
//! | 96     | 8     | `data` and the size of the audio                                     |
//! | 104    |       | interleaved little-endian samples, and a pad byte if their size is odd |
//!
//! RIFF counts sizes in 32 bits, so a WAV file ends at 4 GiB: about 8.3 hours of 24-bit mono at
//! 48 kHz, 3.1 hours of float stereo. Past that the header takes the RF64 form (EBU Tech 3306): the
//! `RIFF` id becomes `RF64` with a size of 0xFFFFFFFF, the `JUNK` chunk becomes a `ds64` chunk of
//! the same size holding the 64-bit RIFF size, data size and sample count (frames per channel), and
//! the data size becomes 0xFFFFFFFF. Nothing moves: that is why the `JUNK` chunk is reserved first,
//! where RF64 wants `ds64`, the first chunk after `WAVE`. Readers that do not know RF64 skip `JUNK`,
//! so a file under 4 GiB is a plain WAV file everywhere.
//!
//! # Format chunk and samples
//!
//! Both recording formats use WAVE_FORMAT_EXTENSIBLE, which Microsoft asks for above 16 bits:
//! 24-bit integer PCM with 24 valid bits (subformat KSDATAFORMAT_SUBTYPE_PCM) and 32-bit IEEE float
//! (KSDATAFORMAT_SUBTYPE_IEEE_FLOAT). The channel mask is front centre for mono, front left and
//! right for stereo, and no positions for other counts. No `fact` chunk is written: it matters only
//! for compressed formats, and readers of uncompressed float take the length from the data size.
//!
//! Integer samples are scaled by 2^23 (8,388,608), the power of two capture uses, so a 24-bit
//! capture sample is written back bit for bit. -1.0 is -8,388,608, the lowest code; +1.0 would be
//! +8,388,608, which 24 bits cannot hold, so it clamps to +8,388,607 like everything past full
//! scale; other values round to the nearest step (within half a step) and NaN is written as
//! silence. Float samples are written as they are.
//!
//! # Crash safety
//!
//! The header is written when the file is created, with a data size of 0, and rewritten in place
//! by [WavWriter::refresh], [WavWriter::sync] and [WavWriter::finish]. The audio is handed to the
//! file before the sizes that count it, so a header never covers audio the file was not given.
//! After a refresh the header covers every frame written up to it, so a process that dies leaves a
//! valid file that plays up to the last refresh; [WavWriter::sync] also asks the system to put the
//! file on the disk, so that point survives a system crash or power loss too, as far as the drive
//! honours it. A refresh past 4 GiB writes the RF64 form, so long takes are covered the same way.
//!
//! The limits: audio written since the last refresh is not in the header (the recorder bounds that
//! loss by refreshing periodically); without [WavWriter::sync] a system crash loses what the system
//! had not written yet; the pad byte after odd-sized audio is only written by [WavWriter::finish].
//! After a failed write the writer stops: every later call returns [WavError::Stopped] and the
//! header is never touched again, so the file keeps the header of its last good refresh.
//!
//! [WavReader] recovers files that never got a good header: when the data size is 0 (the writer
//! died before its first refresh), missing (an RF64 file without its `ds64`) or larger than the
//! file (the header reached the disk before the audio), it reads the whole frames actually there.
//! A data size of 0 is taken for an unfinished file only when the file is laid out as this writer
//! lays it out, or when the bytes after the data chunk's header cannot begin a chunk: another
//! writer's empty data chunk followed by `LIST` or `bext` stays empty. The reader cannot see
//! audio past a smaller data size either, because bytes after the data chunk may be other chunks.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

const RIFF: [u8; 4] = *b"RIFF";
const RF64: [u8; 4] = *b"RF64";
const WAVE: [u8; 4] = *b"WAVE";
const JUNK: [u8; 4] = *b"JUNK";
const DS64: [u8; 4] = *b"ds64";
const FMT: [u8; 4] = *b"fmt ";
const DATA: [u8; 4] = *b"data";

const FORMAT_PCM: u16 = 0x0001;
const FORMAT_IEEE_FLOAT: u16 = 0x0003;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// The 14 bytes after the format code in a KSDATAFORMAT_SUBTYPE GUID made from a WAVE format code,
/// {0000xxxx-0000-0010-8000-00AA00389B71}, as they sit in a file.
const SUBFORMAT_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// What WAVE_FORMAT_EXTENSIBLE adds to the 16-byte fmt: valid bits, channel mask and subformat.
const EXTENSION_SIZE: u16 = 22;

/// The fmt payload the writer writes: 16 bytes, the extension's size and the extension.
const FMT_SIZE: u32 = 16 + 2 + EXTENSION_SIZE as u32;

const SPEAKER_FRONT_LEFT: u32 = 0x1;
const SPEAKER_FRONT_RIGHT: u32 = 0x2;
const SPEAKER_FRONT_CENTER: u32 = 0x4;

/// The ds64 payload: the 64-bit RIFF size, data size and sample count, and a table length of 0.
const DS64_SIZE: u32 = 28;

/// Where the writer's header keeps each field it rewrites.
const RIFF_SIZE_OFFSET: u64 = 4;
const JUNK_OFFSET: u64 = 12;
const FMT_OFFSET: u64 = JUNK_OFFSET + 8 + DS64_SIZE as u64;
const DATA_SIZE_OFFSET: u64 = FMT_OFFSET + 8 + FMT_SIZE as u64 + 4;
const HEADER_LEN: u64 = DATA_SIZE_OFFSET + 4;

/// The bytes the writer's header takes before the audio: what a file holds beyond its frames.
pub(crate) const HEADER_BYTES: u64 = HEADER_LEN;

/// The largest size a 32-bit RIFF field holds: in RF64, 0xFFFFFFFF means "see ds64".
const RIFF_SIZE_LIMIT: u64 = u32::MAX as u64 - 1;

/// Samples encoded per write, so the scratch buffer stays small whatever a call hands over.
const ENCODE_SAMPLES: usize = 4_096;

/// 2^23: one 24-bit step is `1 / INT24_SCALE`.
const INT24_SCALE: f32 = 8_388_608.0;

/// How a file stores its samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// 16-bit integer PCM. Read only.
    Int16,
    /// 24-bit integer PCM, the recording default.
    Int24,
    /// 32-bit integer PCM. Read only.
    Int32,
    /// 32-bit IEEE float.
    Float32,
}

impl SampleFormat {
    /// The bytes one sample takes.
    pub fn bytes_per_sample(self) -> u16 {
        match self {
            SampleFormat::Int16 => 2,
            SampleFormat::Int24 => 3,
            SampleFormat::Int32 | SampleFormat::Float32 => 4,
        }
    }
}

/// What a WAV file holds: its channels, sample rate and sample format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavSpec {
    pub channels: u16,
    pub sample_rate: u32,
    pub format: SampleFormat,
}

impl WavSpec {
    /// The bytes one frame (one sample per channel) takes.
    pub fn bytes_per_frame(&self) -> u32 {
        u32::from(self.channels) * u32::from(self.format.bytes_per_sample())
    }

    /// The bytes one second of audio takes.
    pub fn bytes_per_second(&self) -> u64 {
        u64::from(self.sample_rate) * u64::from(self.bytes_per_frame())
    }
}

/// Why a WAV file could not be written or read.
#[derive(Debug)]
pub enum WavError {
    /// Reading or writing the file failed.
    Io(io::Error),
    /// The bytes are not a WAV file this module can make sense of.
    Malformed(String),
    /// A well-formed WAV file in an encoding this module does not decode.
    Unsupported(String),
    /// A request the writer cannot honour: a spec it does not record, or a partial frame.
    InvalidInput(String),
    /// An earlier write failed, so the writer stopped; the file keeps the header of its last good
    /// refresh.
    Stopped,
}

impl fmt::Display for WavError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WavError::Io(error) => write!(formatter, "{error}"),
            WavError::Malformed(reason) => write!(formatter, "not a readable WAV file: {reason}"),
            WavError::Unsupported(reason) => write!(formatter, "unsupported WAV encoding: {reason}"),
            WavError::InvalidInput(reason) => write!(formatter, "{reason}"),
            WavError::Stopped => write!(
                formatter,
                "the WAV writer stopped after a failed write; the file keeps the header of its last refresh"
            ),
        }
    }
}

impl std::error::Error for WavError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WavError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for WavError {
    fn from(error: io::Error) -> Self {
        WavError::Io(error)
    }
}

/// Where a [WavWriter] puts the file: a [File], or memory.
pub trait WavTarget: Write + Seek {
    /// Asks the system to put everything written so far on the disk itself.
    fn sync(&mut self) -> io::Result<()>;
}

impl WavTarget for File {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_data()
    }
}

impl WavTarget for Cursor<Vec<u8>> {
    /// Memory has no disk to reach.
    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The two encodings the writer records.
#[derive(Clone, Copy)]
enum Encoding {
    Int24,
    Float32,
}

/// Writes interleaved f32 frames to a WAV file, turning it into RF64 past 4 GiB.
pub struct WavWriter<W: WavTarget = File> {
    target: BufWriter<W>,
    spec: WavSpec,
    encoding: Encoding,
    /// Audio bytes handed to `target` so far.
    data_bytes: u64,
    /// The RIFF size above which the header takes the RF64 form.
    size_limit: u64,
    /// Encoded samples on their way to `target`, reused between calls.
    scratch: Vec<u8>,
    /// Set by the first failed write; from then on nothing touches the file.
    stopped: bool,
}

impl WavWriter<File> {
    /// Creates a new file at `path` (never overwriting one) and writes its header.
    pub fn create(path: impl AsRef<Path>, spec: WavSpec) -> Result<Self, WavError> {
        // Checked first, so a refused spec leaves no file behind.
        recording_layout(spec)?;
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Self::new(file, spec)
    }
}

impl<W: WavTarget> WavWriter<W> {
    /// Starts a WAV file at the beginning of `target`, which should be empty.
    pub fn new(target: W, spec: WavSpec) -> Result<Self, WavError> {
        Self::with_size_limit(target, spec, RIFF_SIZE_LIMIT)
    }

    /// [WavWriter::new] with the RIFF size past which the file becomes RF64 (at most the 32-bit
    /// limit), so tests can reach it without writing 4 GiB.
    pub(crate) fn with_size_limit(
        target: W,
        spec: WavSpec,
        size_limit: u64,
    ) -> Result<Self, WavError> {
        let (encoding, block_align, bytes_per_second) = recording_layout(spec)?;
        let mut target = BufWriter::new(target);
        target.seek(SeekFrom::Start(0))?;
        target.write_all(&header(spec, encoding, block_align, bytes_per_second))?;
        target.flush()?;
        Ok(Self {
            target,
            spec,
            encoding,
            data_bytes: 0,
            size_limit: size_limit.min(RIFF_SIZE_LIMIT),
            scratch: Vec::with_capacity(ENCODE_SAMPLES * 4),
            stopped: false,
        })
    }

    /// The spec the file is written with.
    pub fn spec(&self) -> WavSpec {
        self.spec
    }

    /// The frames written so far.
    pub fn frames(&self) -> u64 {
        self.data_bytes / u64::from(self.spec.bytes_per_frame())
    }

    /// Appends interleaved frames: `samples.len()` must be a whole number of frames.
    pub fn write_frames(&mut self, samples: &[f32]) -> Result<(), WavError> {
        self.check_running()?;
        let channels = usize::from(self.spec.channels);
        if !samples.len().is_multiple_of(channels) {
            return Err(WavError::InvalidInput(format!(
                "{} samples are not whole frames of {channels} channels",
                samples.len()
            )));
        }
        for chunk in samples.chunks(ENCODE_SAMPLES) {
            self.scratch.clear();
            match self.encoding {
                Encoding::Int24 => {
                    for sample in chunk {
                        self.scratch.extend_from_slice(&int24_bytes(*sample));
                    }
                }
                Encoding::Float32 => {
                    for sample in chunk {
                        self.scratch.extend_from_slice(&sample.to_le_bytes());
                    }
                }
            }
            if let Err(error) = self.target.write_all(&self.scratch) {
                self.stopped = true;
                return Err(error.into());
            }
            self.data_bytes += self.scratch.len() as u64;
        }
        Ok(())
    }

    /// Rewrites the header's sizes in place to cover every frame written so far and hands it all
    /// to the file. The next frames go on after the last one.
    pub fn refresh(&mut self) -> Result<(), WavError> {
        self.check_running()?;
        let result = self.write_sizes(false);
        self.stop_on_error(result)
    }

    /// [WavWriter::refresh], then asks the system to put the file on the disk.
    pub fn sync(&mut self) -> Result<(), WavError> {
        self.check_running()?;
        let result = self
            .write_sizes(false)
            .and_then(|()| self.target.get_mut().sync().map_err(WavError::from));
        self.stop_on_error(result)
    }

    /// Writes the pad byte and the final sizes (in RF64 form past the size limit), puts the file
    /// on the disk and returns the target.
    pub fn finish(mut self) -> Result<W, WavError> {
        self.check_running()?;
        if self.data_bytes % 2 == 1 {
            // Chunks start on even offsets, so odd-sized audio is followed by a pad byte.
            self.target.write_all(&[0])?;
        }
        self.write_sizes(true)?;
        self.target.get_mut().sync()?;
        self.target
            .into_inner()
            .map_err(|error| WavError::Io(error.into_error()))
    }

    /// Rewrites the sizes for the audio handed over so far (and the pad byte when `padded`), in
    /// RIFF form up to the size limit and RF64 form past it, then puts the write position back at
    /// the end and hands everything to the target.
    fn write_sizes(&mut self, padded: bool) -> Result<(), WavError> {
        // The audio reaches the target before a header that counts it.
        self.target.flush()?;
        let file_end = HEADER_LEN + self.data_bytes + u64::from(padded && self.data_bytes % 2 == 1);
        let riff_size = file_end - 8;
        if riff_size <= self.size_limit {
            // Both fit 32 bits: the limit is at most RIFF_SIZE_LIMIT and the audio is inside the
            // RIFF size.
            self.put(RIFF_SIZE_OFFSET, &(riff_size as u32).to_le_bytes())?;
            self.put(DATA_SIZE_OFFSET, &(self.data_bytes as u32).to_le_bytes())?;
        } else {
            let frames = self.data_bytes / u64::from(self.spec.bytes_per_frame());
            let mut ds64 = [0; 8 + DS64_SIZE as usize];
            ds64[..4].copy_from_slice(&DS64);
            ds64[4..8].copy_from_slice(&DS64_SIZE.to_le_bytes());
            ds64[8..16].copy_from_slice(&riff_size.to_le_bytes());
            ds64[16..24].copy_from_slice(&self.data_bytes.to_le_bytes());
            ds64[24..32].copy_from_slice(&frames.to_le_bytes());
            // Bytes 32..36 stay 0: no other chunk needs a 64-bit size.
            let mut form = [0xFF; 8];
            form[..4].copy_from_slice(&RF64);
            // The sizes before the RF64 id: a file cut short in between still has a data chunk
            // whose size readers recover from (0xFFFFFFFF is larger than the file).
            self.put(JUNK_OFFSET, &ds64)?;
            self.put(DATA_SIZE_OFFSET, &u32::MAX.to_le_bytes())?;
            self.put(0, &form)?;
        }
        self.target.seek(SeekFrom::Start(file_end))?;
        self.target.flush()?;
        Ok(())
    }

    fn put(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        self.target.seek(SeekFrom::Start(offset))?;
        self.target.write_all(bytes)
    }

    fn check_running(&self) -> Result<(), WavError> {
        if self.stopped {
            Err(WavError::Stopped)
        } else {
            Ok(())
        }
    }

    fn stop_on_error(&mut self, result: Result<(), WavError>) -> Result<(), WavError> {
        if result.is_err() {
            self.stopped = true;
        }
        result
    }
}

/// How the writer records `spec`, with its block align and byte rate, or why it cannot.
fn recording_layout(spec: WavSpec) -> Result<(Encoding, u16, u32), WavError> {
    let encoding = match spec.format {
        SampleFormat::Int24 => Encoding::Int24,
        SampleFormat::Float32 => Encoding::Float32,
        SampleFormat::Int16 | SampleFormat::Int32 => {
            return Err(WavError::InvalidInput(format!(
                "recordings are 24-bit integer or 32-bit float, not {:?}",
                spec.format
            )));
        }
    };
    if spec.channels == 0 {
        return Err(WavError::InvalidInput(
            "a WAV file needs at least one channel".to_owned(),
        ));
    }
    if spec.sample_rate == 0 {
        return Err(WavError::InvalidInput(
            "a WAV file needs a sample rate above 0 Hz".to_owned(),
        ));
    }
    let block_align = u16::try_from(spec.bytes_per_frame()).map_err(|_| {
        WavError::InvalidInput(format!("{} channels do not fit a WAV frame", spec.channels))
    })?;
    let bytes_per_second = u32::try_from(spec.bytes_per_second()).map_err(|_| {
        WavError::InvalidInput(format!(
            "{} channels at {} Hz do not fit a WAV header",
            spec.channels, spec.sample_rate
        ))
    })?;
    Ok((encoding, block_align, bytes_per_second))
}

/// The header of a file with no audio yet: see the layout in the module documentation.
fn header(spec: WavSpec, encoding: Encoding, block_align: u16, bytes_per_second: u32) -> Vec<u8> {
    let bits = spec.format.bytes_per_sample() * 8;
    let subformat = match encoding {
        Encoding::Int24 => FORMAT_PCM,
        Encoding::Float32 => FORMAT_IEEE_FLOAT,
    };
    let channel_mask = match spec.channels {
        1 => SPEAKER_FRONT_CENTER,
        2 => SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT,
        _ => 0,
    };
    let mut header = Vec::with_capacity(HEADER_LEN as usize);
    header.extend_from_slice(&RIFF);
    header.extend_from_slice(&((HEADER_LEN - 8) as u32).to_le_bytes());
    header.extend_from_slice(&WAVE);
    header.extend_from_slice(&JUNK);
    header.extend_from_slice(&DS64_SIZE.to_le_bytes());
    header.extend_from_slice(&[0; DS64_SIZE as usize]);
    header.extend_from_slice(&FMT);
    header.extend_from_slice(&FMT_SIZE.to_le_bytes());
    header.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
    header.extend_from_slice(&spec.channels.to_le_bytes());
    header.extend_from_slice(&spec.sample_rate.to_le_bytes());
    header.extend_from_slice(&bytes_per_second.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits.to_le_bytes());
    header.extend_from_slice(&EXTENSION_SIZE.to_le_bytes());
    // Every bit of the container is valid.
    header.extend_from_slice(&bits.to_le_bytes());
    header.extend_from_slice(&channel_mask.to_le_bytes());
    header.extend_from_slice(&subformat.to_le_bytes());
    header.extend_from_slice(&SUBFORMAT_TAIL);
    header.extend_from_slice(&DATA);
    header.extend_from_slice(&0u32.to_le_bytes());
    header
}

/// A sample as the three little-endian bytes of a 24-bit integer: scaled by 2^23 and rounded to
/// the nearest step, clamped to the codes 24 bits hold, NaN as silence.
fn int24_bytes(sample: f32) -> [u8; 3] {
    let code = if sample.is_nan() {
        0
    } else {
        (sample * INT24_SCALE)
            .round()
            .clamp(-INT24_SCALE, INT24_SCALE - 1.0) as i32
    };
    let bytes = code.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

/// Reads interleaved f32 frames from a RIFF or RF64 WAV file.
pub struct WavReader<R: Read + Seek = BufReader<File>> {
    source: R,
    spec: WavSpec,
    /// Where the first frame starts in `source`.
    data_start: u64,
    /// The whole frames the file holds.
    frames: u64,
    /// The next frame [WavReader::read_frames] returns.
    position: u64,
    /// Whether `source` sits at `position`; false after a failed read.
    positioned: bool,
    /// Raw bytes on their way to the caller's buffer, reused between calls.
    scratch: Vec<u8>,
}

impl WavReader<BufReader<File>> {
    /// Opens the WAV file at `path` and reads its header.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WavError> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> WavReader<R> {
    /// Reads the header of the WAV file `source` holds.
    pub fn new(mut source: R) -> Result<Self, WavError> {
        let layout = read_layout(&mut source)?;
        source.seek(SeekFrom::Start(layout.data_start))?;
        Ok(Self {
            source,
            spec: layout.spec,
            data_start: layout.data_start,
            frames: layout.frames,
            position: 0,
            positioned: true,
            scratch: Vec::new(),
        })
    }

    /// The file's channels, sample rate and sample format.
    pub fn spec(&self) -> WavSpec {
        self.spec
    }

    /// The whole frames the file holds.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The next frame [WavReader::read_frames] returns.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Reads as many whole frames as `buffer` holds, interleaved, and returns how many: 0 at the
    /// end of the file, or when `buffer` is shorter than one frame. Float samples come as they
    /// are stored, so they may lie past ±1 or be NaN.
    pub fn read_frames(&mut self, buffer: &mut [f32]) -> Result<usize, WavError> {
        let channels = usize::from(self.spec.channels);
        let room = buffer.len() / channels;
        // At most `room`, so it fits a usize.
        let frames = (room as u64).min(self.frames - self.position) as usize;
        if frames == 0 {
            return Ok(0);
        }
        if !self.positioned {
            self.source
                .seek(SeekFrom::Start(self.offset_of(self.position)))?;
            self.positioned = true;
        }
        let samples = frames * channels;
        let bytes = samples * usize::from(self.spec.format.bytes_per_sample());
        if self.scratch.len() < bytes {
            self.scratch.resize(bytes, 0);
        }
        if let Err(error) = self.source.read_exact(&mut self.scratch[..bytes]) {
            self.positioned = false;
            return Err(error.into());
        }
        decode(
            self.spec.format,
            &self.scratch[..bytes],
            &mut buffer[..samples],
        );
        self.position += frames as u64;
        Ok(frames)
    }

    /// Moves to `frame`, or to the end when the file is shorter.
    pub fn seek(&mut self, frame: u64) -> Result<(), WavError> {
        let frame = frame.min(self.frames);
        self.positioned = false;
        self.source.seek(SeekFrom::Start(self.offset_of(frame)))?;
        self.position = frame;
        self.positioned = true;
        Ok(())
    }

    fn offset_of(&self, frame: u64) -> u64 {
        self.data_start + frame * u64::from(self.spec.bytes_per_frame())
    }
}

/// Where a file's audio is and what it holds.
struct Layout {
    spec: WavSpec,
    data_start: u64,
    frames: u64,
}

/// Reads the header of the WAV file in `source`: walks the chunks (skipping unknown ones and their
/// pad bytes) for `fmt ` and `data`, takes the data size from `ds64` in an RF64 file, and counts
/// the whole frames the audio holds, recovering a size that is 0 (in an unfinished file, see the
/// module documentation), missing or past the end.
fn read_layout<R: Read + Seek>(source: &mut R) -> Result<Layout, WavError> {
    let file_length = source.seek(SeekFrom::End(0))?;
    source.seek(SeekFrom::Start(0))?;
    let riff: [u8; 12] = read_array(source, "the RIFF header")?;
    let rf64 = match [riff[0], riff[1], riff[2], riff[3]] {
        RIFF => false,
        RF64 => true,
        _ => {
            return Err(WavError::Malformed(
                "it starts with neither RIFF nor RF64".to_owned(),
            ))
        }
    };
    if riff[8..] != WAVE {
        return Err(WavError::Malformed(
            "a RIFF file that is not WAVE".to_owned(),
        ));
    }

    let mut ds64_data_size = None;
    let mut spec = None;
    // Where the audio starts and the size its header gives, if any.
    let mut audio: Option<(u64, Option<u64>)> = None;
    // Whether the chunks so far sit where WavWriter puts them (see the layout in the module
    // documentation).
    let mut writer_layout = false;
    let mut offset = 12u64;
    while spec.is_none() || audio.is_none() {
        let Some(body) = offset.checked_add(8).filter(|body| *body <= file_length) else {
            break;
        };
        source.seek(SeekFrom::Start(offset))?;
        let chunk: [u8; 8] = read_array(source, "a chunk header")?;
        let id = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        if offset == JUNK_OFFSET {
            writer_layout = (id == JUNK || id == DS64) && size == DS64_SIZE;
        }
        // In RF64 a size of 0xFFFFFFFF says the real size is in ds64.
        let declared = (!rf64 || size != u32::MAX).then_some(u64::from(size));
        let skip = match id {
            DS64 if rf64 && offset == 12 => {
                ds64_data_size = Some(read_ds64(source, size)?);
                declared
            }
            FMT if spec.is_none() => {
                writer_layout &= offset == FMT_OFFSET && size == FMT_SIZE;
                spec = Some(read_format(source, size)?);
                declared
            }
            DATA if audio.is_none() => {
                let length = declared.or(ds64_data_size);
                audio = Some((body, length));
                if spec.is_some() {
                    break;
                }
                // The fmt chunk may follow the audio: look past it only when its size holds.
                let trusted = length.filter(|length| {
                    *length > 0
                        && body
                            .checked_add(*length)
                            .is_some_and(|end| end <= file_length)
                });
                if trusted.is_none() {
                    return Err(WavError::Malformed(
                        "audio of unknown length comes before the fmt chunk".to_owned(),
                    ));
                }
                trusted
            }
            _ => declared,
        };
        let Some(skip) = skip else {
            return Err(WavError::Unsupported(
                "an RF64 chunk other than data with a 64-bit size".to_owned(),
            ));
        };
        offset = body.saturating_add(skip).saturating_add(skip & 1);
    }

    let spec = spec.ok_or_else(|| WavError::Malformed("there is no fmt chunk".to_owned()))?;
    let (data_start, length) =
        audio.ok_or_else(|| WavError::Malformed("there is no data chunk".to_owned()))?;
    // The data chunk's header was read whole, so its audio starts inside the file.
    let present = file_length - data_start;
    // A data size of 0 is a writer that died before its first refresh only in a file laid out as
    // ours, or when what follows cannot begin a chunk; otherwise the data chunk is really empty
    // and what follows are other chunks (LIST, bext), not audio.
    let ours = writer_layout && data_start == HEADER_LEN;
    let empty = length == Some(0) && !ours && begins_chunk(source, data_start, file_length)?;
    let length = match length {
        Some(length) if length > 0 && length <= present => length,
        Some(_) if empty => 0,
        // 0 (no refresh yet), missing (RF64 without ds64) or past the end (a crash): what is there.
        _ => present,
    };
    Ok(Layout {
        spec,
        data_start,
        frames: length / u64::from(spec.bytes_per_frame()),
    })
}

/// Whether the bytes at `offset` can begin a chunk: an id of four printable ASCII characters and
/// a size that fits in the file.
fn begins_chunk<R: Read + Seek>(
    source: &mut R,
    offset: u64,
    file_length: u64,
) -> Result<bool, WavError> {
    let Some(body) = offset.checked_add(8).filter(|body| *body <= file_length) else {
        return Ok(false);
    };
    source.seek(SeekFrom::Start(offset))?;
    let chunk: [u8; 8] = read_array(source, "a chunk header")?;
    let printable = chunk[..4].iter().all(|byte| (0x20..=0x7E).contains(byte));
    let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
    Ok(printable && body.checked_add(size).is_some_and(|end| end <= file_length))
}

/// The 64-bit data size from a ds64 chunk of `size` bytes.
fn read_ds64<R: Read>(source: &mut R, size: u32) -> Result<u64, WavError> {
    if size < 24 {
        return Err(WavError::Malformed(format!(
            "a ds64 chunk of {size} bytes, under the 24 its sizes take"
        )));
    }
    // The RIFF size is not needed, nor the sample count after the data size: frames are counted
    // from the data size.
    let _riff_size: [u8; 8] = read_array(source, "the ds64 chunk")?;
    let data_size: [u8; 8] = read_array(source, "the ds64 chunk")?;
    Ok(u64::from_le_bytes(data_size))
}

/// The spec a fmt chunk of `size` bytes describes: integer PCM of 16, 24 or 32 bits or 32-bit
/// float, plain or WAVE_FORMAT_EXTENSIBLE.
fn read_format<R: Read>(source: &mut R, size: u32) -> Result<WavSpec, WavError> {
    if size < 16 {
        return Err(WavError::Malformed(format!(
            "a fmt chunk of {size} bytes, under the 16 every format needs"
        )));
    }
    let mut body = [0; FMT_SIZE as usize];
    let length = size.min(FMT_SIZE) as usize;
    read_exact_or(source, &mut body[..length], "the fmt chunk")?;
    let field16 = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
    let tag = field16(0);
    let channels = field16(2);
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let block_align = field16(12);
    let bits = field16(14);

    let code = if tag == FORMAT_EXTENSIBLE {
        if length < FMT_SIZE as usize || field16(16) < EXTENSION_SIZE {
            return Err(WavError::Malformed(
                "a WAVE_FORMAT_EXTENSIBLE fmt chunk without its 22-byte extension".to_owned(),
            ));
        }
        if body[26..] != SUBFORMAT_TAIL {
            return Err(WavError::Unsupported(
                "a WAVE_FORMAT_EXTENSIBLE subformat that is not a WAVE format code".to_owned(),
            ));
        }
        field16(24)
    } else {
        tag
    };
    // The container size decides the decoding; fewer valid bits sit in its top bits.
    let format = match (code, bits) {
        (FORMAT_PCM, 16) => SampleFormat::Int16,
        (FORMAT_PCM, 24) => SampleFormat::Int24,
        (FORMAT_PCM, 32) => SampleFormat::Int32,
        (FORMAT_IEEE_FLOAT, 32) => SampleFormat::Float32,
        (FORMAT_PCM, _) => {
            return Err(WavError::Unsupported(format!("{bits}-bit integer PCM")));
        }
        (FORMAT_IEEE_FLOAT, _) => {
            return Err(WavError::Unsupported(format!("{bits}-bit float")));
        }
        _ => {
            return Err(WavError::Unsupported(format!("format code {code:#06x}")));
        }
    };
    if channels == 0 {
        return Err(WavError::Malformed("a format with no channels".to_owned()));
    }
    if sample_rate == 0 {
        return Err(WavError::Malformed("a sample rate of 0 Hz".to_owned()));
    }
    let spec = WavSpec {
        channels,
        sample_rate,
        format,
    };
    if u32::from(block_align) != spec.bytes_per_frame() {
        return Err(WavError::Malformed(format!(
            "a block align of {block_align} bytes for {channels} channels of {} bytes",
            format.bytes_per_sample()
        )));
    }
    Ok(spec)
}

/// Converts `bytes` of `format` samples to f32: integers scaled by a power of two (2^15, 2^23 or
/// 2^31, as capture does), floats as they are.
fn decode(format: SampleFormat, bytes: &[u8], output: &mut [f32]) {
    let width = usize::from(format.bytes_per_sample());
    for (sample, raw) in output.iter_mut().zip(bytes.chunks_exact(width)) {
        *sample = match format {
            SampleFormat::Int16 => {
                f32::from(i16::from_le_bytes([raw[0], raw[1]])) * (1.0 / 32_768.0)
            }
            SampleFormat::Int24 => {
                i32::from_le_bytes([0, raw[0], raw[1], raw[2]]) as f32 * (1.0 / 2_147_483_648.0)
            }
            SampleFormat::Int32 => {
                i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32
                    * (1.0 / 2_147_483_648.0)
            }
            SampleFormat::Float32 => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
        };
    }
}

/// Reads `N` bytes, calling the file malformed when it ends inside `what`.
fn read_array<const N: usize, R: Read>(source: &mut R, what: &str) -> Result<[u8; N], WavError> {
    let mut bytes = [0; N];
    read_exact_or(source, &mut bytes, what)?;
    Ok(bytes)
}

fn read_exact_or<R: Read>(source: &mut R, buffer: &mut [u8], what: &str) -> Result<(), WavError> {
    source.read_exact(buffer).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            WavError::Malformed(format!("the file ends inside {what}"))
        } else {
            WavError::Io(error)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// One step of 24-bit audio.
    const STEP: f64 = 1.0 / 8_388_608.0;

    /// KSDATAFORMAT_SUBTYPE_PCM and KSDATAFORMAT_SUBTYPE_IEEE_FLOAT as they sit in a file.
    const PCM_GUID: [u8; 16] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ];
    const FLOAT_GUID: [u8; 16] = [
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ];

    fn spec(channels: u16, format: SampleFormat) -> WavSpec {
        WavSpec {
            channels,
            sample_rate: 48_000,
            format,
        }
    }

    fn mono24() -> WavSpec {
        spec(1, SampleFormat::Int24)
    }

    fn stereo24() -> WavSpec {
        spec(2, SampleFormat::Int24)
    }

    fn stereo_float() -> WavSpec {
        spec(2, SampleFormat::Float32)
    }

    /// `count` deterministic samples strictly inside (-1, 1), finer than a 24-bit step near zero.
    fn noise(count: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                // The divisor keeps the largest value two f32 steps below 1.0.
                ((f64::from(state) - 2_147_483_648.0) / 2_147_483_904.0) as f32
            })
            .collect()
    }

    /// The file `samples` make with `spec`, written in memory and finished.
    fn written(spec: WavSpec, samples: &[f32]) -> Vec<u8> {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), spec).unwrap();
        writer.write_frames(samples).unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// The spec and every frame of `bytes`.
    fn read_all(bytes: Vec<u8>) -> (WavSpec, Vec<f32>) {
        let mut reader = WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        let mut samples = vec![0.0; reader.frames() as usize * usize::from(spec.channels)];
        let frames = reader.read_frames(&mut samples).unwrap();
        assert_eq!(frames as u64, reader.frames());
        assert_eq!(
            reader.read_frames(&mut [0.0; 64]).unwrap(),
            0,
            "nothing past the end"
        );
        (spec, samples)
    }

    /// What reading `samples` back after writing them with `spec` gives.
    fn quantized(spec: WavSpec, samples: &[f32]) -> Vec<f32> {
        read_all(written(spec, samples)).1
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn u64_at(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }

    /// A target in memory that the test can still read after the writer is gone, and make fail.
    #[derive(Clone, Default)]
    struct SharedStore {
        bytes: Rc<RefCell<Cursor<Vec<u8>>>>,
        failing: Rc<Cell<bool>>,
    }

    impl SharedStore {
        fn contents(&self) -> Vec<u8> {
            self.bytes.borrow().get_ref().clone()
        }

        fn fail(&self, failing: bool) {
            self.failing.set(failing);
        }

        fn check(&self) -> io::Result<()> {
            if self.failing.get() {
                Err(io::Error::other("the disk is full"))
            } else {
                Ok(())
            }
        }
    }

    impl Write for SharedStore {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.check()?;
            self.bytes.borrow_mut().write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.check()
        }
    }

    impl Seek for SharedStore {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.borrow_mut().seek(position)
        }
    }

    impl WavTarget for SharedStore {
        fn sync(&mut self) -> io::Result<()> {
            self.check()
        }
    }

    /// A RIFF/WAVE file of `chunks`, each followed by a pad byte when its size is odd.
    fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        for (id, payload) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend(body);
        file
    }

    /// A 16-byte fmt payload.
    fn plain_fmt(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let align = channels * bits / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        fmt
    }

    /// A 40-byte WAVE_FORMAT_EXTENSIBLE fmt payload.
    fn extensible_fmt(
        channels: u16,
        rate: u32,
        bits: u16,
        valid_bits: u16,
        mask: u32,
        guid: [u8; 16],
    ) -> Vec<u8> {
        let mut fmt = plain_fmt(0xFFFE, channels, rate, bits);
        fmt.extend_from_slice(&22u16.to_le_bytes());
        fmt.extend_from_slice(&valid_bits.to_le_bytes());
        fmt.extend_from_slice(&mask.to_le_bytes());
        fmt.extend_from_slice(&guid);
        fmt
    }

    fn le16(values: &[i16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn le24(values: &[i32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [bytes[0], bytes[1], bytes[2]]
            })
            .collect()
    }

    fn le32(values: &[i32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn lef32(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn read_error(bytes: Vec<u8>) -> WavError {
        match WavReader::new(Cursor::new(bytes)) {
            Ok(reader) => panic!("read a file it should refuse: {:?}", reader.spec()),
            Err(error) => error,
        }
    }

    /// An empty scratch folder for one test's files.
    fn scratch_directory(test: &str) -> PathBuf {
        let directory = std::env::temp_dir().join("mixlink-wav-tests").join(test);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_24_bit_mono_file_has_the_expected_bytes() {
        let bytes = written(mono24(), &[0.5, -1.0, 1.0]);

        let mut expected = Vec::new();
        expected.extend_from_slice(b"RIFF");
        expected.extend_from_slice(&106u32.to_le_bytes()); // 104 + 9 + 1 pad - 8
        expected.extend_from_slice(b"WAVE");
        expected.extend_from_slice(b"JUNK");
        expected.extend_from_slice(&28u32.to_le_bytes());
        expected.extend_from_slice(&[0; 28]);
        expected.extend_from_slice(b"fmt ");
        expected.extend_from_slice(&40u32.to_le_bytes());
        expected.extend_from_slice(&[0xFE, 0xFF]); // WAVE_FORMAT_EXTENSIBLE
        expected.extend_from_slice(&[0x01, 0x00]); // 1 channel
        expected.extend_from_slice(&[0x80, 0xBB, 0x00, 0x00]); // 48000 Hz
        expected.extend_from_slice(&[0x80, 0x32, 0x02, 0x00]); // 144000 bytes per second
        expected.extend_from_slice(&[0x03, 0x00]); // 3 bytes per frame
        expected.extend_from_slice(&[0x18, 0x00]); // 24 bits per sample
        expected.extend_from_slice(&[0x16, 0x00]); // 22 bytes of extension
        expected.extend_from_slice(&[0x18, 0x00]); // 24 valid bits
        expected.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // front centre
        expected.extend_from_slice(&PCM_GUID);
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&9u32.to_le_bytes());
        expected.extend_from_slice(&[0x00, 0x00, 0x40]); // 0.5 -> 4194304
        expected.extend_from_slice(&[0x00, 0x00, 0x80]); // -1.0 -> -8388608
        expected.extend_from_slice(&[0xFF, 0xFF, 0x7F]); // 1.0 -> 8388607
        expected.push(0x00); // pad byte after odd-sized audio
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_float_stereo_file_has_the_expected_bytes() {
        let bytes = written(stereo_float(), &[0.25, -0.5]);

        let mut expected = Vec::new();
        expected.extend_from_slice(b"RIFF");
        expected.extend_from_slice(&104u32.to_le_bytes()); // 104 + 8 - 8
        expected.extend_from_slice(b"WAVE");
        expected.extend_from_slice(b"JUNK");
        expected.extend_from_slice(&28u32.to_le_bytes());
        expected.extend_from_slice(&[0; 28]);
        expected.extend_from_slice(b"fmt ");
        expected.extend_from_slice(&40u32.to_le_bytes());
        expected.extend_from_slice(&[0xFE, 0xFF]); // WAVE_FORMAT_EXTENSIBLE
        expected.extend_from_slice(&[0x02, 0x00]); // 2 channels
        expected.extend_from_slice(&[0x80, 0xBB, 0x00, 0x00]); // 48000 Hz
        expected.extend_from_slice(&[0x00, 0xDC, 0x05, 0x00]); // 384000 bytes per second
        expected.extend_from_slice(&[0x08, 0x00]); // 8 bytes per frame
        expected.extend_from_slice(&[0x20, 0x00]); // 32 bits per sample
        expected.extend_from_slice(&[0x16, 0x00]); // 22 bytes of extension
        expected.extend_from_slice(&[0x20, 0x00]); // 32 valid bits
        expected.extend_from_slice(&[0x03, 0x00, 0x00, 0x00]); // front left and right
        expected.extend_from_slice(&FLOAT_GUID);
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&8u32.to_le_bytes());
        expected.extend_from_slice(&[0x00, 0x00, 0x80, 0x3E]); // 0.25
        expected.extend_from_slice(&[0x00, 0x00, 0x00, 0xBF]); // -0.5
        assert_eq!(bytes, expected);
    }

    #[test]
    fn int24_round_trips_within_half_a_step_and_24_bit_values_exactly() {
        let samples = noise(4_000, 7);
        let (read_spec, read) = read_all(written(stereo24(), &samples));
        assert_eq!(read_spec, stereo24());
        assert_eq!(read.len(), samples.len());
        for (original, back) in samples.iter().zip(&read) {
            let error = (f64::from(*original) - f64::from(*back)).abs();
            assert!(error <= 0.5 * STEP, "{original} came back as {back}");
        }

        // What a 24-bit capture delivers comes back bit for bit.
        let steps: Vec<f32> = [-8_388_608, -4_194_303, -1, 0, 1, 77_777, 8_388_607]
            .iter()
            .map(|step| (f64::from(*step) * STEP) as f32)
            .collect();
        assert_eq!(quantized(mono24(), &steps), steps);
    }

    #[test]
    fn int24_clamps_past_full_scale_and_writes_nan_as_silence() {
        let top = (8_388_607.0 * STEP) as f32;
        let read = quantized(
            mono24(),
            &[
                1.0,
                1.5,
                f32::INFINITY,
                -1.0,
                -1.5,
                f32::NEG_INFINITY,
                f32::NAN,
            ],
        );
        assert_eq!(read, vec![top, top, top, -1.0, -1.0, -1.0, 0.0]);
    }

    #[test]
    fn float32_round_trips_exactly_even_past_full_scale() {
        let mut samples = noise(4_000, 11);
        samples.extend_from_slice(&[1.5, -2.0, 1e-30, -0.0]);
        let (read_spec, read) = read_all(written(stereo_float(), &samples));
        assert_eq!(read_spec, stereo_float());
        let bits = |values: &[f32]| {
            values
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(bits(&read), bits(&samples));
    }

    #[test]
    fn a_refresh_rewrites_the_sizes_in_place_and_writing_goes_on_after_the_audio() {
        let first = noise(100, 1);
        let later = noise(51, 2);
        let store = SharedStore::default();
        let mut writer = WavWriter::new(store.clone(), mono24()).unwrap();
        writer.write_frames(&first).unwrap();
        writer.refresh().unwrap();

        let bytes = store.contents();
        assert_eq!(bytes.len(), 104 + 300, "all the audio is in the file");
        assert_eq!(u32_at(&bytes, 4), 104 + 300 - 8, "RIFF size");
        assert_eq!(u32_at(&bytes, 100), 300, "data size");
        assert_eq!(writer.frames(), 100);

        writer.write_frames(&later).unwrap();
        writer.refresh().unwrap();
        writer.finish().unwrap();
        let all = [first, later].concat();
        assert_eq!(
            store.contents(),
            written(mono24(), &all),
            "refreshes leave no trace in the finished file"
        );
    }

    #[test]
    fn a_crash_after_a_refresh_reads_exactly_the_refreshed_frames() {
        let first = noise(200, 1);
        let expected = quantized(stereo24(), &first);
        for killed in [false, true] {
            let store = SharedStore::default();
            let mut writer = WavWriter::new(store.clone(), stereo24()).unwrap();
            writer.write_frames(&first).unwrap();
            writer.refresh().unwrap();
            writer.write_frames(&noise(60, 2)).unwrap();
            if killed {
                // The buffered audio never reaches the file, as when the process is killed.
                std::mem::forget(writer);
            } else {
                // Dropping hands the buffered audio to the file, past the header's data size.
                drop(writer);
            }
            let (read_spec, read) = read_all(store.contents());
            assert_eq!(read_spec, stereo24());
            assert_eq!(read, expected, "killed: {killed}");
        }
    }

    #[test]
    fn a_crash_before_the_first_refresh_recovers_the_whole_frames_in_the_file() {
        let samples = noise(200, 3);
        let expected = quantized(stereo24(), &samples);
        let store = SharedStore::default();
        let mut writer = WavWriter::new(store.clone(), stereo24()).unwrap();
        writer.write_frames(&samples).unwrap();
        drop(writer);

        let mut bytes = store.contents();
        assert_eq!(u32_at(&bytes, 100), 0, "the header still says no audio");
        assert_eq!(read_all(bytes.clone()).1, expected);

        // A frame cut short by the crash is left out.
        bytes.truncate(bytes.len() - 4);
        assert_eq!(read_all(bytes).1, expected[..198]);
    }

    #[test]
    fn an_empty_data_chunk_of_another_writer_stays_empty() {
        // Chunks after an empty data chunk are not audio.
        let bytes = riff(&[
            (b"fmt ", &plain_fmt(1, 2, 48_000, 24)),
            (b"data", &[]),
            (b"LIST", b"INFOISFT\x05\x00\x00\x00tool\x00"),
        ]);
        assert_eq!(read_all(bytes), (stereo24(), Vec::new()));

        // Bytes that cannot begin a chunk are the audio of a writer that died before its first
        // refresh, whoever laid the file out.
        let mut bytes = riff(&[(b"fmt ", &plain_fmt(1, 1, 48_000, 24)), (b"data", &[])]);
        bytes.extend_from_slice(&le24(&[0x40_0000, -0x40_0000, 0x20_0000]));
        assert_eq!(read_all(bytes), (mono24(), vec![0.5, -0.5, 0.25]));
    }

    #[test]
    fn an_unfinished_file_laid_out_as_ours_is_recovered_even_if_its_audio_reads_like_a_chunk() {
        let samples = noise(200, 3);
        let store = SharedStore::default();
        let mut writer = WavWriter::new(store.clone(), stereo24()).unwrap();
        writer.write_frames(&samples).unwrap();
        drop(writer);
        let mut bytes = store.contents();
        bytes[104..112].copy_from_slice(b"LIST\x04\x00\x00\x00");
        assert_eq!(WavReader::new(Cursor::new(bytes)).unwrap().frames(), 100);
    }

    #[test]
    fn a_data_size_past_the_end_of_the_file_reads_the_frames_there() {
        let samples = noise(200, 4);
        let expected = quantized(stereo24(), &samples);

        // The header reached the disk, the last of the audio did not.
        let mut bytes = written(stereo24(), &samples);
        bytes.truncate(104 + 6 * 40 + 3);
        assert_eq!(read_all(bytes).1, expected[..80]);

        // A streaming writer's "unknown" size.
        let mut bytes = written(stereo24(), &samples);
        bytes[100..104].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_all(bytes).1, expected);
    }

    #[test]
    fn finishing_past_the_size_limit_converts_the_file_to_rf64() {
        // 10 stereo frames: 60 bytes of audio, a RIFF size of 156.
        let samples = noise(20, 5);
        let plain = written(stereo24(), &samples);
        let mut writer =
            WavWriter::with_size_limit(Cursor::new(Vec::new()), stereo24(), 155).unwrap();
        writer.write_frames(&samples).unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        assert_eq!(bytes.len(), plain.len());
        assert_eq!(&bytes[0..4], b"RF64");
        assert_eq!(u32_at(&bytes, 4), u32::MAX);
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"ds64");
        assert_eq!(u32_at(&bytes, 16), 28);
        assert_eq!(u64_at(&bytes, 20), 156, "64-bit RIFF size");
        assert_eq!(u64_at(&bytes, 28), 60, "64-bit data size");
        assert_eq!(u64_at(&bytes, 36), 10, "sample count");
        assert_eq!(u32_at(&bytes, 44), 0, "no table");
        assert_eq!(bytes[48..96], plain[48..96], "the fmt chunk is unchanged");
        assert_eq!(&bytes[96..100], b"data");
        assert_eq!(u32_at(&bytes, 100), u32::MAX);
        assert_eq!(bytes[104..], plain[104..], "the audio is unchanged");

        assert_eq!(read_all(bytes), read_all(plain));
    }

    #[test]
    fn a_file_up_to_the_size_limit_stays_riff_and_the_pad_byte_counts() {
        let samples = noise(20, 5);
        let at_limit = |limit: u64, spec: WavSpec, samples: &[f32]| {
            let mut writer =
                WavWriter::with_size_limit(Cursor::new(Vec::new()), spec, limit).unwrap();
            writer.write_frames(samples).unwrap();
            writer.finish().unwrap().into_inner()
        };
        assert_eq!(
            at_limit(156, stereo24(), &samples),
            written(stereo24(), &samples)
        );
        assert_eq!(&at_limit(155, stereo24(), &samples)[0..4], b"RF64");

        // Three mono frames: 9 bytes of audio and a pad byte, a RIFF size of 106.
        let odd = [0.1, 0.2, 0.3];
        assert_eq!(&at_limit(106, mono24(), &odd)[0..4], b"RIFF");
        let bytes = at_limit(105, mono24(), &odd);
        assert_eq!(&bytes[0..4], b"RF64");
        assert_eq!(bytes.len(), 114);
        assert_eq!(u64_at(&bytes, 20), 106, "the pad byte is in the RIFF size");
        assert_eq!(u64_at(&bytes, 28), 9, "but not in the data size");
        assert_eq!(read_all(bytes).1, quantized(mono24(), &odd));
    }

    #[test]
    fn a_refresh_past_the_size_limit_leaves_a_readable_rf64_file() {
        let first = noise(20, 6);
        let later = noise(20, 7);
        let store = SharedStore::default();
        let mut writer = WavWriter::with_size_limit(store.clone(), stereo24(), 120).unwrap();
        writer.write_frames(&first).unwrap();
        writer.refresh().unwrap();

        let bytes = store.contents();
        assert_eq!(&bytes[0..4], b"RF64");
        assert_eq!(u64_at(&bytes, 28), 60);
        assert_eq!(read_all(bytes).1, quantized(stereo24(), &first));

        writer.write_frames(&later).unwrap();
        writer.finish().unwrap();
        let bytes = store.contents();
        assert_eq!(u64_at(&bytes, 20), 104 + 120 - 8);
        assert_eq!(u64_at(&bytes, 28), 120);
        assert_eq!(u64_at(&bytes, 36), 20);
        assert_eq!(
            read_all(bytes).1,
            quantized(stereo24(), &[first, later].concat())
        );
    }

    #[test]
    fn reads_16_bit_pcm_mono() {
        let bytes = riff(&[
            (b"fmt ", &plain_fmt(1, 1, 44_100, 16)),
            (b"data", &le16(&[0, 16_384, -32_768, 32_767, -1])),
        ]);
        let (read_spec, read) = read_all(bytes);
        assert_eq!(
            read_spec,
            WavSpec {
                channels: 1,
                sample_rate: 44_100,
                format: SampleFormat::Int16
            }
        );
        assert_eq!(
            read,
            vec![0.0, 0.5, -1.0, 32_767.0 / 32_768.0, -1.0 / 32_768.0]
        );
    }

    #[test]
    fn reads_24_and_32_bit_pcm_stereo() {
        let bytes = riff(&[
            (b"fmt ", &plain_fmt(1, 2, 48_000, 24)),
            (
                b"data",
                &le24(&[0x40_0000, -0x40_0000, 0x7F_FFFF, 1, -0x80_0000, 0]),
            ),
        ]);
        let (read_spec, read) = read_all(bytes);
        assert_eq!(read_spec, stereo24());
        assert_eq!(
            read,
            vec![
                0.5,
                -0.5,
                (8_388_607.0 * STEP) as f32,
                STEP as f32,
                -1.0,
                0.0
            ]
        );

        let bytes = riff(&[
            (b"fmt ", &plain_fmt(1, 2, 96_000, 32)),
            (
                b"data",
                &le32(&[1 << 30, i32::MIN, 0x7FFF_FF00, -(1 << 29)]),
            ),
        ]);
        let (read_spec, read) = read_all(bytes);
        assert_eq!(read_spec, spec(2, SampleFormat::Int32).with_rate(96_000));
        assert_eq!(read, vec![0.5, -1.0, (8_388_607.0 * STEP) as f32, -0.25]);
    }

    #[test]
    fn reads_float_with_a_short_fmt_and_a_fact_chunk() {
        // The 18-byte fmt (an empty extension) and the fact chunk older float writers add.
        let mut fmt = plain_fmt(3, 1, 48_000, 32);
        fmt.extend_from_slice(&0u16.to_le_bytes());
        let bytes = riff(&[
            (b"fmt ", &fmt),
            (b"fact", &3u32.to_le_bytes()),
            (b"data", &lef32(&[0.25, -1.5, 0.0])),
        ]);
        let (read_spec, read) = read_all(bytes);
        assert_eq!(read_spec, spec(1, SampleFormat::Float32));
        assert_eq!(read, vec![0.25, -1.5, 0.0]);
    }

    #[test]
    fn reads_wave_format_extensible() {
        let bytes = riff(&[
            (b"fmt ", &extensible_fmt(2, 48_000, 24, 24, 3, PCM_GUID)),
            (b"data", &le24(&[0x40_0000, -0x20_0000])),
        ]);
        assert_eq!(read_all(bytes), (stereo24(), vec![0.5, -0.25]));

        let bytes = riff(&[
            (b"fmt ", &extensible_fmt(1, 48_000, 32, 32, 4, FLOAT_GUID)),
            (b"data", &lef32(&[0.125, -0.75])),
        ]);
        assert_eq!(
            read_all(bytes),
            (spec(1, SampleFormat::Float32), vec![0.125, -0.75])
        );

        // 24 valid bits in a 32-bit container read as 32-bit samples.
        let bytes = riff(&[
            (b"fmt ", &extensible_fmt(1, 48_000, 32, 24, 4, PCM_GUID)),
            (b"data", &le32(&[0x4000_0000, -0x0000_0100])),
        ]);
        assert_eq!(
            read_all(bytes),
            (spec(1, SampleFormat::Int32), vec![0.5, -STEP as f32])
        );
    }

    #[test]
    fn skips_unknown_and_odd_sized_chunks_and_stops_at_the_data_size() {
        let bytes = riff(&[
            (b"JUNK", &[0; 5]),
            (b"bext", &[7; 6]),
            (b"fmt ", &plain_fmt(1, 1, 48_000, 24)),
            (b"LIST", b"INFOabc"),
            (b"odd ", &[1]),
            (b"data", &le24(&[0x40_0000])),
            (b"LIST", b"INFOsome trailing words"),
        ]);
        assert_eq!(read_all(bytes), (mono24(), vec![0.5]));

        // A fmt chunk after the audio is found past it.
        let bytes = riff(&[
            (b"data", &le24(&[0x40_0000, 0x20_0000])),
            (b"LIST", b"INFO"),
            (b"fmt ", &plain_fmt(1, 1, 48_000, 24)),
        ]);
        assert_eq!(read_all(bytes), (mono24(), vec![0.5, 0.25]));
    }

    #[test]
    fn reads_a_hand_built_rf64_file_and_one_missing_its_ds64() {
        let fmt = plain_fmt(1, 2, 48_000, 24);
        let audio = le24(&[0x40_0000, -0x40_0000, 0x20_0000, -0x20_0000]);

        let mut bytes = b"RF64".to_vec();
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"ds64");
        bytes.extend_from_slice(&28u32.to_le_bytes());
        bytes.extend_from_slice(&(4 + 36 + 24 + 8 + 12u64).to_le_bytes());
        bytes.extend_from_slice(&6u64.to_le_bytes()); // one frame of the two that follow
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&fmt);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&audio);
        assert_eq!(read_all(bytes.clone()), (stereo24(), vec![0.5, -0.5]));

        // Without a ds64 the size is unknown, so every whole frame there is read.
        let mut missing = bytes[..12].to_vec();
        missing.extend_from_slice(&bytes[48..]);
        missing.push(0x11); // part of a frame
        assert_eq!(
            read_all(missing),
            (stereo24(), vec![0.5, -0.5, 0.25, -0.25])
        );
    }

    #[test]
    fn reads_in_chunks_of_any_length_and_seeks() {
        let samples = noise(20, 8);
        let expected = quantized(stereo_float(), &samples);
        let mut reader = WavReader::new(Cursor::new(written(stereo_float(), &samples))).unwrap();
        assert_eq!(reader.frames(), 10);

        // 7 samples hold 3 whole stereo frames.
        let mut buffer = [9.0; 7];
        let mut read = Vec::new();
        let mut counts = Vec::new();
        loop {
            let frames = reader.read_frames(&mut buffer).unwrap();
            if frames == 0 {
                break;
            }
            counts.push(frames);
            read.extend_from_slice(&buffer[..frames * 2]);
        }
        assert_eq!(counts, vec![3, 3, 3, 1]);
        assert_eq!(read, expected);
        assert_eq!(
            buffer[6], 9.0,
            "the sample past the whole frames is untouched"
        );
        assert_eq!(reader.position(), 10);

        reader.seek(4).unwrap();
        assert_eq!(reader.position(), 4);
        let mut two = [0.0; 4];
        assert_eq!(reader.read_frames(&mut two).unwrap(), 2);
        assert_eq!(two[..], expected[8..12]);

        reader.seek(1_000).unwrap();
        assert_eq!(
            reader.position(),
            10,
            "a seek past the end stops at the end"
        );
        assert_eq!(reader.read_frames(&mut two).unwrap(), 0);
        assert_eq!(reader.read_frames(&mut [0.0; 1]).unwrap(), 0);
    }

    #[test]
    fn broken_headers_are_malformed() {
        let fmt = plain_fmt(1, 1, 48_000, 24);
        let audio = le24(&[1, 2]);
        let mut channel_less = fmt.clone();
        channel_less[2] = 0;
        let mut misaligned = fmt.clone();
        misaligned[12] = 4;
        let mut rateless = fmt.clone();
        rateless[4..8].copy_from_slice(&0u32.to_le_bytes());
        let mut short_extension = extensible_fmt(1, 48_000, 24, 24, 4, PCM_GUID);
        short_extension.truncate(18);
        let mut truncated_fmt = riff(&[(b"fmt ", &fmt)]);
        truncated_fmt.truncate(12 + 8 + 10);

        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("only the RIFF id", b"RIFF".to_vec()),
            ("cut inside WAVE", b"RIFF\x04\x00\x00\x00WAV".to_vec()),
            ("big-endian RIFX", b"RIFX\x04\x00\x00\x00WAVE".to_vec()),
            ("not WAVE", b"RIFF\x04\x00\x00\x00AVI ".to_vec()),
            ("no fmt", riff(&[(b"data", &audio)])),
            ("no data", riff(&[(b"fmt ", &fmt)])),
            ("cut inside fmt", truncated_fmt),
            (
                "short fmt",
                riff(&[(b"fmt ", &fmt[..14]), (b"data", &audio)]),
            ),
            (
                "no channels",
                riff(&[(b"fmt ", &channel_less), (b"data", &audio)]),
            ),
            (
                "no sample rate",
                riff(&[(b"fmt ", &rateless), (b"data", &audio)]),
            ),
            (
                "wrong block align",
                riff(&[(b"fmt ", &misaligned), (b"data", &audio)]),
            ),
            (
                "short extension",
                riff(&[(b"fmt ", &short_extension), (b"data", &audio)]),
            ),
            (
                "audio of unknown length before fmt",
                riff(&[(b"data", &[]), (b"fmt ", &fmt)]),
            ),
        ];
        for (case, bytes) in cases {
            let error = read_error(bytes);
            assert!(matches!(error, WavError::Malformed(_)), "{case}: {error:?}");
        }
    }

    #[test]
    fn encodings_it_cannot_decode_are_unsupported() {
        let audio = [0u8; 16];
        let mut unknown_guid = PCM_GUID;
        unknown_guid[15] = 0x72;
        let mut alaw_guid = PCM_GUID;
        alaw_guid[0] = 0x06;
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("8-bit PCM", plain_fmt(1, 1, 48_000, 8)),
            ("ADPCM", plain_fmt(2, 1, 48_000, 4)),
            ("MP3", plain_fmt(0x55, 2, 48_000, 0)),
            ("64-bit float", plain_fmt(3, 1, 48_000, 64)),
            (
                "extensible A-law",
                extensible_fmt(1, 48_000, 8, 8, 4, alaw_guid),
            ),
            (
                "extensible foreign GUID",
                extensible_fmt(1, 48_000, 24, 24, 4, unknown_guid),
            ),
        ];
        for (case, fmt) in cases {
            let error = read_error(riff(&[(b"fmt ", &fmt), (b"data", &audio)]));
            assert!(
                matches!(error, WavError::Unsupported(_)),
                "{case}: {error:?}"
            );
        }
    }

    #[test]
    fn every_truncation_reads_or_fails_without_panicking() {
        let bytes = written(stereo24(), &noise(8, 9));
        for length in 0..=bytes.len() {
            let result = WavReader::new(Cursor::new(bytes[..length].to_vec()));
            if length < 104 {
                assert!(
                    matches!(result, Err(WavError::Malformed(_))),
                    "{length} bytes"
                );
            } else {
                let mut reader = result.unwrap();
                assert_eq!(reader.frames(), (length as u64 - 104) / 6, "{length} bytes");
                let mut buffer = [0.0; 8];
                assert_eq!(
                    reader.read_frames(&mut buffer).unwrap() as u64,
                    reader.frames()
                );
            }
        }
    }

    #[test]
    fn the_writer_refuses_specs_and_partial_frames_it_cannot_write() {
        let refused = [
            spec(0, SampleFormat::Int24),
            WavSpec {
                sample_rate: 0,
                ..mono24()
            },
            spec(1, SampleFormat::Int16),
            spec(2, SampleFormat::Int32),
            spec(u16::MAX, SampleFormat::Float32),
        ];
        for refused in refused {
            let result = WavWriter::new(Cursor::new(Vec::new()), refused);
            assert!(
                matches!(result, Err(WavError::InvalidInput(_))),
                "{refused:?}"
            );
        }

        let mut writer = WavWriter::new(Cursor::new(Vec::new()), stereo24()).unwrap();
        assert!(matches!(
            writer.write_frames(&[0.1, 0.2, 0.3]),
            Err(WavError::InvalidInput(_))
        ));
        writer.write_frames(&[0.1, 0.2]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(
            bytes,
            written(stereo24(), &[0.1, 0.2]),
            "a refused call writes nothing"
        );
    }

    #[test]
    fn a_failed_write_keeps_the_last_good_header_and_stops_the_writer() {
        let first = noise(100, 1);
        let store = SharedStore::default();
        let mut writer = WavWriter::new(store.clone(), mono24()).unwrap();
        writer.write_frames(&first).unwrap();
        writer.refresh().unwrap();

        store.fail(true);
        // More than the buffer holds, so the write reaches the failing disk.
        assert!(matches!(
            writer.write_frames(&noise(10_000, 2)),
            Err(WavError::Io(_))
        ));
        assert!(matches!(
            writer.write_frames(&first),
            Err(WavError::Stopped)
        ));
        assert!(matches!(writer.refresh(), Err(WavError::Stopped)));
        assert!(matches!(writer.sync(), Err(WavError::Stopped)));
        // Even with the disk back, the stopped writer leaves the header alone.
        store.fail(false);
        assert!(matches!(writer.finish(), Err(WavError::Stopped)));

        assert_eq!(read_all(store.contents()).1, quantized(mono24(), &first));
    }

    #[test]
    fn a_failed_refresh_keeps_the_last_good_header() {
        let first = noise(100, 1);
        let store = SharedStore::default();
        let mut writer = WavWriter::new(store.clone(), mono24()).unwrap();
        writer.write_frames(&first).unwrap();
        writer.refresh().unwrap();
        writer.write_frames(&noise(50, 2)).unwrap();

        store.fail(true);
        assert!(matches!(writer.refresh(), Err(WavError::Io(_))));
        store.fail(false);
        assert!(matches!(writer.refresh(), Err(WavError::Stopped)));
        drop(writer);

        assert_eq!(read_all(store.contents()).1, quantized(mono24(), &first));
    }

    #[test]
    fn a_file_on_disk_is_created_once_synced_and_read_back() {
        let directory = scratch_directory("disk");
        let path = directory.join("track.wav");

        assert!(matches!(
            WavWriter::create(&path, spec(0, SampleFormat::Float32)),
            Err(WavError::InvalidInput(_))
        ));
        assert!(!path.exists(), "a refused spec creates no file");

        let samples = noise(960, 10);
        let mut writer = WavWriter::create(&path, stereo_float()).unwrap();
        writer.write_frames(&samples[..480]).unwrap();
        writer.sync().unwrap();
        let mut reader = WavReader::open(&path).unwrap();
        assert_eq!(
            reader.frames(),
            240,
            "a synced file reads while it is written"
        );
        let mut buffer = vec![0.0; 480];
        assert_eq!(reader.read_frames(&mut buffer).unwrap(), 240);
        assert_eq!(buffer, samples[..480]);
        drop(reader);

        writer.write_frames(&samples[480..]).unwrap();
        writer.finish().unwrap();
        let mut reader = WavReader::open(&path).unwrap();
        assert_eq!(reader.spec(), stereo_float());
        let mut buffer = vec![0.0; 960];
        assert_eq!(reader.read_frames(&mut buffer).unwrap(), 480);
        assert_eq!(buffer, samples);
        drop(reader);

        assert!(
            matches!(
                WavWriter::create(&path, stereo_float()),
                Err(WavError::Io(_))
            ),
            "an existing take is never overwritten"
        );
        fs::remove_dir_all(&directory).unwrap();
    }

    impl WavSpec {
        fn with_rate(self, sample_rate: u32) -> Self {
            WavSpec {
                sample_rate,
                ..self
            }
        }
    }
}
