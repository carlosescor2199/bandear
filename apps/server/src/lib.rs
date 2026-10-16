mod capture;
mod cli;
mod control;
mod discovery;
mod engine;
mod health;
mod mix;
mod network;
mod protocol;
mod targets;

pub use capture::{list_input_devices, list_input_devices_for, AudioHost, InputDevice};
pub use cli::{check_groups, parse_arguments, EngineConfig, GroupDefinition};
pub use engine::{
    check_channels, start, CaptureFormat, ChannelKind, ChannelMeta, EngineEvent, EngineHandle,
    EngineMeters, EngineStatus, GroupStatus, MirrorOutcome, MirrorSource, MixOrigin,
    MusicianCounters, MusicianMeters, MusicianStatus, MAX_CHANNEL_NAME_LENGTH,
};
pub use health::{LinkHealth, LinkStatus};
pub use mix::{EngineerControls, EngineerUpdate, MixPreset, MixUpdate, MixValues, PresetChannel};
pub use targets::{MirrorStatus, MusicianKey};
