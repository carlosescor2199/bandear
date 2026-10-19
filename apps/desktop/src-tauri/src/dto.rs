//! The JSON contract between the webview and the engine.
//!
//! The engine's own types are not serialized directly: this module owns the wire shape the window
//! consumes, so the engine keeps no presentation concerns and can change without breaking the UI.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use personal_monitoring::{
    CaptureFormat, EngineMeters, EngineStatus, EngineerControls, EngineerUpdate, InputDevice,
    LinkHealth, LinkSample, LocalOutputState, LocalOutputStatus, MirrorStatus, MixOrigin,
    MixUpdate, MixValues, MusicianKey, MusicianStatus, OutputDevice, PlaybackBuffer,
    RecordingState, RecordingStatus, SampleFormat, SoundcheckClock, SoundcheckState,
    SoundcheckStatus, TakeEnd, TakeMarker, TakeSummary, TakeTrack, CAPABILITY_PLAYBACK_BUFFER,
};

use crate::session::{
    address_key, client_key, SessionAudio, SessionChannel, SessionGroup, SessionOutput,
    SessionPreset,
};
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
    /// Why the last send to this musician failed, for example "network unreachable"; null while
    /// none has.
    pub last_send_error: Option<&'static str>,
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
    /// What the musician's phone announced it supports (for example "stats.v2"), in the order
    /// sent; empty for a configured target and for a phone that predates capabilities.
    pub capabilities: Vec<String>,
    /// The control channel's round trip in milliseconds, smoothed, and the last one measured; null
    /// until its first pong. It does not come from the phone's reports, so an older phone has one
    /// too.
    pub rtt_ms: Option<f64>,
    pub rtt_last_ms: Option<f64>,
    /// The newest second of the musician's link history (a gap included); null before their
    /// phone's first report. The history itself is read with `link_history`, never carried here.
    pub link_latest: Option<LinkSampleDto>,
    /// Whose mix this musician's follows (see [crate::commands::set_mirror]); null when it
    /// follows nobody.
    pub mirror: Option<MirrorDto>,
    /// The playback buffer the engine keeps for this musician (see
    /// [crate::commands::playback_buffer]): `{ mode: "auto" }` or `{ mode: "fixed", ms }`. Auto
    /// when the desk set none.
    pub buffer: BufferDto,
    /// Whether the musician's phone plays the buffer the desk sets (it announced
    /// [personal_monitoring::CAPABILITY_PLAYBACK_BUFFER]); false for an older app and a configured
    /// target, which the window shows as "needs a newer app". The cushion it plays is
    /// `linkLatest.cushionMs`.
    pub buffer_supported: bool,
}

/// A musician's playback buffer as the window reads and sets it: `mode` `auto` or `fixed`, and the
/// length `ms` of a fixed one (absent for Auto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct BufferDto {
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u16>,
}

impl From<PlaybackBuffer> for BufferDto {
    fn from(buffer: PlaybackBuffer) -> Self {
        Self {
            mode: buffer.mode(),
            ms: buffer.fixed_ms(),
        }
    }
}

/// What [crate::commands::playback_buffer::set_musician_buffer] did, for the window.
///
/// `outcome` is one of:
///
/// - `sent`: the phone is connected and was sent it; its next reports say the cushion it plays.
/// - `notConnected`: the engine keeps it, and the phone gets it when it is back.
/// - `unsupported`: the phone does not take a buffer from the desk (it needs a newer app); nothing
///   changed.
/// - `invalid`: the request was not a buffer the desk can set (a fixed length outside
///   `minMs`-`maxMs`, or no length); nothing changed.
///
/// `buffer` is what the musician has now (for `unsupported` and `invalid`, what was asked is not
/// it, and it is absent). `saved` says whether the session keeps it for the next start; a save that
/// failed is in `saveError`, the phone having it all the same. `message` says the outcome in words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSetDto {
    pub musician_key: String,
    pub outcome: &'static str,
    pub buffer: Option<BufferDto>,
    pub saved: bool,
    pub save_error: Option<String>,
    pub message: String,
    pub min_ms: u32,
    pub max_ms: u32,
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

/// What [crate::commands::load_session] did with a saved session.
///
/// `applied` says whether a running engine took it (every phone has its channels and groups now);
/// `saved` whether the desk's session is now the loaded one, and `saveError` why not. `mixes` are
/// the saved mixes pushed to musicians, `skipped` those that went to nobody, `refused` the
/// controls or mirrors the engine refused, `damaged` the parts of the saved session that could not
/// be read and were left out. `audio` is the audio input the next start opens: the saved
/// session's when `audioLoaded`, else the desk's own, with `audioSkipped` saying why the saved one
/// was not taken. `presets` are the engineer's presets now.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadedDto {
    pub session: SnapshotSummary,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
    pub mixes: Vec<LoadedMixDto>,
    pub skipped: Vec<SkippedMixDto>,
    pub refused: Vec<String>,
    pub damaged: Vec<String>,
    pub audio: Option<SessionAudio>,
    pub audio_loaded: bool,
    pub audio_skipped: Option<String>,
    pub presets: Vec<SessionPreset>,
    /// Whether the saved session had a local section and the desk took it; one saved before the
    /// local mix existed leaves the desk's own.
    pub local_loaded: bool,
    /// The local mix now stored, when a running engine took the saved one; else null.
    pub local_mix: Option<MixDto>,
    /// Why the saved session's local output was not opened by the running engine, else null.
    pub local_output_notice: Option<String>,
}

/// A saved mix a loaded session pushed to a musician.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedMixDto {
    /// The musician's address as the status rows show it.
    pub address: String,
    /// The name of the musician whose mix theirs follows, when it follows one: then only their
    /// master volume and mute were pushed.
    pub follows: Option<String>,
    /// Their whole mix now stored.
    pub mix: MixDto,
}

/// What restoring a mix backup did (see [crate::commands::backups::restore_mix_backup]): the
/// backup as listed, the mixes put back on the musicians who are connected, and the backed-up
/// mixes that went to nobody, with why (`notConnected`, `notHere` or `refused`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixRestoredDto {
    pub backup: crate::backups::MixBackupSummary,
    pub restored: Vec<LoadedMixDto>,
    pub skipped: Vec<SkippedMixDto>,
}

/// A saved mix a loaded session pushed to nobody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMixDto {
    /// The musician's key: `client:<id>` or `address:<ip:port>`.
    pub key: String,
    /// The label the loaded session keeps for them, if any.
    pub label: Option<String>,
    /// "notHere" (no musician here under that key), "notConnected" (their phone's control channel
    /// is down: its own mix wins when it connects), "notRunning" (no engine runs) or "refused" (the
    /// engine refused the change, `error` says why).
    pub reason: &'static str,
    pub error: Option<String>,
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

/// The id the window knows the engineer's local mix by: its tab, its pending edits and its
/// `mix-changed` event. No musician's address can be it.
pub const LOCAL_MIX_ID: &str = "local";

/// The payload of the `mix-changed` event for the engineer's local mix: the whole mix right after
/// a change, made here or moved by a group, channel or device change. It names the id
/// [LOCAL_MIX_ID] and no address, so it can never be taken for a musician's.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMixChangedDto {
    /// The engine run the change happened in.
    pub run: u64,
    /// Always [LOCAL_MIX_ID].
    pub id: &'static str,
    /// Always "desktop": no phone changes the local mix.
    pub origin: &'static str,
    /// The whole mix now stored, nested as in a musician's change.
    pub mix: MixDto,
}

impl LocalMixChangedDto {
    pub fn new(mix: &MixValues, run: u64) -> Self {
        Self {
            run,
            id: LOCAL_MIX_ID,
            origin: "desktop",
            mix: MixDto::from(mix),
        }
    }
}

/// One output device the local mix can play on, with its output channel count (see
/// [crate::commands::local::output_devices]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDeviceDto {
    pub name: String,
    pub channels: u16,
}

impl From<&OutputDevice> for OutputDeviceDto {
    fn from(device: &OutputDevice) -> Self {
        Self {
            name: device.name.clone(),
            channels: device.channels,
        }
    }
}

/// The output the window asks the local mix to play on ([crate::commands::local::set_local_output]):
/// the device by its exact name and the first output of the stereo pair, 0-based (outputs 1/2 are
/// `0`). A field the desk does not know is rejected.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalOutputRequestDto {
    pub device: String,
    pub first_channel: u16,
}

/// Where the local mix plays and how, on the status (`localOutput`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalOutputDto {
    /// The output device in use (or the one that failed); null when the local output is off.
    pub device: Option<String>,
    /// The first output of its stereo pair, 0-based; null when off.
    pub first_channel: Option<u16>,
    /// "off", "running" or "failed".
    pub state: &'static str,
    /// Why it failed; null unless `state` is "failed".
    pub reason: Option<String>,
    /// Frames of silence played because no audio was there, since this output was chosen.
    pub underrun_frames: u64,
    /// Frames dropped to hold the delay, since this output was chosen.
    pub dropped_frames: u64,
    /// Why the output the session keeps was not opened when the engine started or a session was
    /// loaded (the running driver does not offer it, or it could not be opened); null otherwise.
    /// Filled by the command layer: the engine knows nothing of the session.
    pub notice: Option<String>,
}

impl From<&LocalOutputStatus> for LocalOutputDto {
    fn from(status: &LocalOutputStatus) -> Self {
        let (state, reason) = match &status.state {
            LocalOutputState::Off => ("off", None),
            LocalOutputState::Running => ("running", None),
            LocalOutputState::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Self {
            device: status.output.as_ref().map(|output| output.device.clone()),
            first_channel: status.output.as_ref().map(|output| output.first_channel),
            state,
            reason,
            underrun_frames: status.underrun_frames,
            dropped_frames: status.dropped_frames,
            notice: None,
        }
    }
}

/// What [crate::commands::local::set_local_output] did, as [GroupsSetDto] says it for groups:
/// `applied` whether a running engine took the choice (the local mix plays there now, or stopped),
/// `saved` whether the session keeps it for the next start, and `saveError` why not. With no engine
/// running it is kept and applies at the next start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalOutputSetDto {
    /// The output now chosen, as the session keeps it; null for none.
    pub output: Option<SessionOutput>,
    /// The local output as the running engine reports it now; null with no engine.
    pub local_output: Option<LocalOutputDto>,
    pub applied: bool,
    pub saved: bool,
    pub save_error: Option<String>,
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
    /// The 0-100 peak of the engineer's local mix, whether or not an output plays it.
    pub local: OutputLevelsDto,
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
            local: OutputLevelsDto {
                left: meters.local_levels[0],
                right: meters.local_levels[1],
            },
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
            last_send_error: musician.counters.last_send_error,
            link_health: link_health_name(musician.link.health),
            packets_per_second: musician.link.packets_per_second,
            underruns_per_second: musician.link.underruns_per_second,
            loss_percent: musician.link.loss_percent,
            receive_timeouts_per_second: musician.link.receive_timeouts_per_second,
            capabilities: musician.capabilities.clone(),
            rtt_ms: musician.link.round_trip.smoothed_ms,
            rtt_last_ms: musician.link.round_trip.last_ms,
            link_latest: musician.link.latest.as_ref().map(LinkSampleDto::from),
            mirror: musician.mirror.as_ref().map(MirrorDto::from),
            buffer: BufferDto::from(musician.playback_buffer),
            buffer_supported: musician
                .capabilities
                .iter()
                .any(|name| name == CAPABILITY_PLAYBACK_BUFFER),
        }
    }
}

/// One second of a musician's link history, for the window (see
/// [personal_monitoring::LinkSample]). Every number is null in a gap, the round trip excepted, and
/// the phone's own numbers (jitter, cushion, backlog, recovered) are null when it does not report
/// them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSampleDto {
    /// The sample's number in its history: the cursor (`sinceSeq`) a window passes for what is new.
    pub seq: u64,
    /// When, on the monotonic clock: milliseconds since the history began. Only ever grows, by the
    /// time that really passed; the axis a chart draws on.
    pub mono_ms: u64,
    /// When, in Unix milliseconds (UTC), for display only: the wall clock may have been stepped.
    pub at_ms: u64,
    /// No report arrived for this second while the phone was connected.
    pub gap: bool,
    /// The first sample after the phone reconnected.
    pub reconnected: bool,
    /// "unknown" (a gap), "good", "warning" or "bad".
    pub health: &'static str,
    pub interval_ms: Option<u32>,
    pub packets_received: Option<u64>,
    pub packets_lost: Option<u64>,
    pub loss_percent: Option<f64>,
    pub underruns: Option<u64>,
    pub receive_timeouts: Option<u64>,
    pub rtt_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub cushion_ms: Option<f64>,
    pub backlog_ms: Option<f64>,
    pub recovered: Option<u64>,
}

impl From<&LinkSample> for LinkSampleDto {
    fn from(sample: &LinkSample) -> Self {
        Self {
            seq: sample.seq,
            mono_ms: sample.mono_ms,
            at_ms: sample.at_ms,
            gap: sample.gap,
            reconnected: sample.reconnected,
            health: link_health_name(sample.health),
            interval_ms: sample.interval_ms,
            packets_received: sample.packets_received,
            packets_lost: sample.packets_lost,
            loss_percent: sample.loss_percent,
            underruns: sample.underruns,
            receive_timeouts: sample.receive_timeouts,
            rtt_ms: sample.rtt_ms,
            jitter_ms: sample.jitter_ms,
            cushion_ms: sample.cushion_ms,
            backlog_ms: sample.backlog_ms,
            recovered: sample.recovered,
        }
    }
}

/// What `link_history` answers: the musician's key as asked, the history's id (a new one means a new
/// history, read whole), their samples numbered after `sinceSeq` (all of them without it), the
/// oldest first, and how many a history keeps and how far apart they are.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkHistoryDto {
    pub musician_key: String,
    pub history_id: u64,
    pub samples: Vec<LinkSampleDto>,
    pub max_samples: usize,
    pub period_ms: u64,
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
    /// The engineer's local mix (see [crate::commands::local]): a whole mix as a musician's, with
    /// no level cap, limit or locks the window shows.
    pub local_mix: MixDto,
    /// Where the local mix plays and how.
    pub local_output: LocalOutputDto,
    /// The take being recorded, if any (see [crate::commands::recording]).
    pub recording: RecordingDto,
    /// The most recent take that ended while this engine ran, however it ended; null before one.
    pub last_take: Option<TakeSummaryDto>,
    /// The take played back in place of the inputs, if any (see [crate::commands::soundcheck]).
    pub soundcheck: SoundcheckDto,
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
            local_mix: MixDto::from(&status.local_mix),
            local_output: LocalOutputDto::from(&status.local_output),
            recording: RecordingDto::from(&status.recording),
            last_take: status
                .recording
                .last_take
                .as_ref()
                .map(TakeSummaryDto::from),
            soundcheck: SoundcheckDto::from(&status.soundcheck),
        }
    }
}

/// The virtual soundcheck on the status (`soundcheck`) and in every soundcheck command's answer:
/// whether a take plays in place of the inputs, where it is and what paces it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundcheckDto {
    /// "off" (the inputs are live), "playing", "paused" (by the engineer, or stopped at the end of
    /// a take that does not loop) or "failed" (reading the take failed; only stopping is left).
    pub state: &'static str,
    /// Why it failed; null unless `state` is "failed".
    pub reason: Option<String>,
    /// The take's folder, whole, while a take plays (or is paused or failed); null when off.
    pub take_folder: Option<String>,
    /// The take's name from its `take.json`; null when off.
    pub take_name: Option<String>,
    /// The next frame of the take to play, from 0, and the same in seconds.
    pub position_frames: u64,
    pub position_seconds: f64,
    /// The frames the take plays for, and the same in seconds.
    pub length_frames: u64,
    pub length_seconds: f64,
    pub looping: bool,
    /// Frames of silence played because the disk fell behind.
    pub underrun_frames: u64,
    /// What paces the take: "output" (the local output's device) or "timer" (no output plays).
    pub clock: &'static str,
}

impl From<&SoundcheckStatus> for SoundcheckDto {
    fn from(status: &SoundcheckStatus) -> Self {
        let (state, reason) = match &status.state {
            SoundcheckState::Off => ("off", None),
            SoundcheckState::Playing => ("playing", None),
            SoundcheckState::Paused => ("paused", None),
            SoundcheckState::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Self {
            state,
            reason,
            take_folder: status
                .take_folder
                .as_ref()
                .map(|folder| folder.display().to_string()),
            take_name: status.take_name.clone(),
            position_frames: status.position_frames,
            // Every take plays at the engine's rate, whatever the device's.
            position_seconds: seconds_of(status.position_frames, TAKE_SAMPLE_RATE),
            length_frames: status.length_frames,
            length_seconds: seconds_of(status.length_frames, TAKE_SAMPLE_RATE),
            looping: status.looping,
            underrun_frames: status.underrun_frames,
            clock: match status.clock {
                SoundcheckClock::Timer => "timer",
                SoundcheckClock::Output => "output",
            },
        }
    }
}

/// The rate every take is recorded at: the engine's internal rate.
const TAKE_SAMPLE_RATE: u32 = 48_000;

/// `frames` at `sample_rate` in seconds; 0 for a rate of 0.
fn seconds_of(frames: u64, sample_rate: u32) -> f64 {
    if sample_rate == 0 {
        return 0.0;
    }
    frames as f64 / f64::from(sample_rate)
}

/// A take's sample format by the name `take.json` and the window use: `int24` or `float32`.
pub fn format_name(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::Int16 => "int16",
        SampleFormat::Int24 => "int24",
        SampleFormat::Int32 => "int32",
        SampleFormat::Float32 => "float32",
    }
}

/// A named point in a take: its frame from the take's first, the same in seconds, and its name.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeMarkerDto {
    pub frame: u64,
    pub seconds: f64,
    pub name: String,
}

impl TakeMarkerDto {
    /// `marker` of a take recorded at `sample_rate`.
    pub fn of(marker: &TakeMarker, sample_rate: u32) -> Self {
        Self {
            frame: marker.frame,
            seconds: seconds_of(marker.frame, sample_rate),
            name: marker.name.clone(),
        }
    }
}

/// The recorder on the status (`recording`): whether a take records and how far it got.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    /// "idle", "recording", "stopping" (its files are being finished) or "failed" (a disk error
    /// stopped it; `stop_recording` collects it).
    pub state: &'static str,
    /// Why it failed; null unless `state` is "failed".
    pub reason: Option<String>,
    /// The take's folder while there is a take; null when idle.
    pub take_folder: Option<String>,
    /// Frames recorded so far, and the same in seconds.
    pub frames: u64,
    pub seconds: f64,
    /// Frames dropped because the disk fell behind.
    pub dropped_frames: u64,
    pub bytes_written: u64,
    /// The take's track names, in file order.
    pub tracks: Vec<String>,
    pub markers: Vec<TakeMarkerDto>,
    /// The free space on the take's disk; null without a take or when it cannot be read.
    pub free_bytes: Option<u64>,
    /// The whole minutes of this take that space still holds; null when unknown.
    pub minutes_left: Option<u64>,
}

impl From<&RecordingStatus> for RecordingDto {
    fn from(status: &RecordingStatus) -> Self {
        let (state, reason) = match &status.state {
            RecordingState::Idle => ("idle", None),
            RecordingState::Recording => ("recording", None),
            RecordingState::Stopping => ("stopping", None),
            RecordingState::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Self {
            state,
            reason,
            take_folder: status
                .take_folder
                .as_ref()
                .map(|folder| folder.display().to_string()),
            frames: status.frames,
            seconds: seconds_of(status.frames, TAKE_SAMPLE_RATE),
            dropped_frames: status.dropped_frames,
            bytes_written: status.bytes_written,
            tracks: status.tracks.clone(),
            markers: status
                .markers
                .iter()
                .map(|marker| TakeMarkerDto::of(marker, TAKE_SAMPLE_RATE))
                .collect(),
            free_bytes: status.free_bytes,
            minutes_left: status.minutes_left,
        }
    }
}

/// One file of a take.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeTrackDto {
    /// The file's name inside the take folder.
    pub file: String,
    /// "input" (one input, mono), "pair" (a linked pair, stereo) or "local" (the local mix).
    pub kind: String,
    /// The inputs it holds, 1-based; empty for the local mix.
    pub channels: Vec<u16>,
    pub name: String,
    /// The instrument kind's name when the take started; null for the local mix.
    pub instrument: Option<String>,
    /// The frames its file holds (0 while the take records).
    pub frames: u64,
}

impl From<&TakeTrack> for TakeTrackDto {
    fn from(track: &TakeTrack) -> Self {
        Self {
            file: track.file.clone(),
            kind: track.kind.name().to_owned(),
            channels: track.channels.clone(),
            name: track.name.clone(),
            instrument: track.instrument.map(|kind| kind.name().to_owned()),
            frames: track.frames,
        }
    }
}

/// A take as `take.json` describes it: the one `stop_recording` answers, the status's `lastTake`
/// and each entry `list_takes` reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeSummaryDto {
    /// The take's folder, whole.
    pub folder: String,
    pub name: String,
    /// When it started, UTC, RFC 3339.
    pub started: String,
    pub sample_rate: u32,
    /// "int24" or "float32".
    pub format: String,
    /// The input device it was recorded from.
    pub device: String,
    pub tracks: Vec<TakeTrackDto>,
    /// The frames every file holds, and the same in seconds.
    pub frames: u64,
    pub seconds: f64,
    pub markers: Vec<TakeMarkerDto>,
    pub dropped_frames: u64,
    /// How it ended: "stopped", "engineStopped" or "diskError"; in a listing also "recording"
    /// (the take being recorded now) or "interrupted" (it never ended: the app or the computer
    /// stopped while it recorded, and its files hold what their last refresh did).
    pub ended: String,
    /// Why a disk error ended it; null otherwise.
    pub reason: Option<String>,
}

/// One take as `list_takes` lists it: the same shape as a summary.
pub type TakeListItemDto = TakeSummaryDto;

impl From<&TakeSummary> for TakeSummaryDto {
    fn from(take: &TakeSummary) -> Self {
        let (ended, reason) = match &take.ended {
            TakeEnd::Stopped => ("stopped", None),
            TakeEnd::EngineStopped => ("engineStopped", None),
            TakeEnd::DiskError { reason } => ("diskError", Some(reason.clone())),
        };
        Self {
            folder: take.folder.display().to_string(),
            name: take.name.clone(),
            started: take.started.clone(),
            sample_rate: take.sample_rate,
            format: format_name(take.format).to_owned(),
            device: take.device.clone(),
            tracks: take.tracks.iter().map(TakeTrackDto::from).collect(),
            frames: take.frames,
            seconds: seconds_of(take.frames, take.sample_rate),
            markers: take
                .markers
                .iter()
                .map(|marker| TakeMarkerDto::of(marker, take.sample_rate))
                .collect(),
            dropped_frames: take.dropped_frames,
            ended: ended.to_owned(),
            reason,
        }
    }
}

/// How this desk records takes, as `recording_settings` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSettingsDto {
    /// The folder takes are made in, whole: the kept one, or the default.
    pub folder: String,
    /// The desk's default folder (the user's Music folder + `MixLink\Recordings`).
    pub default_folder: String,
    /// Whether `folder` is the default.
    pub folder_is_default: bool,
    /// "int24" or "float32".
    pub format: &'static str,
    pub include_local: bool,
    /// The armed inputs, 1-based, ascending.
    pub armed: Vec<u16>,
    /// Why the default folder is not in the Music folder, when it could not be found; null
    /// otherwise.
    pub notice: Option<String>,
}

/// How the window asks the desk to record takes (`set_recording_settings`). `folder` null or blank
/// is the default folder; `format` "int24" (the default) or "float32"; `armed` 1-based inputs. A
/// field the desk does not know is rejected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordingSettingsRequest {
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub include_local: bool,
    #[serde(default)]
    pub armed: Vec<u32>,
}

/// What `start_recording` did: the take that records, where, and which armed inputs it holds.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStartedDto {
    /// The recorder now (state "recording", with the take's folder and tracks).
    pub recording: RecordingDto,
    /// The folder the take folder was made in.
    pub folder: String,
    /// The armed inputs the take records, 1-based (either side of a linked pair records the pair).
    pub armed: Vec<u16>,
    /// Armed inputs the running device does not have, 1-based: left out of the take.
    pub skipped: Vec<u16>,
    pub include_local: bool,
    pub format: &'static str,
}

/// What `stop_recording` did: the finished take, or `finishing` when its files were still being
/// finished after the engine's wait (the take is stopping; ask again).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStoppedDto {
    pub finishing: bool,
    /// The finished take; null while `finishing`.
    pub take: Option<TakeSummaryDto>,
    /// What the engine said while the files are still being finished; null otherwise.
    pub message: Option<String>,
}

/// What a take with the current settings would take on the disk (`record_estimate`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskEstimateDto {
    /// The folder takes are made in.
    pub folder: String,
    /// The bytes each second of the take adds; null when it cannot be worked out (see `problem`).
    pub bytes_per_second: Option<u64>,
    /// The free space on the folder's disk; null when it cannot be read.
    pub free_bytes: Option<u64>,
    /// The whole minutes that space holds; null when either is unknown.
    pub minutes_left: Option<u64>,
    /// Why the bytes per second are unknown (no engine runs, nothing is armed); null otherwise.
    pub problem: Option<String>,
}

/// The takes in the recordings folder (`list_takes`), the newest first.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakesDto {
    /// The folder that was read.
    pub folder: String,
    pub takes: Vec<TakeListItemDto>,
    /// Take folders whose `take.json` could not be read, left out.
    pub unreadable: usize,
    /// The folder does not exist yet (no take was recorded there).
    pub missing: bool,
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
            local_mix: personal_monitoring::MixValues::default(),
            local_output: Default::default(),
            recording: Default::default(),
            soundcheck: Default::default(),
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
            capabilities: Vec::new(),
            mirror: None,
            playback_buffer: personal_monitoring::PlaybackBuffer::Auto,
            redundancy: personal_monitoring::Redundancy::Off,
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
            capabilities: Vec::new(),
            mirror: None,
            playback_buffer: personal_monitoring::PlaybackBuffer::Auto,
            redundancy: personal_monitoring::Redundancy::Off,
        };

        let dto = MusicianDto::from(&musician);

        assert_eq!(dto.client_name, None);
        assert_eq!(dto.name, None);
        assert!(!dto.name_is_local);
    }

    #[test]
    fn the_last_send_error_reaches_the_window_and_is_null_without_one() {
        let failing = MusicianStatus {
            counters: personal_monitoring::MusicianCounters {
                packets_discarded: 3,
                last_send_error: Some("network unreachable"),
                ..Default::default()
            },
            ..musician_with_link(LinkStatus::default())
        };
        let fine = musician_with_link(LinkStatus::default());

        let failing_row =
            serde_json::to_value(MusicianDto::from(&failing)).expect("a row serializes");
        let fine_row = serde_json::to_value(MusicianDto::from(&fine)).expect("a row serializes");

        assert_eq!(failing_row["lastSendError"], "network unreachable");
        assert_eq!(failing_row["packetsDiscarded"], 3);
        assert!(fine_row["lastSendError"].is_null(), "{fine_row}");
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
            capabilities: Vec::new(),
            mirror: Some(MirrorStatus {
                leader: MusicianKey::Client("0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b".to_owned()),
                name: "Bass".to_owned(),
                leader_address: Some("192.168.1.12:50000".parse().expect("test address")),
            }),
            ..musician_with_link(LinkStatus::default())
        };
        let away = MusicianStatus {
            capabilities: Vec::new(),
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
            capabilities: Vec::new(),
            mirror: None,
            playback_buffer: personal_monitoring::PlaybackBuffer::Auto,
            redundancy: personal_monitoring::Redundancy::Off,
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
            ..LinkStatus::default()
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
            ..LinkStatus::default()
        })))
        .expect("a musician serializes");
        let silent = serde_json::to_value(MusicianDto::from(&musician_with_link(LinkStatus {
            health: LinkHealth::Bad,
            packets_per_second: Some(0.0),
            underruns_per_second: Some(0.0),
            loss_percent: Some(0.0),
            receive_timeouts_per_second: Some(0.0),
            ..LinkStatus::default()
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
    fn a_rows_buffer_and_whether_its_phone_takes_one_reach_the_window() {
        let mut phone = musician_with_link(LinkStatus::default());
        phone.capabilities = vec!["stats.v2".to_owned(), "playback.buffer".to_owned()];
        phone.playback_buffer = PlaybackBuffer::Fixed { ms: 40 };
        let older = musician_with_link(LinkStatus::default());

        let phone = serde_json::to_value(MusicianDto::from(&phone)).expect("serializes");
        let older = serde_json::to_value(MusicianDto::from(&older)).expect("serializes");

        assert_eq!(
            phone["buffer"],
            serde_json::json!({ "mode": "fixed", "ms": 40 })
        );
        assert_eq!(phone["bufferSupported"], true);
        assert_eq!(older["buffer"], serde_json::json!({ "mode": "auto" }));
        assert_eq!(older["bufferSupported"], false, "an older phone");
    }

    #[test]
    fn the_phones_capabilities_reach_the_window_and_the_other_fields_keep_their_names() {
        let mut phone = musician_with_link(LinkStatus::default());
        phone.capabilities = vec!["stats.v2".to_owned(), "playback.buffer".to_owned()];
        let older = musician_with_link(LinkStatus::default());

        let phone = serde_json::to_value(MusicianDto::from(&phone)).expect("serializes");
        let older = serde_json::to_value(MusicianDto::from(&older)).expect("serializes");

        assert_eq!(
            phone["capabilities"],
            serde_json::json!(["stats.v2", "playback.buffer"])
        );
        assert_eq!(
            older["capabilities"],
            serde_json::json!([]),
            "an older phone"
        );
        for field in [
            "address",
            "clientName",
            "clientId",
            "name",
            "nameIsLocal",
            "controlConnected",
            "volumePercent",
            "packetsSent",
            "packetsDiscarded",
            "lastSendError",
            "linkHealth",
            "packetsPerSecond",
            "underrunsPerSecond",
            "lossPercent",
            "receiveTimeoutsPerSecond",
            "mirror",
        ] {
            assert!(older.get(field).is_some(), "{field} is still on the row");
        }
    }

    #[test]
    fn a_row_carries_the_round_trip_and_the_newest_sample_but_never_the_history() {
        let latest = LinkSample {
            seq: 5,
            mono_ms: 5_000,
            at_ms: 5_000,
            gap: false,
            reconnected: false,
            health: LinkHealth::Warning,
            interval_ms: Some(1_000),
            packets_received: Some(199),
            packets_lost: Some(1),
            loss_percent: Some(0.5),
            underruns: Some(0),
            receive_timeouts: Some(0),
            rtt_ms: Some(7.0),
            jitter_ms: None,
            cushion_ms: Some(40.0),
            backlog_ms: None,
            recovered: None,
        };
        let phone = musician_with_link(LinkStatus {
            round_trip: personal_monitoring::RoundTrip {
                last_ms: Some(7.0),
                smoothed_ms: Some(6.5),
            },
            latest: Some(latest),
            ..LinkStatus::default()
        });

        let row = serde_json::to_value(MusicianDto::from(&phone)).expect("serializes");
        let never = serde_json::to_value(MusicianDto::from(&musician_with_link(
            LinkStatus::default(),
        )))
        .expect("serializes");

        assert_eq!(row["rttMs"], 6.5, "smoothed");
        assert_eq!(row["rttLastMs"], 7.0);
        assert_eq!(row["linkLatest"]["atMs"], 5_000);
        assert_eq!(row["linkLatest"]["health"], "warning");
        assert_eq!(row["linkLatest"]["cushionMs"], 40.0);
        assert!(row["linkLatest"]["jitterMs"].is_null());
        assert!(row.get("linkHistory").is_none() && row.get("samples").is_none());
        assert!(never["rttMs"].is_null());
        assert!(never["rttLastMs"].is_null());
        assert!(never["linkLatest"].is_null());
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
            local_levels: [25, 90],
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
                "local": { "left": 25, "right": 90 },
            }),
            "the local mix's peak, left then right, beside the musicians'"
        );
    }

    #[test]
    fn the_local_mix_and_its_output_reach_the_window_on_the_status() {
        let mut status = status_with(Vec::new(), vec![0, 0]);
        status.local_mix = MixValues {
            volume_percent: 70,
            revision: 4,
            ..MixValues::default()
        };
        status.local_output = LocalOutputStatus {
            output: Some(personal_monitoring::LocalOutputRequest {
                device: "OUTPUT 1/2 (Volt 4)".to_owned(),
                first_channel: 2,
            }),
            state: LocalOutputState::Running,
            underrun_frames: 480,
            dropped_frames: 12,
        };

        let json = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");

        assert_eq!(json["localMix"]["volumePercent"], 70);
        assert_eq!(json["localMix"]["revision"], 4);
        assert_eq!(
            json["localMix"]["channelGains"].as_array().map(Vec::len),
            Some(32),
            "a whole mix, as a musician's"
        );
        assert_eq!(
            json["localOutput"],
            serde_json::json!({
                "device": "OUTPUT 1/2 (Volt 4)",
                "firstChannel": 2,
                "state": "running",
                "reason": null,
                "underrunFrames": 480,
                "droppedFrames": 12,
                "notice": null
            })
        );

        status.local_output = LocalOutputStatus {
            state: LocalOutputState::Failed {
                reason: "the device was unplugged".to_owned(),
            },
            ..status.local_output
        };
        let failed = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");
        assert_eq!(failed["localOutput"]["state"], "failed");
        assert_eq!(failed["localOutput"]["reason"], "the device was unplugged");

        status.local_output = LocalOutputStatus::default();
        let off = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");
        assert_eq!(
            off["localOutput"],
            serde_json::json!({
                "device": null,
                "firstChannel": null,
                "state": "off",
                "reason": null,
                "underrunFrames": 0,
                "droppedFrames": 0,
                "notice": null
            }),
            "no local output is the default"
        );
    }

    #[test]
    fn a_local_mix_change_names_the_id_local_and_no_address() {
        let mix = MixValues {
            volume_percent: 33,
            ..MixValues::default()
        };

        let json = serde_json::to_value(LocalMixChangedDto::new(&mix, 5)).expect("serializes");

        assert_eq!(json["run"], 5);
        assert_eq!(json["id"], LOCAL_MIX_ID);
        assert_eq!(json["origin"], "desktop");
        assert_eq!(json["mix"]["volumePercent"], 33);
        assert!(
            json.get("address").is_none(),
            "never taken for a musician's: {json}"
        );
    }

    #[test]
    fn an_output_choice_reads_camel_case_and_answers_what_was_done() {
        let request: LocalOutputRequestDto = serde_json::from_value(
            serde_json::json!({ "device": "Speakers (Realtek)", "firstChannel": 2 }),
        )
        .expect("a request reads");
        assert_eq!(
            request,
            LocalOutputRequestDto {
                device: "Speakers (Realtek)".to_owned(),
                first_channel: 2,
            }
        );
        let misspelt = serde_json::from_value::<LocalOutputRequestDto>(
            serde_json::json!({ "device": "Speakers", "firstChannel": 0, "channel": 1 }),
        );
        assert!(misspelt.is_err(), "an unknown field is refused");

        let outcome = LocalOutputSetDto {
            output: Some(SessionOutput {
                device: "Speakers (Realtek)".to_owned(),
                first_channel: 2,
            }),
            local_output: None,
            applied: false,
            saved: true,
            save_error: None,
        };
        assert_eq!(
            serde_json::to_value(outcome).expect("serializes"),
            serde_json::json!({
                "output": { "device": "Speakers (Realtek)", "firstChannel": 2 },
                "localOutput": null,
                "applied": false,
                "saved": true,
                "saveError": null
            })
        );
        assert_eq!(
            serde_json::to_value(OutputDeviceDto::from(&OutputDevice {
                name: "Volt 4".to_owned(),
                channels: 4,
            }))
            .expect("serializes"),
            serde_json::json!({ "name": "Volt 4", "channels": 4 })
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
            local_levels: [0, 0],
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
            local_levels: [0, 0],
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

    fn take(ended: TakeEnd) -> TakeSummary {
        TakeSummary {
            folder: std::path::PathBuf::from("D:\\Takes\\2026-10-16_190405_Soundcheck"),
            name: "Soundcheck".to_owned(),
            started: "2026-10-16T17:04:05Z".to_owned(),
            sample_rate: 48_000,
            format: SampleFormat::Int24,
            device: "Volt 4".to_owned(),
            tracks: vec![
                TakeTrack {
                    file: "01 Kick.wav".to_owned(),
                    kind: personal_monitoring::TrackKind::Input,
                    channels: vec![1],
                    name: "Kick".to_owned(),
                    instrument: Some(personal_monitoring::ChannelKind::Drums),
                    frames: 96_000,
                },
                TakeTrack {
                    file: "Local mix.wav".to_owned(),
                    kind: personal_monitoring::TrackKind::Local,
                    channels: Vec::new(),
                    name: "Local mix".to_owned(),
                    instrument: None,
                    frames: 96_000,
                },
            ],
            frames: 96_000,
            markers: vec![TakeMarker {
                frame: 24_000,
                name: "Chorus".to_owned(),
            }],
            dropped_frames: 0,
            ended,
        }
    }

    #[test]
    fn the_status_carries_the_take_being_recorded_and_the_last_take() {
        let mut status = status_with(Vec::new(), vec![0, 0]);
        status.recording = RecordingStatus {
            state: RecordingState::Recording,
            take_folder: Some(std::path::PathBuf::from(
                "D:\\Takes\\2026-10-16_191000_Show",
            )),
            frames: 144_000,
            dropped_frames: 256,
            bytes_written: 1_000_000,
            tracks: vec!["Kick".to_owned(), "Local mix".to_owned()],
            markers: vec![TakeMarker {
                frame: 48_000,
                name: "Marker 1".to_owned(),
            }],
            free_bytes: Some(5_000_000_000),
            minutes_left: Some(289),
            last_take: Some(take(TakeEnd::Stopped)),
        };

        let json = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");

        assert_eq!(
            json["recording"],
            serde_json::json!({
                "state": "recording",
                "reason": null,
                "takeFolder": "D:\\Takes\\2026-10-16_191000_Show",
                "frames": 144_000,
                "seconds": 3.0,
                "droppedFrames": 256,
                "bytesWritten": 1_000_000,
                "tracks": ["Kick", "Local mix"],
                "markers": [{ "frame": 48_000, "seconds": 1.0, "name": "Marker 1" }],
                "freeBytes": 5_000_000_000u64,
                "minutesLeft": 289
            })
        );
        assert_eq!(json["lastTake"]["name"], "Soundcheck");
        assert_eq!(json["lastTake"]["ended"], "stopped");
    }

    #[test]
    fn an_idle_recorder_and_a_failed_take_are_said_with_their_reason() {
        let idle = serde_json::to_value(EngineStatusDto::from(&status_with(Vec::new(), vec![])))
            .expect("serializes");
        assert_eq!(idle["recording"]["state"], "idle");
        assert_eq!(idle["recording"]["takeFolder"], serde_json::Value::Null);
        assert_eq!(idle["lastTake"], serde_json::Value::Null);

        let failed = RecordingDto::from(&RecordingStatus {
            state: RecordingState::Failed {
                reason: "the disk is full".to_owned(),
            },
            ..RecordingStatus::default()
        });
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.reason.as_deref(), Some("the disk is full"));
    }

    #[test]
    fn the_status_carries_the_soundcheck_and_where_the_take_is() {
        let mut status = status_with(Vec::new(), vec![0, 0]);
        status.soundcheck = SoundcheckStatus {
            state: SoundcheckState::Playing,
            take_folder: Some(std::path::PathBuf::from(
                "D:\\Takes\\2026-10-16_191000_Show",
            )),
            take_name: Some("Show".to_owned()),
            position_frames: 72_000,
            length_frames: 480_000,
            looping: true,
            underrun_frames: 256,
            clock: SoundcheckClock::Output,
        };

        let json = serde_json::to_value(EngineStatusDto::from(&status)).expect("serializes");

        assert_eq!(
            json["soundcheck"],
            serde_json::json!({
                "state": "playing",
                "reason": null,
                "takeFolder": "D:\\Takes\\2026-10-16_191000_Show",
                "takeName": "Show",
                "positionFrames": 72_000,
                "positionSeconds": 1.5,
                "lengthFrames": 480_000,
                "lengthSeconds": 10.0,
                "looping": true,
                "underrunFrames": 256,
                "clock": "output"
            })
        );
    }

    #[test]
    fn a_soundcheck_off_paused_or_failed_is_said_with_its_reason() {
        let off = SoundcheckDto::from(&SoundcheckStatus::default());
        assert_eq!((off.state, off.clock), ("off", "timer"));
        assert_eq!(off.take_folder, None);

        let paused = SoundcheckDto::from(&SoundcheckStatus {
            state: SoundcheckState::Paused,
            ..SoundcheckStatus::default()
        });
        assert_eq!(paused.state, "paused");

        let failed = SoundcheckDto::from(&SoundcheckStatus {
            state: SoundcheckState::Failed {
                reason: "reading \"01 Kick.wav\" failed".to_owned(),
            },
            ..SoundcheckStatus::default()
        });
        assert_eq!(failed.state, "failed");
        assert_eq!(
            failed.reason.as_deref(),
            Some("reading \"01 Kick.wav\" failed")
        );
    }

    #[test]
    fn a_take_summary_has_the_windows_shape() {
        let json = serde_json::to_value(TakeSummaryDto::from(&take(TakeEnd::DiskError {
            reason: "write failed".to_owned(),
        })))
        .expect("serializes");

        assert_eq!(
            json,
            serde_json::json!({
                "folder": "D:\\Takes\\2026-10-16_190405_Soundcheck",
                "name": "Soundcheck",
                "started": "2026-10-16T17:04:05Z",
                "sampleRate": 48_000,
                "format": "int24",
                "device": "Volt 4",
                "tracks": [
                    {
                        "file": "01 Kick.wav",
                        "kind": "input",
                        "channels": [1],
                        "name": "Kick",
                        "instrument": "drums",
                        "frames": 96_000
                    },
                    {
                        "file": "Local mix.wav",
                        "kind": "local",
                        "channels": [],
                        "name": "Local mix",
                        "instrument": null,
                        "frames": 96_000
                    }
                ],
                "frames": 96_000,
                "seconds": 2.0,
                "markers": [{ "frame": 24_000, "seconds": 0.5, "name": "Chorus" }],
                "droppedFrames": 0,
                "ended": "diskError",
                "reason": "write failed"
            })
        );
        let stopped = TakeSummaryDto::from(&take(TakeEnd::EngineStopped));
        assert_eq!(stopped.ended, "engineStopped");
        assert_eq!(stopped.reason, None);
    }

    #[test]
    fn recording_settings_requests_reject_unknown_fields() {
        let request: RecordingSettingsRequest = serde_json::from_str(
            r#"{"folder":"D:\\Takes","format":"float32","includeLocal":true,"armed":[1,3]}"#,
        )
        .expect("reads");
        assert_eq!(
            request,
            RecordingSettingsRequest {
                folder: Some("D:\\Takes".to_owned()),
                format: Some("float32".to_owned()),
                include_local: true,
                armed: vec![1, 3],
            }
        );
        let error = serde_json::from_str::<RecordingSettingsRequest>(r#"{"armd":[1]}"#)
            .expect_err("a misspelt field must not be a silent no-op");
        assert!(error.to_string().contains("armd"), "names it: {error}");
    }
}
