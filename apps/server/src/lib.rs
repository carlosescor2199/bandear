mod capture;
mod cli;
mod control;
mod decimate;
mod discovery;
mod engine;
mod health;
pub mod logging;
mod mix;
mod network;
mod output;
mod pipeline;
mod protocol;
mod recorder;
mod soundcheck;
mod targets;
mod wav;

pub use capture::{list_input_devices, list_input_devices_for, AudioHost, InputDevice};
pub use cli::{check_groups, parse_arguments, EngineConfig, GroupDefinition};
pub use engine::{
    check_channels, start, CaptureFormat, ChannelKind, ChannelMeta, EngineEvent, EngineHandle,
    EngineMeters, EngineStatus, GroupStatus, MirrorOutcome, MirrorSource, MixOrigin,
    MusicianCounters, MusicianMeters, MusicianStatus, MAX_CHANNEL_NAME_LENGTH,
};
pub use health::{LinkHealth, LinkStatus};
pub use mix::{EngineerControls, EngineerUpdate, MixPreset, MixUpdate, MixValues, PresetChannel};
pub use output::{LocalOutputRequest, LocalOutputState, LocalOutputStatus, OutputDevice};
pub use protocol::{
    CAPABILITY_PLAYBACK_BUFFER, CAPABILITY_REDUNDANCY_RECOVERED, CAPABILITY_STATS_V2,
    KNOWN_CAPABILITIES,
};
pub use recorder::{
    free_space, minutes_left, DiskEstimate, RecordRequest, RecordingState, RecordingStatus,
    TakeEnd, TakeMarker, TakeSummary, TakeTrack, TrackKind,
};
pub use soundcheck::{
    SoundcheckClock, SoundcheckError, SoundcheckRequest, SoundcheckState, SoundcheckStatus,
};
pub use targets::{MirrorStatus, MusicianKey};
pub use wav::{SampleFormat, WavError, WavReader, WavSpec, WavTarget, WavWriter};
