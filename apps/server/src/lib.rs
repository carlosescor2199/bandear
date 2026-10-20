#[doc(hidden)]
pub mod bench;
mod capture;
mod cli;
mod control;
mod decimate;
mod demo;
mod discovery;
// Channel processing. The master limiter is not on the audio path yet.
#[allow(dead_code)]
mod dsp;
mod engine;
mod health;
mod link_history;
mod load;
pub mod logging;
mod mix;
mod network;
mod output;
mod pipeline;
mod playback;
mod processing;
mod protocol;
mod recorder;
mod redundancy;
mod soundcheck;
mod targets;
mod wav;

pub use capture::{list_input_devices, list_input_devices_for, AudioHost, InputDevice};
pub use cli::{check_groups, parse_arguments, EngineConfig, GroupDefinition};
pub use demo::{DEMO_CHANNELS, DEMO_DEVICE_NAME, DEMO_VOICES};
pub use dsp::dynamics::{CompressorSettings, GateSettings, LimiterSettings};
pub use dsp::eq::{EqBandSettings, EqSettings, HighPassSettings};
pub use dsp::strip::StripSettings;
pub use engine::{
    check_channels, start, CaptureFormat, ChannelKind, ChannelMeta, EngineEvent, EngineHandle,
    EngineMeters, EngineStatus, GroupStatus, LimiterSetting, MirrorOutcome, MirrorSource,
    MixOrigin, MusicianCounters, MusicianMeters, MusicianStatus, MAX_CHANNEL_NAME_LENGTH,
};
pub use health::{LinkHealth, LinkStatus};
pub use link_history::{LinkHistoryRead, LinkSample, RoundTrip, HISTORY_SAMPLES};
pub use load::EngineLoadStatus;
pub use mix::{EngineerControls, EngineerUpdate, MixPreset, MixUpdate, MixValues, PresetChannel};
pub use output::{LocalOutputRequest, LocalOutputState, LocalOutputStatus, OutputDevice};
pub use playback::{
    BufferOutcome, BufferSetting, PlaybackBuffer, MAX_FIXED_BUFFER_MS, MIN_FIXED_BUFFER_MS,
};
pub use protocol::{
    CAPABILITY_PLAYBACK_BUFFER, CAPABILITY_REDUNDANCY_HOLD, CAPABILITY_REDUNDANCY_RECOVERED,
    CAPABILITY_STATS_V2, KNOWN_CAPABILITIES,
};
pub use recorder::{
    free_space, minutes_left, DiskEstimate, RecordRequest, RecordingState, RecordingStatus,
    TakeEnd, TakeMarker, TakeSummary, TakeTrack, TrackKind,
};
pub use redundancy::{
    Redundancy, RedundancyOutcome, RedundancySetting, DEFAULT_REDUNDANCY_DELAY_BLOCKS,
    MAX_REDUNDANCY_DELAY_BLOCKS, MIN_REDUNDANCY_DELAY_BLOCKS,
};
pub use soundcheck::{
    SoundcheckClock, SoundcheckError, SoundcheckRequest, SoundcheckState, SoundcheckStatus,
};
pub use targets::{MirrorStatus, MusicianKey};
pub use wav::{SampleFormat, WavError, WavReader, WavSpec, WavTarget, WavWriter};
