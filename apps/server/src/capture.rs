use std::cmp::Reverse;
use std::error::Error;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{
    BufferSize, Device, InputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
};

use crate::decimate::sample_rate_reduction_ratio;
use crate::mix::MAX_MIX_CHANNELS;
use crate::pipeline::{
    block_frames, CaptureLinks, CaptureSample, CaptureWriter, SourceFormat, SourceReader,
};

pub(crate) const TARGET_SAMPLE_RATE: u32 = 48_000;

/// One selectable input device and the channel count the engine would capture from it.
///
/// `channels` is the count of the configuration [select_input_config] would choose, not the raw
/// maximum the hardware advertises, so it is exactly what a switch would change the session to.
/// A device with no supported PCM input reports `0` and is listed but not usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub channels: u16,
}

/// Which audio host the engine captures from.
///
/// `Auto` is the default and resolves to the platform's own host, which is the safe choice: it needs
/// no third-party driver. ASIO is deliberately explicit, because choosing it loads a kernel-mode
/// driver, and an unstable one takes the whole machine down: a Realtek ASIO driver did exactly that
/// during testing, with two bugchecks in one afternoon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioHost {
    #[default]
    Auto,
    Asio,
    Wasapi,
}

impl AudioHost {
    /// Parses a `--host` value. Case-insensitive; the error names the accepted values.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "asio" => Ok(Self::Asio),
            "wasapi" => Ok(Self::Wasapi),
            other => Err(format!("--host `{other}` is not one of auto, asio, wasapi")),
        }
    }

    /// The value the window shows and the CLI accepts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Asio => "asio",
            Self::Wasapi => "wasapi",
        }
    }
}

/// The ASIO host, created once and shared.
///
/// ASIO can only have one driver loaded at a time, and building a second host re-loads a driver,
/// which fails as soon as the device list has already touched a different one - exactly what a UI
/// that lists devices and then starts the engine does.
#[cfg(all(target_os = "windows", feature = "asio"))]
fn asio_host() -> &'static cpal::Host {
    use std::sync::OnceLock;

    static HOST: OnceLock<cpal::Host> = OnceLock::new();
    HOST.get_or_init(|| match cpal::host_from_id(cpal::HostId::Asio) {
        Ok(host) => host,
        Err(error) => {
            crate::log_warn!(
                "capture",
                "ASIO host unavailable ({error}); falling back to the default host"
            );
            cpal::default_host()
        }
    })
}

/// The platform default host, created once so it can be returned by reference like [asio_host].
fn default_host() -> &'static cpal::Host {
    use std::sync::OnceLock;

    static HOST: OnceLock<cpal::Host> = OnceLock::new();
    HOST.get_or_init(cpal::default_host)
}

/// The cpal host the engine captures from, for the requested backend.
///
/// Only an explicit [AudioHost::Asio] loads the ASIO driver. `Auto` resolves to the platform's own
/// host: ASIO is a kernel-mode driver and an unstable one can restart the machine, so it is never
/// entered by default. When ASIO is asked for and cannot load, the default host is used instead of
/// failing the session.
pub(crate) fn input_host(host: AudioHost) -> &'static cpal::Host {
    let want_asio = matches!(host, AudioHost::Asio);
    #[cfg(all(target_os = "windows", feature = "asio"))]
    if want_asio {
        return asio_host();
    }
    let _ = want_asio;
    default_host()
}

/// Enumerates the default host's input devices with the channel count a switch would capture.
pub fn list_input_devices() -> Result<Vec<InputDevice>, Box<dyn Error>> {
    list_input_devices_for(AudioHost::Auto)
}

/// Enumerates one backend's input devices with the channel count a switch would capture.
pub fn list_input_devices_for(host: AudioHost) -> Result<Vec<InputDevice>, Box<dyn Error>> {
    // ASIO is listed from the registry instead of from cpal. cpal describes a device by loading its
    // driver, and ASIO only holds one driver at a time, so a running session hides every driver but
    // its own. The installed names are exactly what a DAW lists, and they are the names cpal uses.
    // Only an explicit ASIO request lists them: `Auto` is the safe host and must not offer drivers
    // that load a kernel-mode driver.
    #[cfg(all(target_os = "windows", feature = "asio"))]
    if matches!(host, AudioHost::Asio) {
        let names = installed_asio_driver_names();
        if !names.is_empty() {
            return Ok(names
                .into_iter()
                .map(|name| InputDevice { name, channels: 0 })
                .collect());
        }
    }

    let host = input_host(host);
    let devices = host
        .input_devices()
        .map_err(|error| format!("could not enumerate input devices: {error}"))?;
    Ok(devices
        .map(|device| {
            describe_input_device(
                device.to_string(),
                select_input_config(&device).map(|config| config.channels()),
            )
        })
        .collect())
}

/// Parses the subkey lines of `reg query HKLM\SOFTWARE\ASIO` into the installed driver names.
///
/// Pure so it can be tested without the registry. `reg` prints the full hive name, so both the
/// `HKEY_LOCAL_MACHINE` and `HKLM` spellings are accepted.
fn parse_asio_driver_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line
                .strip_prefix(r"HKEY_LOCAL_MACHINE\SOFTWARE\ASIO\")
                .or_else(|| line.strip_prefix(r"HKLM\SOFTWARE\ASIO\"))?;
            let name = rest.trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

/// Reads the installed ASIO driver names from the Windows registry.
///
/// Shelling out to `reg` keeps this to one call and no new dependency, and the registry is the only
/// complete source: asking cpal would load a driver, which is the thing that hides the others.
#[cfg(all(target_os = "windows", feature = "asio"))]
fn installed_asio_driver_names() -> Vec<String> {
    let Ok(output) = std::process::Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\ASIO"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_asio_driver_names(&String::from_utf8_lossy(&output.stdout))
}

/// Joins a device name with its configuration selection into a reportable [InputDevice].
///
/// Kept pure and separate from enumeration so the channel-count reporting is testable without an
/// audio host; an unsupported device carries `0` channels rather than being hidden.
fn describe_input_device(name: String, config: Result<u16, Box<dyn Error>>) -> InputDevice {
    InputDevice {
        name,
        channels: config.unwrap_or(0),
    }
}

/// Selects the input device whose reported name equals `name`, exactly.
///
/// Device names overlap (a bare interface name is a prefix of the one with a port suffix), so a
/// switch that came from a device list must match the whole name rather than a substring.
pub(crate) fn select_device_by_name(
    devices: Vec<Device>,
    name: &str,
) -> Result<Device, Box<dyn Error>> {
    devices
        .into_iter()
        .find(|device| device.to_string() == name)
        .ok_or_else(|| format!("input device \"{name}\" is no longer available").into())
}

pub(crate) fn select_device(
    devices: Vec<Device>,
    filter: Option<&str>,
) -> Result<Device, Box<dyn Error>> {
    match filter {
        None => Ok(devices
            .into_iter()
            .next()
            .expect("device list was checked before selection")),
        Some(filter) => {
            let filter = filter.to_lowercase();
            devices
                .into_iter()
                .find(|device| device.to_string().to_lowercase().contains(&filter))
                .ok_or_else(|| format!("no input device name contains \"{filter}\".").into())
        }
    }
}

pub(crate) fn select_input_config(
    device: &Device,
) -> Result<SupportedStreamConfig, Box<dyn Error>> {
    let mut best_config = None;
    let supported_configs = device
        .supported_input_configs()
        .map_err(|error| format!("could not enumerate input configurations: {error}"))?;

    for range in supported_configs {
        if !is_supported_sample_format(range.sample_format()) {
            continue;
        }

        let sample_rate = if supports_sample_rate(
            range.min_sample_rate(),
            range.max_sample_rate(),
            TARGET_SAMPLE_RATE,
        ) {
            TARGET_SAMPLE_RATE
        } else if let Some(sample_rate) = supported_reduction_sample_rate(&range) {
            sample_rate
        } else {
            continue;
        };

        let config = range.with_sample_rate(sample_rate);
        let rank = input_config_rank(sample_rate, config.channels(), config.sample_format());
        if best_config
            .as_ref()
            .map_or(true, |(best_rank, _)| rank < *best_rank)
        {
            best_config = Some((rank, config));
        }
    }

    best_config.map(|(_, config)| config).ok_or_else(|| {
        format!(
            "input device has no supported PCM input at {TARGET_SAMPLE_RATE} Hz or an integer multiple (96 kHz/192 kHz)"
        )
        .into()
    })
}

/// Orders candidate input configurations, lowest first: the target rate, then every input the
/// interface offers (up to what the mixer can hold), then the cheapest sample format to convert.
fn input_config_rank(
    sample_rate: u32,
    channels: u16,
    sample_format: SampleFormat,
) -> (u8, u8, Reverse<u16>, u8) {
    (
        u8::from(sample_rate != TARGET_SAMPLE_RATE),
        u8::from(usize::from(channels) > MAX_MIX_CHANNELS),
        Reverse(channels),
        sample_format_rank(sample_format),
    )
}

fn supported_reduction_sample_rate(range: &SupportedStreamConfigRange) -> Option<u32> {
    [TARGET_SAMPLE_RATE * 2, TARGET_SAMPLE_RATE * 4]
        .into_iter()
        .find(|sample_rate| {
            supports_sample_rate(
                range.min_sample_rate(),
                range.max_sample_rate(),
                *sample_rate,
            )
        })
}

pub(crate) fn sample_format_rank(sample_format: SampleFormat) -> u8 {
    match sample_format {
        SampleFormat::I16 => 0,
        SampleFormat::F32 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::U16 => 3,
        _ => u8::MAX,
    }
}

fn supports_sample_rate(min_sample_rate: u32, max_sample_rate: u32, sample_rate: u32) -> bool {
    min_sample_rate <= sample_rate && sample_rate <= max_sample_rate
}

pub(crate) fn is_supported_sample_format(sample_format: SampleFormat) -> bool {
    matches!(
        sample_format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::U16
    )
}

/// Applies an explicit buffer size to a stream configuration.
///
/// This is the single biggest lever on capture latency here: the engine's block, and so the packet
/// the phones are sent, is one callback long (see [block_frames]), so the callback size is also the
/// packet interval. Kept pure and separate from stream construction so the mapping is testable
/// without an audio host.
pub(crate) fn with_buffer_size(
    mut config: StreamConfig,
    buffer_frames: Option<u32>,
) -> StreamConfig {
    if let Some(frames) = buffer_frames {
        config.buffer_size = BufferSize::Fixed(frames);
    }
    config
}

/// Narrows a requested buffer to the range the device reports.
///
/// Some drivers only accept one buffer size (a Realtek ASIO driver reports `1024..=1024`, the Volt's
/// accepts a range). Requesting outside that range fails the stream open, so clamping here turns a
/// failed session into the size the driver actually allows.
pub(crate) fn clamp_buffer_frames(requested: u32, supported: &SupportedBufferSize) -> u32 {
    match supported {
        SupportedBufferSize::Range { min, max } => requested.clamp(*min, *max),
        SupportedBufferSize::Unknown => requested,
    }
}

/// Human-readable form of an applied buffer size, for `CaptureFormat::buffer_size`.
pub(crate) fn describe_buffer(buffer_frames: Option<u32>) -> String {
    match buffer_frames {
        Some(frames) => format!("Fixed({frames})"),
        None => "device default".to_owned(),
    }
}

/// One capture stream, open and ready to play: the stream itself, the buffer size that actually
/// took effect, and the engine thread's end of its ring.
pub(crate) struct OpenedStream {
    pub(crate) stream: Stream,
    pub(crate) buffer_frames: Option<u32>,
    pub(crate) reader: SourceReader,
}

/// Builds and opens the input stream with its own capture ring (see [crate::pipeline]).
///
/// An explicitly requested buffer that the driver rejects is not fatal: the session falls back to the
/// device default, which is exactly what the engine used before this option existed, and reports
/// `None` so the caller can see the request was not honoured instead of assuming it was.
pub(crate) fn build_input_stream(
    device: &Device,
    supported_config: &SupportedStreamConfig,
    buffer_frames: Option<u32>,
    links: &CaptureLinks,
) -> Result<OpenedStream, Box<dyn Error>> {
    let sample_format = supported_config.sample_format();
    let base = supported_config.config();
    let supported_buffer = supported_config.buffer_size();
    let requested_frames =
        buffer_frames.map(|frames| clamp_buffer_frames(frames, &supported_buffer));
    let requested = with_buffer_size(base.clone(), requested_frames);

    match open_stream(device, &requested, sample_format, requested_frames, links) {
        Ok((stream, reader)) => Ok(OpenedStream {
            stream,
            buffer_frames: requested_frames,
            reader,
        }),
        Err(error) => match requested_frames {
            Some(frames) => {
                crate::log_warn!(
                    "capture",
                    "Requested input buffer of {frames} frames was rejected ({error}); falling back to the device default buffer."
                );
                let fallback = with_buffer_size(base, None);
                let (stream, reader) = open_stream(device, &fallback, sample_format, None, links)?;
                Ok(OpenedStream {
                    stream,
                    buffer_frames: None,
                    reader,
                })
            }
            None => Err(error),
        },
    }
}

/// Opens one stream whose callback feeds a fresh ring through a [CaptureWriter].
///
/// Every supported sample format goes through the same writer, which converts it to f32 (keeping
/// 24-bit resolution), decimates high rates through the anti-alias filter and meters the result.
fn open_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    buffer_frames: Option<u32>,
    links: &CaptureLinks,
) -> Result<(Stream, SourceReader), Box<dyn Error>> {
    let ratio = sample_rate_reduction_ratio(config.sample_rate)?;
    let format = SourceFormat {
        channels: usize::from(config.channels),
        block_frames: block_frames(buffer_frames, ratio),
    };
    let (writer, reader) = CaptureWriter::new(config.sample_rate, format, links.clone())?;

    let stream = match sample_format {
        SampleFormat::F32 => open_typed::<f32>(device, config, writer),
        SampleFormat::I16 => open_typed::<i16>(device, config, writer),
        SampleFormat::U16 => open_typed::<u16>(device, config, writer),
        // ASIO drivers hand over 32-bit integers, 24-bit audio left-justified in each word.
        SampleFormat::I32 => open_typed::<i32>(device, config, writer),
        format => {
            return Err(
                format!("the selected input uses unsupported sample format {format:?}").into(),
            )
        }
    }?;
    Ok((stream, reader))
}

fn open_typed<T: SizedSample + CaptureSample>(
    device: &Device,
    config: &StreamConfig,
    mut writer: CaptureWriter,
) -> Result<Stream, Box<dyn Error>> {
    device
        .build_input_stream(
            *config,
            move |data: &[T], _info: &InputCallbackInfo| writer.write(data),
            // The driver may call this on its audio thread, so it stays out of the log sink
            // (which formats, locks and writes a file): it keeps the stderr line it always had.
            |error| eprintln!("Audio input error: {error}"),
            None,
        )
        .map_err(|error| format!("could not open PCM input stream: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_target_sample_rate_inside_supported_range() {
        assert!(supports_sample_rate(44_100, 192_000, TARGET_SAMPLE_RATE));
        assert!(supports_sample_rate(48_000, 48_000, TARGET_SAMPLE_RATE));
    }

    #[test]
    fn rejects_target_sample_rate_outside_supported_range() {
        assert!(!supports_sample_rate(8_000, 44_100, TARGET_SAMPLE_RATE));
        assert!(!supports_sample_rate(96_000, 192_000, TARGET_SAMPLE_RATE));
    }

    #[test]
    fn the_target_rate_beats_more_channels_at_a_reduced_rate() {
        assert!(
            input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::I16)
                < input_config_rank(TARGET_SAMPLE_RATE * 2, 8, SampleFormat::I16)
        );
    }

    #[test]
    fn more_input_channels_beat_a_stereo_config() {
        assert!(
            input_config_rank(TARGET_SAMPLE_RATE, 4, SampleFormat::F32)
                < input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::I16)
        );
        assert!(
            input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::I16)
                < input_config_rank(TARGET_SAMPLE_RATE, 1, SampleFormat::I16)
        );
    }

    #[test]
    fn a_config_over_the_mix_channel_limit_ranks_last() {
        let over_limit = (MAX_MIX_CHANNELS + 1) as u16;

        assert!(
            input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::U16)
                < input_config_rank(TARGET_SAMPLE_RATE, over_limit, SampleFormat::I16)
        );
        assert!(
            input_config_rank(
                TARGET_SAMPLE_RATE,
                MAX_MIX_CHANNELS as u16,
                SampleFormat::I16
            ) < input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::I16)
        );
    }

    #[test]
    fn the_sample_format_decides_between_equal_channel_counts() {
        assert!(
            input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::I16)
                < input_config_rank(TARGET_SAMPLE_RATE, 2, SampleFormat::F32)
        );
    }

    #[test]
    fn describe_input_device_reports_the_selected_channel_count() {
        assert_eq!(
            describe_input_device("Interface".to_owned(), Ok(4)),
            InputDevice {
                name: "Interface".to_owned(),
                channels: 4,
            }
        );
    }

    #[test]
    fn a_device_with_no_supported_input_reports_zero_channels() {
        let error: Box<dyn Error> = "no supported PCM input".into();

        let device = describe_input_device("Broken".to_owned(), Err(error));

        assert_eq!(device.channels, 0);
        assert_eq!(device.name, "Broken");
    }

    #[test]
    fn with_buffer_size_applies_a_fixed_size_only_when_requested() {
        let base = StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: BufferSize::Default,
        };

        assert_eq!(
            with_buffer_size(base.clone(), Some(480)).buffer_size,
            BufferSize::Fixed(480)
        );
        assert_eq!(
            with_buffer_size(base, None).buffer_size,
            BufferSize::Default
        );
    }

    #[test]
    fn describe_buffer_names_the_applied_size() {
        assert_eq!(describe_buffer(Some(480)), "Fixed(480)");
        assert_eq!(describe_buffer(None), "device default");
    }

    #[test]
    fn parses_installed_asio_driver_names_from_a_reg_query() {
        let text = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\ASIO\\Realtek ASIO\r\n\
                    HKEY_LOCAL_MACHINE\\SOFTWARE\\ASIO\\Universal Audio Volt\r\n\
                    HKEY_LOCAL_MACHINE\\SOFTWARE\\ASIO\\X-USB ASIO Driver\r\n";

        assert_eq!(
            parse_asio_driver_names(text),
            vec![
                "Realtek ASIO".to_owned(),
                "Universal Audio Volt".to_owned(),
                "X-USB ASIO Driver".to_owned(),
            ]
        );
    }

    #[test]
    fn ignores_unrelated_lines_and_a_missing_key() {
        assert!(parse_asio_driver_names("").is_empty());
        assert!(
            parse_asio_driver_names("ERROR: The system was unable to find the key\r\n").is_empty()
        );
    }

    #[test]
    fn clamps_a_requested_buffer_into_the_supported_range() {
        let single = SupportedBufferSize::Range {
            min: 1024,
            max: 1024,
        };
        assert_eq!(clamp_buffer_frames(256, &single), 1024);

        let range = SupportedBufferSize::Range { min: 128, max: 512 };
        assert_eq!(clamp_buffer_frames(256, &range), 256);
        assert_eq!(clamp_buffer_frames(64, &range), 128);
        assert_eq!(clamp_buffer_frames(4096, &range), 512);
        assert_eq!(clamp_buffer_frames(256, &SupportedBufferSize::Unknown), 256);
    }
}
