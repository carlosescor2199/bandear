//! The JSON contract between the webview and the engine.
//!
//! The engine's own types are not serialized directly: this module owns the wire shape the window
//! consumes, so the engine keeps no presentation concerns and can change without breaking the UI.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use personal_monitoring::{
    CaptureFormat, EngineMeters, EngineStatus, EngineerControls, EngineerUpdate, InputDevice,
    LinkHealth, MirrorStatus, MixOrigin, MixUpdate, MixValues, MusicianKey, MusicianStatus,
};

use crate::session::{address_key, client_key, SessionChannel, SessionGroup, SessionPreset};
use crate::sessions::SnapshotSummary;

/// The start-up configuration the webview sends to [crate::commands::start_engine].
///
/// Every field is optional so the window can send a partial object and let the defaults fill in.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    #[serde(default)]
    pub device_filter: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub control_port: Option<u16>,
    #[serde(default)]
    pub groups: Vec<GroupRequest>,
    /// The capture backend: "auto", "asio" or "wasapi". `None` means "auto", the platform's own
    /// host; only an explicit "asio" loads an ASIO driver.
    #[serde(default)]
    pub host: Option<String>,
    /// The capture buffer size in frames. `None` means the engine default.
    #[serde(default)]
    pub buffer_frames: Option<u32>,
}

/// One group as the window describes it. Channel numbers are 1-based, exactly as the CLI takes
/// them, because the conversion to the protocol's 0-based indices happens in the engine.
///
/// `id` is the group's stable id as the status reports it. A group the window has just created has
/// none, and the desk gives it a new one (see [crate::commands::set_groups]); from then on the
/// window sends it back, so every musician's level for the group follows it through renames,
/// moves and new channels.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub channels: Vec<usize>,
}

/// What [crate::commands::set_groups] did with an accepted list of groups.
///
/// `applied` says whether a running engine took the groups (every phone has them now); `saved`
/// whether the session keeps them. With no engine running they are saved and apply at the next
/// start (`applied: false, saved: true`). A running engine that took them while the save failed
/// gives `applied: true, saved: false` with the reason in `saveError`. A list that is refused, by
/// the check or by an engine that is stopping, is an error instead, and nothing changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupsSetDto {
    /// The groups in their order, each with its id (a new group's is the one the desk gave it).
    pub groups: Vec<SessionGroup>,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
}

/// One input's details as the window describes them, in a list in channel order (the first entry
/// is CH 1); see [crate::commands::set_channels].
///
/// `kind` is one of the engine's instrument kinds (`voice`, `guitar`, `bass`, `keys`, `drums`,
/// `percussion`, `horns`, `strings`, `playback`, `click`, `other`); `link` makes this input and the
/// next one a stereo pair. Every field may be left out and then has its default (no name, `other`,
/// shown, on its own). A field the desk does not know is rejected, so a misspelt name fails loudly
/// rather than silently changing nothing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default = "other_kind")]
    pub kind: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub link: bool,
}

fn other_kind() -> String {
    personal_monitoring::ChannelKind::Other.name().to_owned()
}

/// What [crate::commands::set_channels] did with an accepted list of channel details, as
/// [GroupsSetDto] says it for groups: `applied` whether a running engine took them (every phone has
/// them now), `saved` whether the session keeps them, and `saveError` why not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsSetDto {
    /// The details as kept, in channel order, names trimmed.
    pub channels: Vec<SessionChannel>,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
}

/// The audio input the window ran an engine with, to keep for the next start (see
/// [crate::commands::set_audio_settings]): `{ host, device, bufferFrames, asioDriver }`.
///
/// `host` is `auto`, `wasapi` or `asio`; `device` the input picked under `auto` or `wasapi` (none
/// for the system's default); `asioDriver` the ASIO driver the engineer picked, required with
/// `asio`. A field the desk does not know is rejected, as for [ChannelRequest].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioSettingsRequest {
    #[serde(default = "auto_host")]
    pub host: String,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub buffer_frames: Option<u32>,
    #[serde(default)]
    pub asio_driver: Option<String>,
}

fn auto_host() -> String {
    "auto".to_owned()
}

/// The capture format the engine selected, in plain values a webview can render.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFormatDto {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: String,
    pub buffer_size: String,
}

impl From<&CaptureFormat> for CaptureFormatDto {
    fn from(format: &CaptureFormat) -> Self {
        Self {
            channels: format.channels,
            sample_rate: format.sample_rate,
            sample_format: format.sample_format.clone(),
            buffer_size: format.buffer_size.clone(),
        }
    }
}

impl CaptureFormatDto {
    /// A one-line description in the same words the CLI prints at startup.
    pub fn describe(&self) -> String {
        format!(
            "{} ch @ {} Hz, {}, buffer {}",
            self.channels, self.sample_rate, self.sample_format, self.buffer_size
        )
    }
}

/// What [crate::commands::start_engine] returns: the two values T3 asks the window to show.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSummary {
    /// The engine run this start began (see [crate::commands::DesktopState::run]).
    pub run: u64,
    pub device_name: String,
    pub capture_format: CaptureFormatDto,
}

/// One musician row: the engine's target/control join, flattened for JSON.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicianDto {
    pub address: String,
    /// The name the musician typed on their phone and announced when registering, straight from the
    /// engine. It is client input, never a desktop label, and is never sent back to a client.
    pub client_name: Option<String>,
    /// The stable id the musician's phone announced, straight from the engine; null for a
    /// configured target and a phone that sent none. It survives a change of IP, so the desk keys
    /// what it keeps about a musician (its label) on it rather than on the address.
    pub client_id: Option<String>,
    /// The name this desk shows: the client's own announced name when it has one, otherwise the
    /// engineer's local label. Filled by the command layer; the engine knows nothing of the label.
    pub name: Option<String>,
    /// True when `name` is the desk's local label rather than the client's announced name, so the
    /// window can mark it as local. It is only ever true when the client announced no name; a
    /// client that announced one always wins, which is why the two cases need telling apart.
    pub name_is_local: bool,
    pub control_connected: bool,
    /// The musician's whole mix, flat on the row so `volumePercent`, `maxLevelPercent` and `muted`
    /// keep the names the window already reads.
    #[serde(flatten)]
    pub mix: MixDto,
    pub packets_sent: u64,
    pub packets_discarded: u64,
    /// The phone's link health as the server classified it: "unknown", "good", "warning" or "bad".
    pub link_health: &'static str,
    /// The rates the health was judged on. All four are absent when there is no fresh report: a
    /// phone that never reported, one that left, or a lost link ("bad" with no numbers to show).
    ///
    /// Packets the phone received per second; 0 means it hears no audio at all.
    pub packets_per_second: Option<f64>,
    pub underruns_per_second: Option<f64>,
    /// Lost packets as a percentage of the packets the phone expected.
    pub loss_percent: Option<f64>,
    pub receive_timeouts_per_second: Option<f64>,
    /// Whose mix this musician's follows (see [crate::commands::set_mirror]); null when it
    /// follows nobody.
    pub mirror: Option<MirrorDto>,
}

/// Whose mix a musician's follows, on their status row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorDto {
    /// The leader as the desk keys them: `client:<id>` for a phone with a client id, else
    /// `address:<ip:port>` (see [crate::session::client_key]).
    pub leader: String,
    /// The leader's address as the status rows show it; null while the leader is not here, and
    /// then the follower keeps the last mix they took.
    pub leader_address: Option<String>,
    /// The name the follower's phone is told it follows.
    pub name: String,
}

/// The key the desk keeps what it knows about `key`'s musician under.
pub fn session_key(key: &MusicianKey) -> String {
    match key {
        MusicianKey::Client(id) => client_key(id),
        MusicianKey::Address(address) => address_key(&address.to_string()),
    }
}

impl From<&MirrorStatus> for MirrorDto {
    fn from(mirror: &MirrorStatus) -> Self {
        Self {
            leader: session_key(&mirror.leader),
            leader_address: mirror.leader_address.map(|address| address.to_string()),
            name: mirror.name.clone(),
        }
    }
}

/// What a change to the engineer's presets did ([crate::commands::save_preset],
/// [crate::commands::rename_preset], [crate::commands::delete_preset]): the preset saved or
/// renamed (null after a delete) and every preset now kept, in order. A refused change or a save
/// that fails is an error instead, and nothing changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetsDto {
    pub preset: Option<SessionPreset>,
    pub presets: Vec<SessionPreset>,
}

/// One musician's mix after a desk change of several ([crate::commands::apply_preset],
/// [crate::commands::copy_mix]).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicianMixDto {
    /// The musician's address as the status rows show it.
    pub address: String,
    pub mix: MixDto,
}

/// What a preset or a copy did: each musician's mix now stored, in the order named. The running
/// engine took them all (each phone is sent its mix); a refusal is an error and changed nobody.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixesSetDto {
    pub mixes: Vec<MusicianMixDto>,
}

impl MixesSetDto {
    pub fn new(mixes: &[(std::net::SocketAddr, MixValues)]) -> Self {
        Self {
            mixes: mixes
                .iter()
                .map(|(address, mix)| MusicianMixDto {
                    address: address.to_string(),
                    mix: MixDto::from(mix),
                })
                .collect(),
        }
    }
}

/// The musician a mirror should follow, as the webview sends it to [crate::commands::set_mirror]:
/// their address as the status rows show it, their phone's client id when the row has one, and the
/// name the follower's phone is told ("Following Bass's mix"; the desk sends the name it shows).
/// A field the desk does not know is rejected.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MirrorRequest {
    pub address: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub name: String,
}

/// What [crate::commands::set_mirror] did, as [EngineerControlsSetDto] says it for the engineer's
/// controls: `applied` whether the running engine took the mirror (the follower's mix follows the
/// leader's now and their phone is held off its channels and groups), `saved` whether the session
/// keeps it, and `saveError` why not. With no engine running it is saved and applies at the next
/// start.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorSetDto {
    /// The follower's address as the status rows show it.
    pub address: String,
    /// The leader's address as it was asked for; null once the mirror stopped.
    pub leader: Option<String>,
    /// The name the follower's phone is told it follows; null once the mirror stopped.
    pub name: Option<String>,
    /// "phone" when the mirror is kept for the follower's phone (it follows them to a new IP),
    /// "address" when it is kept for their address.
    pub kept_for: &'static str,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
    /// The follower's whole mix now stored; null when no engine took the mirror.
    pub mix: Option<MixDto>,
}

/// What [crate::commands::save_session] saved: the named session as the window lists it, and
/// whether an engine ran, so the musicians' mixes were saved with it (`session.mixes` of them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSavedDto {
    pub session: SnapshotSummary,
    pub engine_running: bool,
}

/// One musician's whole mix.
///
/// The per-channel tables always hold the engine's full 32 entries (16 for groups), index-aligned
/// with the 0-based source channels and the group list; the window shows as many as the current
/// device has. Entries past that are the stored values for channels the device lacks.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixDto {
    /// The stored version of the mix: every change has a higher one. A status row, a
    /// `mix-changed` event and a `set_mix` answer can reach the window in any order, so the window
    /// keeps the copy with the highest revision.
    pub revision: u64,
    pub volume_percent: u8,
    pub max_level_percent: u8,
    pub muted: bool,
    pub channel_gains: Vec<u8>,
    pub pans: Vec<u8>,
    pub channel_muted: Vec<bool>,
    pub channel_solo: Vec<bool>,
    pub group_levels: Vec<u8>,
    /// The engineer's limit on what the musician hears, 0-100 (100 is no limit), apart from the
    /// phone's own `maxLevelPercent`: the engine plays at most the lower of the two. Only
    /// [crate::commands::set_engineer_controls] changes it; `set_mix` cannot.
    pub engineer_limit_percent: u8,
    /// One entry per channel, index-aligned with `channelGains`: `true` for a channel the engineer
    /// locked, whose level, pan, mute and solo the musician's phone cannot change (the desk still
    /// can). Both sides of a stereo pair are locked together.
    pub locked_channels: Vec<bool>,
}

impl From<&MixValues> for MixDto {
    fn from(mix: &MixValues) -> Self {
        Self {
            revision: mix.revision,
            volume_percent: mix.volume_percent,
            max_level_percent: mix.max_level_percent,
            muted: mix.muted,
            channel_gains: mix.channel_gains.to_vec(),
            pans: mix.pans.to_vec(),
            channel_muted: mix.channel_muted.to_vec(),
            channel_solo: mix.channel_solo.to_vec(),
            group_levels: mix.group_levels.to_vec(),
            engineer_limit_percent: mix.engineer.limit_percent,
            locked_channels: locked_table(&mix.engineer, mix.channel_gains.len()),
        }
    }
}

/// One lock per channel for `channels` channels, index-aligned with the mix's channel tables.
pub fn locked_table(controls: &EngineerControls, channels: usize) -> Vec<bool> {
    (0..channels)
        .map(|channel| controls.is_locked(channel))
        .collect()
}

/// A change to the engineer's controls for one musician that the webview sends to
/// [crate::commands::set_engineer_controls].
///
/// Both fields are optional and an absent one keeps its value. `limitPercent` is 0-100 (clamped);
/// `lockedChannels` replaces each channel's lock from CH 1 on (entry 0 is CH 1) and keeps the rest,
/// as a `set_mix` table does, at most one entry per channel. A field the desk does not know is
/// rejected.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineerControlsRequest {
    #[serde(default)]
    pub limit_percent: Option<i32>,
    #[serde(default)]
    pub locked_channels: Option<Vec<bool>>,
}

impl From<EngineerControlsRequest> for EngineerUpdate {
    fn from(request: EngineerControlsRequest) -> Self {
        Self {
            limit_percent: request.limit_percent,
            locked: request.locked_channels,
        }
    }
}

/// What [crate::commands::set_engineer_controls] did, as [GroupsSetDto] says it for groups:
/// `applied` whether the running engine took the controls (the musician's phone has them now),
/// `saved` whether the session keeps them, and `saveError` why not. With no engine running they are
/// saved for the address and apply at the next start.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineerControlsSetDto {
    /// The musician's address as the status rows show it.
    pub address: String,
    /// The controls now kept: the limit (0-100) and one lock per channel, index-aligned with the
    /// mix's channel tables.
    pub limit_percent: u8,
    pub locked_channels: Vec<bool>,
    /// "phone" when they are kept for the musician's phone (its client id), so they follow it to a
    /// new IP; "address" when they are kept for the address.
    pub kept_for: &'static str,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
    /// The musician's whole mix now stored, with the new controls; null when no engine took them.
    pub mix: Option<MixDto>,
}

/// The payload of the `mix-changed` event: a musician's whole mix right after a change.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixChangedDto {
    /// The engine run the change happened in.
    pub run: u64,
    /// The musician's address as the status rows show it, whoever made the change.
    pub address: String,
    /// "phone" for a change from the musician's phone, "desktop" for one made here.
    pub origin: &'static str,
    /// The whole mix now stored, nested rather than flat as on a status row.
    pub mix: MixDto,
}

impl MixChangedDto {
    pub fn new(address: SocketAddr, origin: MixOrigin, mix: &MixValues, run: u64) -> Self {
        Self {
            run,
            address: address.to_string(),
            origin: match origin {
                MixOrigin::Client { .. } => "phone",
                // The engine API is how this desk changes a mix; nothing else calls it here.
                MixOrigin::Engine => "desktop",
            },
            mix: MixDto::from(mix),
        }
    }
}

/// The payload of the `meters` event: the live levels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetersDto {
    /// The engine run the levels were read from; 0 until the reader sets it.
    pub run: u64,
    /// One 0-100 input peak per source channel, index-aligned with the status's `channels`.
    pub channels: Vec<u8>,
    /// Each musician's 0-100 output peak, keyed by the address the status rows show.
    pub musicians: BTreeMap<String, OutputLevelsDto>,
}

/// The 0-100 peak of the stereo mix one musician is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct OutputLevelsDto {
    pub left: u8,
    pub right: u8,
}

impl From<&EngineMeters> for MetersDto {
    fn from(meters: &EngineMeters) -> Self {
        Self {
            run: 0,
            channels: meters.channel_levels.clone(),
            musicians: meters
                .musicians
                .iter()
                .map(|musician| {
                    let [left, right] = musician.output_levels;
                    (
                        musician.address.to_string(),
                        OutputLevelsDto { left, right },
                    )
                })
                .collect(),
        }
    }
}

/// A partial mix change the webview sends to [crate::commands::set_mix].
///
/// Every field is optional and an absent one keeps its value. A list replaces the values from the
/// first channel (or group) on, so the window can send a whole table or just its first entries;
/// levels are 0-100 and are clamped by the engine. A field the engine does not know is rejected
/// instead of ignored, so a misspelt name fails loudly rather than silently changing nothing.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MixUpdateRequest {
    #[serde(default)]
    pub volume_percent: Option<i32>,
    #[serde(default)]
    pub max_level_percent: Option<i32>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub channel_gains: Option<Vec<i32>>,
    #[serde(default)]
    pub pans: Option<Vec<i32>>,
    #[serde(default)]
    pub channel_muted: Option<Vec<bool>>,
    #[serde(default)]
    pub channel_solo: Option<Vec<bool>>,
    #[serde(default)]
    pub group_levels: Option<Vec<i32>>,
}

impl From<MixUpdateRequest> for MixUpdate {
    fn from(request: MixUpdateRequest) -> Self {
        Self {
            channel_gains: request.channel_gains,
            pans: request.pans,
            channel_muted: request.channel_muted,
            channel_solo: request.channel_solo,
            group_levels: request.group_levels,
            volume_percent: request.volume_percent,
            max_level_percent: request.max_level_percent,
            muted: request.muted,
        }
    }
}

/// One configured group, with the same 0-based source indices the engine uses. Its position in
/// the list is the index of its level in every musician's `groupLevels`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    /// The group's stable id: what the window joins channels and edits on, never the name.
    pub id: String,
    pub name: String,
    pub channels: Vec<usize>,
    /// True when the current input device is missing a channel this group names. The group is still
    /// shown and kept; the window uses this to mark it and to warn before a further switch.
    pub invalid: bool,
}

/// One selectable input device: its name and the channel count a switch would capture.
///
/// `channels` of `0` means the device has no supported PCM input, so it is listed but not usable.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceDto {
    pub name: String,
    pub channels: u16,
}

impl From<&InputDevice> for InputDeviceDto {
    fn from(device: &InputDevice) -> Self {
        Self {
            name: device.name.clone(),
            channels: device.channels,
        }
    }
}

/// One source channel as the window shows it: a 1-based number, the engineer's details, its group
/// when the engineer configured one, and the live pre-mix peak.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    pub number: u8,
    /// The engineer's name for the input; empty when it has none, and the window then calls it by
    /// its number ("CH 3").
    pub name: String,
    /// The instrument kind's name (see [ChannelRequest]).
    pub kind: &'static str,
    /// Hidden inputs are heard in no mix; the window leaves them out of the mix.
    pub hidden: bool,
    /// This input and the next one are a stereo pair (set on the pair's first input): one strip
    /// whose level, mute and solo write both channels, which the engine keeps equal.
    pub link: bool,
    /// The name of the channel's group, for display.
    pub group: Option<String>,
    /// The id of the channel's group: two groups may share a name, so this is the join.
    pub group_id: Option<String>,
    pub level: u8,
}

impl From<&MusicianStatus> for MusicianDto {
    fn from(musician: &MusicianStatus) -> Self {
        Self {
            address: musician.address.to_string(),
            client_name: musician.name.clone(),
            client_id: musician.client_id.clone(),
            name: None,
            name_is_local: false,
            control_connected: musician.control_connected,
            mix: MixDto::from(&musician.mix),
            packets_sent: musician.counters.packets_sent,
            packets_discarded: musician.counters.packets_discarded,
            link_health: link_health_name(musician.link.health),
            packets_per_second: musician.link.packets_per_second,
            underruns_per_second: musician.link.underruns_per_second,
            loss_percent: musician.link.loss_percent,
            receive_timeouts_per_second: musician.link.receive_timeouts_per_second,
            mirror: musician.mirror.as_ref().map(MirrorDto::from),
        }
    }
}

/// The name of a link health level, in the lowercase the window matches on.
fn link_health_name(health: LinkHealth) -> &'static str {
    match health {
        LinkHealth::Unknown => "unknown",
        LinkHealth::Good => "good",
        LinkHealth::Warning => "warning",
        LinkHealth::Bad => "bad",
    }
}

/// What the `engine_status` command answers: whether an engine runs, and if so its status.
///
/// No engine running is an answer, not an error, so the window can tell it apart from a real
/// failure without matching error text: `{ "state": "notRunning", "run": n }` or the whole
/// [EngineStatusDto] with `"state": "running"`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum EngineStateDto {
    /// No engine runs; `run` is the last run begun (0 before the first start).
    NotRunning {
        run: u64,
    },
    Running(EngineStatusDto),
}

/// A snapshot of the engine, ready for the webview.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusDto {
    /// The engine run the snapshot was taken from, so the window can ignore a snapshot of an
    /// engine that has already stopped. 0 until the command layer sets it: the engine's own
    /// status knows nothing of runs.
    pub run: u64,
    pub device_name: String,
    pub capture_format: CaptureFormatDto,
    pub control_port: u16,
    pub sample_rate: u32,
    pub source_channels: u8,
    pub groups: Vec<GroupDto>,
    pub channels: Vec<ChannelDto>,
    pub stopped: bool,
    pub samples_received: u64,
    pub packets_sent: u64,
    pub packets_discarded: u64,
    pub musicians: Vec<MusicianDto>,
}

impl From<&EngineStatus> for EngineStatusDto {
    fn from(status: &EngineStatus) -> Self {
        let groups: Vec<GroupDto> = status
            .groups
            .iter()
            .map(|group| GroupDto {
                id: group.id.clone(),
                name: group.name.clone(),
                channels: group.channels.clone(),
                invalid: group.invalid,
            })
            .collect();
        // The engine publishes the group layout once; the per-channel membership the window needs
        // is derived here rather than being stored twice and risking the two views disagreeing.
        let channels = (0..usize::from(status.source_channels))
            .map(|index| {
                // An invalid group is shown in the group list but does not own any channel, because
                // the mixer ignores it; labelling a channel with it would overstate what is applied.
                let group = status
                    .groups
                    .iter()
                    .find(|group| !group.invalid && group.channels.contains(&index));
                let details = status.channels.get(index).cloned().unwrap_or_default();
                ChannelDto {
                    number: (index + 1) as u8,
                    name: details.name,
                    kind: details.kind.name(),
                    hidden: details.hidden,
                    link: details.linked_to_next,
                    group: group.map(|group| group.name.clone()),
                    group_id: group.map(|group| group.id.clone()),
                    level: status.channel_levels.get(index).copied().unwrap_or(0),
                }
            })
            .collect();
        Self {
            run: 0,
            device_name: status.device_name.clone(),
            capture_format: (&status.capture_format).into(),
            control_port: status.control_port,
            sample_rate: status.sample_rate,
            source_channels: status.source_channels,
            groups,
            channels,
            stopped: status.stopped,
            samples_received: status.samples_received,
            packets_sent: status.packets_sent,
            packets_discarded: status.packets_discarded,
            musicians: status.musicians.iter().map(MusicianDto::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use personal_monitoring::{
        CaptureFormat, EngineMeters, EngineStatus, GroupStatus, LinkHealth, LinkStatus, MixOrigin,
        MusicianMeters, MusicianStatus,
    };

    fn status_with(groups: Vec<GroupStatus>, channel_levels: Vec<u8>) -> EngineStatus {
        EngineStatus {
            device_name: "Test device".to_owned(),
            capture_format: CaptureFormat {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "I16".to_owned(),
                buffer_size: "Default".to_owned(),
            },
            control_port: 50001,
            sample_rate: 48_000,
            source_channels: 2,
            groups,
            channels: Vec::new(),
            channel_levels,
            stopped: false,
            samples_received: 0,
            packets_sent: 0,
            packets_discarded: 0,
            musicians: Vec::new(),
        }
    }

    #[test]
    fn channels_label_by_number_with_their_group_and_level() {
        let status = status_with(
            vec![GroupStatus {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![0],
                invalid: false,
            }],
            vec![50, 100],
        );

        let dto = EngineStatusDto::from(&status);

        assert_eq!(dto.channels.len(), 2);
        assert_eq!(dto.channels[0].number, 1);
        assert_eq!(dto.channels[0].group.as_deref(), Some("Drums"));
        assert_eq!(dto.channels[0].level, 50);
        assert_eq!(dto.channels[1].number, 2);
        assert_eq!(dto.channels[1].group, None);
        assert_eq!(dto.channels[1].level, 100);
        assert_eq!(dto.groups.len(), 1);
        assert_eq!(dto.groups[0].channels, vec![0]);
    }

    #[test]
    fn a_channel_without_a_group_reports_no_group() {
        let status = status_with(Vec::new(), vec![0, 0]);

        let dto = EngineStatusDto::from(&status);

        assert!(dto.channels.iter().all(|channel| channel.group.is_none()));
        assert!(dto.groups.is_empty());
    }

    #[test]
    fn an_invalid_group_is_flagged_for_the_window_and_keeps_its_channels() {
        let status = status_with(
            vec![GroupStatus {
                id: "vox".to_owned(),
                name: "Vocals".to_owned(),
                channels: vec![2, 3],
                invalid: true,
            }],
            vec![0, 0],
        );

        let dto = EngineStatusDto::from(&status);

        assert!(dto.groups[0].invalid);
        assert_eq!(dto.groups[0].channels, vec![2, 3]);
    }

    #[test]
    fn the_engine_name_becomes_the_client_name_and_the_label_is_left_to_the_command_layer() {
        let musician = MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: Some("Ana".to_owned()),
            client_id: None,
            control_connected: false,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
            mirror: None,
        };

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.client_name.as_deref(), Some("Ana"));
        assert_eq!(dto.name, None);
        assert!(!dto.name_is_local);
    }

    #[test]
    fn a_musician_that_never_announced_a_name_reports_no_client_name() {
        let musician = MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: None,
            client_id: None,
            control_connected: false,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
            mirror: None,
        };

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.client_name, None);
        assert_eq!(dto.name, None);
        assert!(!dto.name_is_local);
    }

    #[test]
    fn the_phones_client_id_reaches_the_window_and_is_null_without_one() {
        let phone = MusicianStatus {
            client_id: Some("0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b".to_owned()),
            ..musician_with_link(LinkStatus::default())
        };
        let rack = musician_with_link(LinkStatus::default());

        let phone_row = serde_json::to_value(MusicianDto::from(&phone)).expect("a row serializes");
        let rack_row = serde_json::to_value(MusicianDto::from(&rack)).expect("a row serializes");

        assert_eq!(
            phone_row["clientId"],
            "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b"
        );
        assert!(
            rack_row["clientId"].is_null(),
            "a configured target or an older phone has no id: {rack_row}"
        );
    }

    #[test]
    fn a_row_says_whose_mix_the_musician_follows_and_whether_the_leader_is_here() {
        let follower = MusicianStatus {
            mirror: Some(MirrorStatus {
                leader: MusicianKey::Client("0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b".to_owned()),
                name: "Bass".to_owned(),
                leader_address: Some("192.168.1.12:50000".parse().expect("test address")),
            }),
            ..musician_with_link(LinkStatus::default())
        };
        let away = MusicianStatus {
            mirror: Some(MirrorStatus {
                leader: MusicianKey::Address("192.168.1.40:50000".parse().expect("test address")),
                name: "Rack".to_owned(),
                leader_address: None,
            }),
            ..musician_with_link(LinkStatus::default())
        };

        let row = serde_json::to_value(MusicianDto::from(&follower)).expect("a row serializes");
        let away_row = serde_json::to_value(MusicianDto::from(&away)).expect("a row serializes");
        let free = serde_json::to_value(MusicianDto::from(&musician_with_link(
            LinkStatus::default(),
        )))
        .expect("a row serializes");

        assert_eq!(
            row["mirror"],
            serde_json::json!({
                "leader": "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b",
                "leaderAddress": "192.168.1.12:50000",
                "name": "Bass"
            })
        );
        assert_eq!(away_row["mirror"]["leader"], "address:192.168.1.40:50000");
        assert!(
            away_row["mirror"]["leaderAddress"].is_null(),
            "the leader is away"
        );
        assert!(free["mirror"].is_null(), "{free}");
        let misspelt = serde_json::from_value::<MirrorRequest>(
            serde_json::json!({ "address": "192.168.1.12:50000", "leaderName": "Bass" }),
        );
        assert!(misspelt.is_err(), "an unknown field is refused");
    }

    fn musician_with_link(link: LinkStatus) -> MusicianStatus {
        MusicianStatus {
            address: "192.168.1.30:50000".parse().expect("test address"),
            name: None,
            client_id: None,
            control_connected: true,
            mix: Default::default(),
            counters: Default::default(),
            link,
            mirror: None,
        }
    }

    #[test]
    fn each_link_health_level_reaches_the_window_as_its_lowercase_name() {
        for (health, name) in [
            (LinkHealth::Unknown, "unknown"),
            (LinkHealth::Good, "good"),
            (LinkHealth::Warning, "warning"),
            (LinkHealth::Bad, "bad"),
        ] {
            let musician = musician_with_link(LinkStatus {
                health,
                ..LinkStatus::default()
            });

            assert_eq!(MusicianDto::from(&musician).link_health, name);
        }
    }

    #[test]
    fn the_rates_the_health_was_judged_on_are_passed_through() {
        let musician = musician_with_link(LinkStatus {
            health: LinkHealth::Warning,
            packets_per_second: Some(187.5),
            underruns_per_second: Some(0.5),
            loss_percent: Some(1.25),
            receive_timeouts_per_second: Some(0.0),
        });

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.link_health, "warning");
        assert_eq!(dto.packets_per_second, Some(187.5));
        assert_eq!(dto.underruns_per_second, Some(0.5));
        assert_eq!(dto.loss_percent, Some(1.25));
        assert_eq!(dto.receive_timeouts_per_second, Some(0.0));
    }

    #[test]
    fn a_musician_without_a_fresh_report_has_no_rates() {
        let never_reported = MusicianDto::from(&musician_with_link(LinkStatus::default()));
        let lost = MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            ..LinkStatus::default()
        }));

        assert_eq!(never_reported.link_health, "unknown");
        assert_eq!(
            lost.link_health, "bad",
            "a lost link is bad with no numbers"
        );
        for dto in [never_reported, lost] {
            assert_eq!(dto.packets_per_second, None);
            assert_eq!(dto.underruns_per_second, None);
            assert_eq!(dto.loss_percent, None);
            assert_eq!(dto.receive_timeouts_per_second, None);
        }
    }

    #[test]
    fn the_link_fields_are_camel_case_json_with_absent_rates_as_null() {
        let lost = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            ..LinkStatus::default()
        })))
        .expect("a musician serializes");
        let reporting = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Good,
            packets_per_second: Some(200.0),
            underruns_per_second: Some(0.0),
            loss_percent: Some(0.25),
            receive_timeouts_per_second: Some(0.0),
        })))
        .expect("a musician serializes");
        let silent = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            packets_per_second: Some(0.0),
            underruns_per_second: Some(0.0),
            loss_percent: Some(0.0),
            receive_timeouts_per_second: Some(0.0),
        })))
        .expect("a musician serializes");

        assert_eq!(lost["linkHealth"], "bad");
        assert!(lost["packetsPerSecond"].is_null());
        assert!(lost["underrunsPerSecond"].is_null());
        assert!(lost["lossPercent"].is_null());
        assert!(lost["receiveTimeoutsPerSecond"].is_null());
        assert_eq!(reporting["linkHealth"], "good");
        assert_eq!(reporting["packetsPerSecond"], 200.0);
        assert_eq!(reporting["underrunsPerSecond"], 0.0);
        assert_eq!(reporting["lossPercent"], 0.25);
        assert_eq!(reporting["receiveTimeoutsPerSecond"], 0.0);
        assert_eq!(silent["linkHealth"], "bad");
        assert_eq!(
            silent["packetsPerSecond"], 0.0,
            "a phone hearing nothing is a number the window can tell from a lost link"
        );
    }

    #[test]
    fn the_full_mix_reaches_the_window_in_camel_case() {
        let mut mix = MixValues {
            volume_percent: 42,
            max_level_percent: 90,
            muted: true,
            ..MixValues::default()
        };
        mix.channel_gains[1] = 30;
        mix.pans[0] = 50;
        mix.channel_muted[2] = true;
        mix.channel_solo[3] = true;
        mix.group_levels[0] = 70;
        let musician = MusicianStatus {
            mix,
            ..musician_with_link(LinkStatus::default())
        };

        let dto = MusicianDto::from(&musician);
        let json = serde_json::to_value(&dto).expect("a musician serializes");

        assert_eq!(dto.mix.channel_gains, mix.channel_gains.to_vec());
        assert_eq!(dto.mix.pans, mix.pans.to_vec());
        assert_eq!(dto.mix.channel_muted, mix.channel_muted.to_vec());
        assert_eq!(dto.mix.channel_solo, mix.channel_solo.to_vec());
        assert_eq!(dto.mix.group_levels, mix.group_levels.to_vec());
        // The mix sits flat on the row, so the fields the window already reads keep their names.
        assert_eq!(json["volumePercent"], 42);
        assert_eq!(json["maxLevelPercent"], 90);
        assert_eq!(json["muted"], true);
        assert_eq!(json["channelGains"][1], 30);
        assert_eq!(json["pans"][0], 50);
        assert_eq!(json["channelMuted"][2], true);
        assert_eq!(json["channelSolo"][3], true);
        assert_eq!(json["groupLevels"][0], 70);
        assert!(json.get("mix").is_none(), "no nested object: {json}");
    }

    #[test]
    fn the_mixs_revision_reaches_the_window_on_rows_changes_and_answers() {
        let mix = MixValues {
            revision: 41,
            ..MixValues::default()
        };
        let musician = MusicianStatus {
            mix,
            ..musician_with_link(LinkStatus::default())
        };
        let address = "192.168.1.3:50000".parse().expect("test address");

        let row = serde_json::to_value(MusicianDto::from(&musician)).expect("a row serializes");
        let change = serde_json::to_value(MixChangedDto::new(address, MixOrigin::Engine, &mix, 1))
            .expect("a change serializes");
        let answer = serde_json::to_value(MixDto::from(&mix)).expect("an answer serializes");

        assert_eq!(row["revision"], 41, "flat on a status row");
        assert_eq!(change["mix"]["revision"], 41, "nested in a mix change");
        assert_eq!(answer["revision"], 41, "in the set_mix answer");
    }

    #[test]
    fn the_engineers_limit_and_locks_reach_the_window_on_rows_changes_and_answers() {
        let mix = MixValues {
            max_level_percent: 90,
            engineer: EngineerControls {
                limit_percent: 60,
                locked: 0b101,
            },
            ..MixValues::default()
        };
        let musician = MusicianStatus {
            mix,
            ..musician_with_link(LinkStatus::default())
        };
        let address = "192.168.1.3:50000".parse().expect("test address");

        let row = serde_json::to_value(MusicianDto::from(&musician)).expect("a row serializes");
        let change = serde_json::to_value(MixChangedDto::new(address, MixOrigin::Engine, &mix, 1))
            .expect("a change serializes");
        let answer = serde_json::to_value(MixDto::from(&mix)).expect("an answer serializes");

        for (copy, json) in [
            ("a status row", &row),
            ("a mix change", &change["mix"]),
            ("a set_mix answer", &answer),
        ] {
            assert_eq!(json["engineerLimitPercent"], 60, "{copy}");
            assert_eq!(
                json["maxLevelPercent"], 90,
                "{copy}: the phone's cap is apart"
            );
            let locked: Vec<bool> =
                serde_json::from_value(json["lockedChannels"].clone()).expect("a table of locks");
            assert_eq!(locked.len(), 32, "{copy}: one per channel");
            assert_eq!(&locked[..4], &[true, false, true, false], "{copy}");
        }
    }

    #[test]
    fn an_engineer_controls_request_reads_camel_case_and_refuses_unknown_fields() {
        let request: EngineerControlsRequest =
            serde_json::from_str(r#"{"limitPercent":60,"lockedChannels":[false,true]}"#)
                .expect("a change parses");

        assert_eq!(
            EngineerUpdate::from(request),
            EngineerUpdate {
                limit_percent: Some(60),
                locked: Some(vec![false, true]),
            }
        );
        let partial: EngineerControlsRequest =
            serde_json::from_str(r#"{"limitPercent":70}"#).expect("a partial change parses");
        assert_eq!(partial.locked_channels, None, "absent means unchanged");
        let error = serde_json::from_str::<EngineerControlsRequest>(r#"{"limit":60}"#)
            .expect_err("a misspelt field must not be a silent no-op");
        assert!(error.to_string().contains("limit"), "names it: {error}");
        assert!(
            serde_json::from_str::<MixUpdateRequest>(r#"{"engineerLimitPercent":100}"#).is_err(),
            "set_mix cannot change the engineer's controls"
        );
    }

    #[test]
    fn a_mix_request_reads_camel_case_and_leaves_absent_fields_unset() {
        let request: MixUpdateRequest =
            serde_json::from_str(r#"{"pans":[10,90],"volumePercent":80,"channelSolo":[true]}"#)
                .expect("a partial mix parses");

        let update = MixUpdate::from(request);

        assert_eq!(
            update,
            MixUpdate {
                pans: Some(vec![10, 90]),
                volume_percent: Some(80),
                channel_solo: Some(vec![true]),
                ..MixUpdate::default()
            }
        );
    }

    #[test]
    fn a_mix_request_with_an_unknown_field_is_rejected_instead_of_ignored() {
        let error = serde_json::from_str::<MixUpdateRequest>(r#"{"volume":80}"#)
            .expect_err("a misspelt field must not be a silent no-op");

        assert!(error.to_string().contains("volume"), "names it: {error}");
    }

    #[test]
    fn input_device_reports_its_name_and_channel_count() {
        let device = InputDevice {
            name: "Interface".to_owned(),
            channels: 6,
        };

        let dto = InputDeviceDto::from(&device);

        assert_eq!(dto.name, "Interface");
        assert_eq!(dto.channels, 6);
    }

    #[test]
    fn meters_reach_the_window_with_output_peaks_keyed_by_address() {
        let meters = EngineMeters {
            channel_levels: vec![50, 0],
            musicians: vec![
                MusicianMeters {
                    address: "192.168.1.4:50000".parse().expect("test address"),
                    output_levels: [0, 7],
                },
                MusicianMeters {
                    address: "192.168.1.3:50000".parse().expect("test address"),
                    output_levels: [100, 50],
                },
            ],
        };

        let json = serde_json::to_value(MetersDto::from(&meters)).expect("meters serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "run": 0,
                "channels": [50, 0],
                "musicians": {
                    "192.168.1.3:50000": { "left": 100, "right": 50 },
                    "192.168.1.4:50000": { "left": 0, "right": 7 },
                },
            })
        );
    }

    #[test]
    fn a_mix_change_names_its_origin_and_carries_the_whole_mix_nested() {
        let mix = MixValues {
            volume_percent: 42,
            ..MixValues::default()
        };
        let address = "192.168.1.3:50000".parse().expect("test address");
        let peer = "192.168.1.3:41000".parse().expect("test address");

        let from_phone = MixChangedDto::new(address, MixOrigin::Client { peer }, &mix, 1);
        let from_desktop = MixChangedDto::new(address, MixOrigin::Engine, &mix, 1);
        let json = serde_json::to_value(&from_phone).expect("a change serializes");

        assert_eq!(from_phone.origin, "phone");
        assert_eq!(from_desktop.origin, "desktop");
        assert_eq!(json["address"], "192.168.1.3:50000");
        assert_eq!(json["origin"], "phone");
        assert_eq!(json["mix"]["volumePercent"], 42);
        assert_eq!(
            json["mix"]["channelGains"].as_array().map(Vec::len),
            Some(32)
        );
        assert!(
            json.get("volumePercent").is_none(),
            "the mix is nested: {json}"
        );
    }

    #[test]
    fn every_payload_names_the_engine_run_it_belongs_to() {
        let mut status = EngineStatusDto::from(&status_with(Vec::new(), vec![0, 0]));
        status.run = 3;
        let mut meters = MetersDto::from(&EngineMeters {
            channel_levels: vec![0],
            musicians: Vec::new(),
        });
        meters.run = 3;
        let change = MixChangedDto::new(
            "192.168.1.3:50000".parse().expect("test address"),
            MixOrigin::Engine,
            &MixValues::default(),
            3,
        );
        let summary = StartSummary {
            run: 3,
            device_name: "Test device".to_owned(),
            capture_format: CaptureFormatDto {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "I16".to_owned(),
                buffer_size: "Default".to_owned(),
            },
        };

        for json in [
            serde_json::to_value(&status).expect("a status serializes"),
            serde_json::to_value(&meters).expect("meters serialize"),
            serde_json::to_value(&change).expect("a change serializes"),
            serde_json::to_value(&summary).expect("a summary serializes"),
        ] {
            assert_eq!(json["run"], 3, "{json}");
        }
    }

    #[test]
    fn a_running_engines_state_is_its_status_marked_running() {
        let mut status = EngineStatusDto::from(&status_with(Vec::new(), vec![0, 0]));
        status.run = 2;

        let json = serde_json::to_value(EngineStateDto::Running(status)).expect("serializes");

        assert_eq!(json["state"], "running");
        assert_eq!(json["run"], 2);
        assert_eq!(json["deviceName"], "Test device", "the whole status, flat");
        assert_eq!(
            serde_json::to_value(EngineStateDto::NotRunning { run: 2 }).expect("serializes"),
            serde_json::json!({ "state": "notRunning", "run": 2 })
        );
    }

    #[test]
    fn a_translated_status_or_meters_belong_to_no_run_until_one_is_set() {
        let status = EngineStatusDto::from(&status_with(Vec::new(), vec![0, 0]));
        let meters = MetersDto::from(&EngineMeters {
            channel_levels: Vec::new(),
            musicians: Vec::new(),
        });

        assert_eq!(status.run, 0);
        assert_eq!(meters.run, 0);
    }

    #[test]
    fn groups_and_the_channels_in_them_carry_the_groups_id() {
        let status = status_with(
            vec![
                GroupStatus {
                    id: "kit".to_owned(),
                    name: "Drums".to_owned(),
                    channels: vec![1],
                    invalid: false,
                },
                GroupStatus {
                    id: "kit-2".to_owned(),
                    name: "Drums".to_owned(),
                    channels: vec![0],
                    invalid: false,
                },
            ],
            vec![0, 0],
        );

        let json = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");

        assert_eq!(json["groups"][0]["id"], "kit");
        assert_eq!(json["groups"][1]["id"], "kit-2");
        assert_eq!(
            json["channels"][0]["groupId"], "kit-2",
            "joined by id, not name"
        );
        assert_eq!(json["channels"][1]["groupId"], "kit");
    }

    #[test]
    fn a_group_edit_answers_what_was_done_in_camel_case() {
        let answer = GroupsSetDto {
            groups: vec![SessionGroup {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1, 2],
            }],
            applied: false,
            saved: true,
            save_error: None,
        };

        assert_eq!(
            serde_json::to_value(answer).expect("serializes"),
            serde_json::json!({
                "groups": [{ "id": "kit", "name": "Drums", "channels": [1, 2] }],
                "applied": false,
                "saved": true,
                "saveError": null
            })
        );
    }

    #[test]
    fn a_group_request_may_name_its_id_or_leave_it_to_the_desk() {
        let requests: Vec<GroupRequest> = serde_json::from_str(
            r#"[{"id":"kit","name":"Drums","channels":[1,2]},{"name":"Vocals","channels":[3]}]"#,
        )
        .expect("both shapes parse");

        assert_eq!(requests[0].id.as_deref(), Some("kit"));
        assert_eq!(requests[1].id, None);
        assert_eq!(requests[1].channels, vec![3]);
    }

    #[test]
    fn channels_carry_their_details_and_an_input_without_any_gets_the_defaults() {
        let mut status = status_with(Vec::new(), vec![10, 20]);
        status.channels = vec![personal_monitoring::ChannelMeta {
            name: "Keys L".to_owned(),
            kind: personal_monitoring::ChannelKind::Keys,
            hidden: true,
            linked_to_next: true,
        }];

        let json = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");

        assert_eq!(
            json["channels"],
            serde_json::json!([
                {
                    "number": 1, "name": "Keys L", "kind": "keys", "hidden": true, "link": true,
                    "group": null, "groupId": null, "level": 10
                },
                {
                    "number": 2, "name": "", "kind": "other", "hidden": false, "link": false,
                    "group": null, "groupId": null, "level": 20
                }
            ])
        );
    }

    #[test]
    fn a_channel_edit_answers_what_was_done_in_camel_case() {
        let answer = ChannelsSetDto {
            channels: vec![SessionChannel {
                name: "Kick".to_owned(),
                kind: "drums".to_owned(),
                hidden: false,
                link: false,
            }],
            applied: true,
            saved: false,
            save_error: Some("disk full".to_owned()),
        };

        assert_eq!(
            serde_json::to_value(answer).expect("serializes"),
            serde_json::json!({
                "channels": [{ "name": "Kick", "kind": "drums", "hidden": false, "link": false }],
                "applied": true,
                "saved": false,
                "saveError": "disk full"
            })
        );
    }

    #[test]
    fn a_channel_request_takes_defaults_for_what_it_leaves_out_and_refuses_unknown_fields() {
        let requests: Vec<ChannelRequest> = serde_json::from_str(
            r#"[{"name":"Kick","kind":"drums","hidden":false,"link":true},{}]"#,
        )
        .expect("both shapes parse");

        assert_eq!(
            requests,
            vec![
                ChannelRequest {
                    name: "Kick".to_owned(),
                    kind: "drums".to_owned(),
                    hidden: false,
                    link: true,
                },
                ChannelRequest {
                    name: String::new(),
                    kind: "other".to_owned(),
                    hidden: false,
                    link: false,
                },
            ]
        );
        let error = serde_json::from_str::<ChannelRequest>(r#"{"hiden":true}"#)
            .expect_err("a misspelt field must not be a silent no-op");
        assert!(error.to_string().contains("hiden"), "names it: {error}");
    }

    #[test]
    fn a_channel_of_an_invalid_group_is_not_labelled_with_it() {
        let status = status_with(
            vec![GroupStatus {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![0, 1],
                invalid: true,
            }],
            vec![0, 0],
        );

        let dto = EngineStatusDto::from(&status);

        assert_eq!(dto.channels[0].group, None);
        assert!(dto.groups[0].invalid);
    }
}
