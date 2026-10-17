use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use cpal::traits::{HostTrait, StreamTrait};

use crate::capture::{
    build_input_stream, describe_buffer, input_host, select_device, select_device_by_name,
    select_input_config, AudioHost, TARGET_SAMPLE_RATE,
};
use crate::cli::{check_groups, resolve_targets, validate_groups, EngineConfig, GroupDefinition};
use crate::control::{spawn_control_thread, ConfigSnapshot, ConfigState, ControlPeers};
use crate::discovery::spawn_discovery_thread;
use crate::health::{link_status, LinkStatus};
use crate::mix::{
    group_sources, ChannelFlags, EngineerControls, EngineerUpdate, Group, GroupLayout, Membership,
    MixPreset, MixState, MixUpdate, MixValues, MAX_MIX_CHANNELS,
};
use crate::network::{NetworkSink, PacketStats};
use crate::output::{
    check_request, local_output_sink, offered_outputs, open_output, output_channels,
    system_output_device, system_output_devices, LocalOutputRequest, LocalOutputState,
    LocalOutputStatus, OpenedOutput, OutputControl, OutputDevice, OutputShared,
};
use crate::pipeline::{CaptureLinks, CaptureStats, Pipeline, PipelineHandle, SourceReader};
use crate::recorder::{
    free_space, minutes_left, open_take, plan_tracks, recorder_sink, take_bytes_per_second,
    DiskEstimate, FileOpener, RecordRequest, Recorder, RecordingStatus, TakeEnd, TakeMarker,
    TakeOptions, TakeSummary, TrackOpener, STOP_WAIT,
};
use crate::soundcheck::{
    PlaybackClock, Soundcheck, SoundcheckError, SoundcheckRequest, SoundcheckStatus,
};
use crate::targets::{MirrorStatus, MusicianKey, TargetEntry, TargetRegistry};

/// The capture buffer the engine requests when the caller did not choose one.
///
/// It is a size a driver can honour (ASIO requires a power of two) and it keeps the packet rate near
/// 190 per second at 48 kHz. That is the measured balance: small enough to cut both the capture
/// period and the client's playback cushion, and slow enough that a phone's single receive loop keeps
/// up. The ASIO driver default of 128 frames is faster still but made the Android client drop about
/// 7% of packets in testing, and WASAPI shared mode ignores the request entirely.
const DEFAULT_BUFFER_FRAMES: u32 = 256;

/// Something that changed while the engine was running.
///
/// The engine emits these over an [EngineHandle::subscribe] channel. Connect and disconnect are the
/// state a UI cannot cheaply poll for, and a mix change is the confirmation that a command was
/// applied. Every address identifies its musician by IP, the engine's join key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    /// A control channel became connected for this peer.
    ClientConnected { address: SocketAddr },
    /// The last control channel for this peer's IP closed.
    ClientDisconnected { address: SocketAddr },
    /// The mix of the musician at this address's IP changed. The address is the control peer's
    /// for a change from the phone and the musician's UDP target for a change through the engine.
    MixChanged {
        address: SocketAddr,
        origin: MixOrigin,
    },
    /// The groups the status reports changed: the engineer edited them, or a device switch changed
    /// which of them the device can feed. It comes before the [Self::MixChanged] of every mix whose
    /// group levels moved with an edit, so a consumer can show the new groups before levels placed
    /// for them.
    GroupsChanged,
    /// The channel details the status reports changed: the engineer named, hid or linked inputs
    /// ([EngineHandle::set_channels]), or a device switch changed which inputs they are shown for.
    /// It comes before the [Self::MixChanged] of every mix a new stereo pair panned apart.
    ChannelsChanged,
    /// The engine's local mix changed: through [EngineHandle::set_local_mix], or because a group
    /// edit, a device switch or new channel details moved its levels or pans (then after
    /// [Self::GroupsChanged] or [Self::ChannelsChanged]). Read it with [EngineHandle::local_mix].
    /// It is no musician's mix, so it names no address and no phone is ever sent it.
    LocalMixChanged,
    /// The soundcheck changed through the engine: a take started or stopped playing in place of
    /// the inputs, or was paused, resumed, moved or set to loop. Read it with
    /// [EngineHandle::soundcheck]. What the take does on its own (reaching its end, a read error)
    /// shows in the status only: the engine thread never touches the event bus.
    SoundcheckChanged,
}

/// Where a mix change came from.
///
/// The control channel uses it to tell a phone about changes it did not make itself: a phone's
/// own change is answered with `mix_ack`, and pushing it back would only echo it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixOrigin {
    /// A `mix` message from the musician's phone, over the control connection with this peer
    /// address. The peer's port tells two connections from the same phone apart.
    Client { peer: SocketAddr },
    /// [EngineHandle::set_mix], the engine API the desktop uses.
    Engine,
}

/// The capture format the engine selected, as plain strings so a UI can render it without a cpal
/// dependency. [Self::sample_rate] is the device's rate; the packaged rate is always 48 kHz.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureFormat {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: String,
    pub buffer_size: String,
}

/// One musician's send counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MusicianCounters {
    pub packets_sent: u64,
    pub packets_discarded: u64,
}

/// One musician: a UDP target joined with its control channel and its mix.
///
/// This is the row the UI shows. The engine performs the join, because the target list and the
/// control peers are two views of the same person and letting the UI infer the relationship from
/// two lists lets them disagree.
///
/// It is `PartialEq` but not `Eq`, because the link rates are fractional.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicianStatus {
    pub address: SocketAddr,
    /// The name the musician typed on their own app and announced when registering, if any.
    ///
    /// It is client input, not configuration: the desktop may show its own local label instead,
    /// but this is the name that travelled from the phone.
    pub name: Option<String>,
    /// The stable id the musician's phone announced when registering, if it sent one the server
    /// keeps; `None` for a configured target and for a phone that sent none.
    ///
    /// Unlike the address it survives a change of IP (the row moves with the phone), so it is what
    /// a consumer keys anything about the musician on. It is opaque, random and LAN-only.
    pub client_id: Option<String>,
    pub control_connected: bool,
    pub mix: MixValues,
    pub counters: MusicianCounters,
    /// What the musician's phone reports about the audio it receives, classified into a level.
    ///
    /// [LinkHealth::Unknown] with no rates when the phone never reported (including a phone that
    /// predates stats reports) or when its reports stopped after its control channel closed.
    /// [LinkHealth::Bad] with no rates when its reports stopped while the control channel is still
    /// connected: the phone is there but its link is lost.
    pub link: LinkStatus,
    /// Whose mix this musician's follows, when the engineer made it follow one (see
    /// [EngineHandle::set_mirror]); `None` when it follows nobody.
    pub mirror: Option<MirrorStatus>,
}

/// One configured group of source channels, as the observation API exposes it.
///
/// Channels are 0-based source indices, the same numbering the protocol's `config` message and the
/// mixer use. The groups change only when the engineer edits them ([EngineHandle::set_groups]); a
/// device switch never rewrites them, and instead [Self::invalid] reports when the current device no
/// longer has one of the channels the group names, so a UI can warn without the engineer's
/// configuration being destroyed. The position of a group in the list is the index of its level in
/// every mix's `group_levels`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupStatus {
    /// The group's stable id (see [GroupDefinition]).
    pub id: String,
    pub name: String,
    pub channels: Vec<usize>,
    /// True when the current capture device is missing a channel this group names. An invalid group
    /// is reported and kept, but is excluded from the mixer so it cannot affect audio.
    pub invalid: bool,
}

/// What a source channel carries, as the engineer labels it, from a fixed set every client knows.
///
/// The set is closed on purpose: a phone picks the icon for each kind, so a kind it does not know
/// would have none. [ChannelKind::name] is the name the `config` message and the desktop use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    Voice,
    Guitar,
    Bass,
    Keys,
    Drums,
    Percussion,
    Horns,
    Strings,
    /// Tracks or samples played from a computer.
    Playback,
    /// The click track or a metronome.
    Click,
    /// Anything else, and every input the engineer has not labelled.
    #[default]
    Other,
}

impl ChannelKind {
    /// Every kind, in the order a UI offers them.
    pub const ALL: [ChannelKind; 11] = [
        ChannelKind::Voice,
        ChannelKind::Guitar,
        ChannelKind::Bass,
        ChannelKind::Keys,
        ChannelKind::Drums,
        ChannelKind::Percussion,
        ChannelKind::Horns,
        ChannelKind::Strings,
        ChannelKind::Playback,
        ChannelKind::Click,
        ChannelKind::Other,
    ];

    /// The kind's name on the wire and in the desktop: lowercase, one word.
    pub fn name(self) -> &'static str {
        match self {
            ChannelKind::Voice => "voice",
            ChannelKind::Guitar => "guitar",
            ChannelKind::Bass => "bass",
            ChannelKind::Keys => "keys",
            ChannelKind::Drums => "drums",
            ChannelKind::Percussion => "percussion",
            ChannelKind::Horns => "horns",
            ChannelKind::Strings => "strings",
            ChannelKind::Playback => "playback",
            ChannelKind::Click => "click",
            ChannelKind::Other => "other",
        }
    }

    /// The kind with this [ChannelKind::name], or `None` for a name that is no kind.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// The longest channel name the engine takes, in characters.
pub const MAX_CHANNEL_NAME_LENGTH: usize = 32;

/// What the engineer says about one source channel.
///
/// The default is an unnamed input of kind [ChannelKind::Other], shown and on its own: a client
/// labels an unnamed channel by its number ("CH 3").
///
/// - A hidden channel is not heard in any mix (a spare input must not leak noise into anyone's
///   ears) and clients do not show it.
/// - `linked_to_next` makes this channel and the next one a stereo pair, set on the pair's first
///   channel only. A client shows a pair as one strip whose level, mute and solo write both
///   channels and whose pan is a balance over the two pans; the server keeps the pair's level, mute
///   and solo equal whoever writes them (see [EngineHandle::set_channels]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelMeta {
    pub name: String,
    pub kind: ChannelKind,
    pub hidden: bool,
    pub linked_to_next: bool,
}

/// Checks the engineer's channel details, in channel order (the first entry is CH 1), before any of
/// them is applied or kept.
///
/// At most 32 entries (the mixer's channels); names of at most [MAX_CHANNEL_NAME_LENGTH]
/// characters; a stereo pair only between two entries of the list, never overlapping another pair;
/// and both sides of a pair hidden or both shown, since a client shows a pair as one strip. Errors
/// name the channel by its 1-based number.
pub fn check_channels(channels: &[ChannelMeta]) -> Result<(), String> {
    if channels.len() > MAX_MIX_CHANNELS {
        return Err(format!(
            "channel details are supported for at most {MAX_MIX_CHANNELS} channels, received {}",
            channels.len()
        ));
    }
    for (index, channel) in channels.iter().enumerate() {
        let number = index + 1;
        if channel.name.chars().count() > MAX_CHANNEL_NAME_LENGTH {
            return Err(format!(
                "the name of CH {number} is longer than {MAX_CHANNEL_NAME_LENGTH} characters"
            ));
        }
        if !channel.linked_to_next {
            continue;
        }
        let Some(next) = channels.get(index + 1) else {
            return Err(format!(
                "CH {number} is linked to the next channel, but it is the last one"
            ));
        };
        if next.linked_to_next {
            return Err(format!(
                "CH {} cannot be in two stereo pairs (CH {number}-{} and CH {}-{})",
                number + 1,
                number + 1,
                number + 1,
                number + 2
            ));
        }
        if next.hidden != channel.hidden {
            return Err(format!(
                "CH {number} and CH {} are one stereo pair: hide or show both",
                number + 1
            ));
        }
    }
    Ok(())
}

/// The channel details for a device with `source_channels` inputs: one entry per input, in order.
///
/// Details of inputs the device has are kept, inputs with none get the defaults, and details past
/// the last input are left out, along with a pair whose second side is gone. It only shapes what
/// clients see: the engine keeps the details as set (see [crate::control::ConfigSnapshot]), so a
/// device with more inputs shows them again.
pub(crate) fn fit_channels(channels: &[ChannelMeta], source_channels: u8) -> Vec<ChannelMeta> {
    let count = usize::from(source_channels);
    let mut fitted: Vec<ChannelMeta> = channels.iter().take(count).cloned().collect();
    fitted.resize(count, ChannelMeta::default());
    // The last input has no next one to be paired with.
    if let Some(last) = fitted.last_mut() {
        last.linked_to_next = false;
    }
    fitted
}

/// The mixer's flags for the engineer's channel details as set (see [crate::mix::ChannelFlags]),
/// including those of inputs the device lacks: a hidden spare is silent the moment its input
/// exists, and a pair stays linked in every mix while its second side is missing. Details past the
/// mixer's channels carry no flag: the mixer does not hear those inputs.
pub(crate) fn channel_flags(channels: &[ChannelMeta]) -> ChannelFlags {
    let mut flags = ChannelFlags::default();
    for (index, channel) in channels.iter().enumerate().take(MAX_MIX_CHANNELS) {
        if channel.hidden {
            flags.hidden |= 1 << index;
        }
        if channel.linked_to_next && index + 1 < MAX_MIX_CHANNELS {
            flags.linked |= 1 << index;
        }
    }
    flags
}

/// A snapshot of everything the engine can report without blocking the audio path.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineStatus {
    pub device_name: String,
    pub capture_format: CaptureFormat,
    pub control_port: u16,
    /// The rate the engine packages at; the device format above may be a multiple of this.
    pub sample_rate: u32,
    /// Source channel count after capture, which fixes the valid group channel range.
    pub source_channels: u8,
    /// The configured group layout, in the order the engineer defined it.
    pub groups: Vec<GroupStatus>,
    /// The engineer's details of every source channel, one entry per channel in channel order
    /// (see [ChannelMeta]); an input the engineer said nothing about has the defaults.
    pub channels: Vec<ChannelMeta>,
    /// One 0-100 source-channel peak per channel, index-aligned with the source channels.
    ///
    /// The value is measured before mixing, so it describes the input rather than any one
    /// musician's output. It rises at once and falls with the audio (full scale to silence in
    /// 1.5 s), whoever reads it and however often: reading never changes it.
    pub channel_levels: Vec<u8>,
    pub stopped: bool,
    pub samples_received: u64,
    pub packets_sent: u64,
    pub packets_discarded: u64,
    pub musicians: Vec<MusicianStatus>,
    /// The engine's local mix (see [EngineHandle::local_mix]), with its revision.
    pub local_mix: MixValues,
    /// Where the local mix plays, and how (see [EngineHandle::set_local_output]).
    pub local_output: LocalOutputStatus,
    /// The take being recorded, if any, and the last one (see [EngineHandle::recording]).
    pub recording: RecordingStatus,
    /// The take playing in place of the inputs, if any (see [EngineHandle::soundcheck]).
    pub soundcheck: SoundcheckStatus,
}

/// How many events a live subscriber may fall behind before it is told it lagged.
///
/// Events are a few bytes and a control connection drains them as they come, so only a connection
/// stuck on a dead socket gets near this.
const LIVE_EVENT_CAPACITY: usize = 256;

/// A fan-out of [EngineEvent] to any number of observers.
///
/// A channel was chosen over a listener trait: it decouples the engine threads from the consumer,
/// needs no `Fn + Send + Sync` callback and no reentrancy care, and reuses `std::sync::mpsc` that
/// the crate already depends on. Each [EventBus::subscribe] hands out an independent receiver.
///
/// The control connections are async tasks, which cannot park on a `std` receiver, so the same
/// events also go out on a `tokio` broadcast for [EventBus::subscribe_live]. Both sides are fed by
/// the one [EventBus::emit], so they never disagree.
#[derive(Clone)]
pub(crate) struct EventBus {
    subscribers: Arc<Mutex<Vec<Sender<EngineEvent>>>>,
    live: tokio::sync::broadcast::Sender<EngineEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        let (live, _) = tokio::sync::broadcast::channel(LIVE_EVENT_CAPACITY);
        Self {
            subscribers: Arc::default(),
            live,
        }
    }
}

impl EventBus {
    pub(crate) fn subscribe(&self) -> Receiver<EngineEvent> {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.subscribers
            .lock()
            .expect("event subscribers mutex poisoned")
            .push(sender);
        receiver
    }

    /// Subscribes an async task to every event emitted from now on.
    pub(crate) fn subscribe_live(&self) -> tokio::sync::broadcast::Receiver<EngineEvent> {
        self.live.subscribe()
    }

    pub(crate) fn emit(&self, event: EngineEvent) {
        // An error only means no live subscriber exists right now, which is not a failure.
        let _ = self.live.send(event.clone());
        let mut subscribers = self
            .subscribers
            .lock()
            .expect("event subscribers mutex poisoned");
        subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }
}

/// Applies a partial mix update to the musician whose IP `value` names, and announces it as a
/// change made through the engine.
///
/// `value` is the address string the engine reports, so a UI can hand back what it displayed; the
/// musician is found by IP, the same join a phone's `mix` uses. The update is validated and merged
/// exactly as a phone's is. Every musician who follows this one's mix takes it at once, each
/// announced after it. An invalid address, an unknown musician or an update that cannot be applied
/// is an error naming the problem, and then nothing changes and nothing is announced.
pub(crate) fn set_target_mix(
    targets: &TargetRegistry,
    events: &EventBus,
    value: &str,
    update: &MixUpdate,
) -> Result<MixValues, String> {
    let (address, mix_state) = musician_at(targets, value)?;
    let applied = mix_state.apply(update)?;
    announce_engine_change(events, address);
    announce_followers(targets, events, address);
    Ok(applied)
}

/// The musician at `value`'s IP, at the address the status reports, and their mix.
fn musician_at(
    targets: &TargetRegistry,
    value: &str,
) -> Result<(SocketAddr, Arc<MixState>), String> {
    let requested = value
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid target `{value}`: {error}"))?;
    targets
        .read()
        .iter()
        .find(|entry| entry.address.ip() == requested.ip())
        .map(|entry| (entry.address, Arc::clone(&entry.mix_state)))
        .ok_or_else(|| format!("target `{value}` is not configured"))
}

/// Announces a change of `address`'s mix made through the engine.
fn announce_engine_change(events: &EventBus, address: SocketAddr) {
    events.emit(EngineEvent::MixChanged {
        address,
        origin: MixOrigin::Engine,
    });
}

/// Makes everyone who follows the mix of the musician at `leader`'s IP take it now (see
/// [TargetRegistry::propagate]) and announces each of them.
fn announce_followers(targets: &TargetRegistry, events: &EventBus, leader: SocketAddr) {
    for address in targets.propagate(leader.ip()) {
        announce_engine_change(events, address);
    }
}

/// The musicians `values` name, for a change of their mixes from the desk: each found by IP, once,
/// in the order named. A value that does not parse or is no musician is an error naming it, and so
/// is a musician whose mix follows another's (a change would only last until the leader's next
/// change): the engineer stops that mirror first.
fn desk_targets(
    targets: &TargetRegistry,
    values: &[String],
) -> Result<Vec<(SocketAddr, Arc<MixState>)>, String> {
    let mut found: Vec<(SocketAddr, Arc<MixState>)> = Vec::with_capacity(values.len());
    for value in values {
        let (address, mix_state) = musician_at(targets, value)?;
        if let Some(leader) = mix_state.following() {
            return Err(format!(
                "{value} follows {leader}'s mix: stop that mirror before changing their mix"
            ));
        }
        if !found.iter().any(|(known, _)| known.ip() == address.ip()) {
            found.push((address, mix_state));
        }
    }
    Ok(found)
}

/// The mix of the musician at `value`'s IP as a preset (see [EngineHandle::mix_preset]).
pub(crate) fn target_preset(
    targets: &TargetRegistry,
    config: &ConfigState,
    value: &str,
) -> Result<MixPreset, String> {
    let (_, mix_state) = musician_at(targets, value)?;
    let live = config.snapshot();
    Ok(MixPreset::of(
        &mix_state.current(),
        &live.layout,
        usize::from(live.source_channels),
    ))
}

/// Puts `preset` on the mix of every musician `musicians` names, as [EngineHandle::apply_preset]
/// does, and announces each change, each followed by those of the musicians who follow them.
pub(crate) fn apply_target_preset(
    targets: &TargetRegistry,
    config: &ConfigState,
    events: &EventBus,
    preset: &MixPreset,
    musicians: &[String],
) -> Result<Vec<(SocketAddr, MixValues)>, String> {
    let found = desk_targets(targets, musicians)?;
    let layout = config.snapshot().layout.clone();
    Ok(found
        .into_iter()
        .map(|(address, mix_state)| {
            let applied = mix_state.apply_preset(preset, &layout);
            announce_engine_change(events, address);
            announce_followers(targets, events, address);
            (address, applied)
        })
        .collect())
}

/// Copies the mix of the musician at `from`'s IP onto every musician `to` names, as
/// [EngineHandle::copy_mix] does, and announces each change, each followed by those of the
/// musicians who follow them.
pub(crate) fn copy_target_mix(
    targets: &TargetRegistry,
    events: &EventBus,
    from: &str,
    to: &[String],
) -> Result<Vec<(SocketAddr, MixValues)>, String> {
    let (source, source_mix) = musician_at(targets, from)?;
    let found = desk_targets(targets, to)?;
    if found.iter().any(|(address, _)| address.ip() == source.ip()) {
        return Err(format!("{from}'s mix cannot be copied onto itself"));
    }
    let values = source_mix.current();
    Ok(found
        .into_iter()
        .map(|(address, mix_state)| {
            let copied = mix_state.follow(&values);
            if copied.is_some() {
                announce_engine_change(events, address);
                announce_followers(targets, events, address);
            }
            (address, copied.unwrap_or_else(|| mix_state.current()))
        })
        .collect())
}

/// The musician a mirror follows, as [EngineHandle::set_mirror] takes it: their address as the
/// status reports it, and the name the follower's phone is told it follows ("Following Bass's
/// mix"); an empty name is the name the leader's phone announced, else their IP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirrorSource<'a> {
    pub address: &'a str,
    pub name: &'a str,
}

/// What [EngineHandle::set_mirror] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MirrorOutcome {
    /// The key the mirror is kept under: the follower's phone's client id, else their address.
    pub follower: MusicianKey,
    /// The leader by the key the mirror keeps, or `None` when the mirror stopped.
    pub leader: Option<MusicianKey>,
    /// The name the follower's phone is told it follows, or `None` when the mirror stopped.
    pub name: Option<String>,
    /// The follower's mix now stored.
    pub mix: MixValues,
}

/// Starts, changes or stops a mirror, as [EngineHandle::set_mirror] does, and announces every mix
/// it moved: the follower's first, then those of the musicians who follow them.
pub(crate) fn set_target_mirror(
    targets: &TargetRegistry,
    events: &EventBus,
    follower: &str,
    leader: Option<MirrorSource<'_>>,
) -> Result<MirrorOutcome, String> {
    let set = targets.set_mirror(follower, leader.map(|source| (source.address, source.name)))?;
    for address in set.changed {
        announce_engine_change(events, address);
    }
    Ok(MirrorOutcome {
        follower: set.follower,
        leader: set.leader,
        name: set.name.map(|name| name.to_string()),
        mix: set.mix,
    })
}

/// Keeps a mirror and puts it in force, as [EngineHandle::keep_mirror] does, announcing every mix
/// it moved.
pub(crate) fn keep_target_mirror(
    targets: &TargetRegistry,
    events: &EventBus,
    follower: &MusicianKey,
    leader: &MusicianKey,
    name: &str,
) -> Result<(), String> {
    for address in targets.keep_mirror(follower, leader, name)? {
        announce_engine_change(events, address);
    }
    Ok(())
}

/// Changes the engineer's controls for the musician whose IP `value` names, keeps them for that
/// musician (see [TargetRegistry::set_controls]) and announces the change as one made through the
/// engine, so the phone is sent its mix with the new limit and locks. Returns the key the controls
/// are kept under and the mix now stored.
///
/// An invalid address, an unknown musician or a lock list longer than the mix is an error naming the
/// problem, and then nothing changes and nothing is announced.
pub(crate) fn set_target_controls(
    targets: &TargetRegistry,
    events: &EventBus,
    value: &str,
    update: &EngineerUpdate,
) -> Result<(MusicianKey, MixValues), String> {
    let (key, address, applied) = targets.set_controls(value, update)?;
    events.emit(EngineEvent::MixChanged {
        address,
        origin: MixOrigin::Engine,
    });
    Ok((key, applied))
}

/// Keeps `controls` for the musician `key` names and puts them in force if they are here, as
/// [EngineHandle::keep_engineer_controls] does; each mix that changed is announced as a change made
/// through the engine.
pub(crate) fn keep_target_controls(
    targets: &TargetRegistry,
    events: &EventBus,
    key: &MusicianKey,
    controls: EngineerControls,
) {
    for address in targets.keep_controls(key, controls) {
        events.emit(EngineEvent::MixChanged {
            address,
            origin: MixOrigin::Engine,
        });
    }
}

/// Replaces every control and mirror the engine keeps with a loaded session's in one step, as
/// [EngineHandle::replace_kept] does (see [TargetRegistry::replace_kept]): each mix that changed is
/// announced as a change made through the engine, those the controls moved first. Returns the
/// mirrors refused.
pub(crate) fn replace_target_kept(
    targets: &TargetRegistry,
    events: &EventBus,
    controls: &[(MusicianKey, EngineerControls)],
    mirrors: &[(MusicianKey, MusicianKey, String)],
) -> Vec<String> {
    let (by_controls, by_mirrors, refused) = targets.replace_kept(controls, mirrors);
    for address in by_controls.into_iter().chain(by_mirrors) {
        announce_engine_change(events, address);
    }
    refused
}

/// What a musician's phone announced about itself when it registered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Announced {
    pub(crate) name: Option<String>,
    pub(crate) client_id: Option<String>,
}

/// What every target announced, by IP: a registered phone's name and client id, each when it sent
/// one. A configured target is the engineer's and announced nothing, so both are `None`.
pub(crate) fn announced_identities(entries: &[TargetEntry]) -> HashMap<IpAddr, Announced> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.address.ip(),
                Announced {
                    name: entry.origin.name().map(str::to_owned),
                    client_id: entry.origin.client_id().map(str::to_owned),
                },
            )
        })
        .collect()
}

/// Joins the per-target state into one row per musician.
///
/// Rows are driven by the target list, so a target with no control connection is still reported.
/// Per-target data that is missing falls back to its neutral value rather than dropping the row.
pub(crate) fn join_musicians(
    targets: &[SocketAddr],
    connected: &HashSet<IpAddr>,
    counters: &HashMap<IpAddr, MusicianCounters>,
    mixes: &HashMap<IpAddr, MixValues>,
    announced: &HashMap<IpAddr, Announced>,
    links: &HashMap<IpAddr, LinkStatus>,
    mirrors: &HashMap<IpAddr, MirrorStatus>,
) -> Vec<MusicianStatus> {
    targets
        .iter()
        .map(|address| {
            let ip = address.ip();
            let announced = announced.get(&ip).cloned().unwrap_or_default();
            MusicianStatus {
                address: *address,
                name: announced.name,
                client_id: announced.client_id,
                control_connected: connected.contains(&ip),
                mix: mixes.get(&ip).copied().unwrap_or_default(),
                counters: counters.get(&ip).copied().unwrap_or_default(),
                link: links.get(&ip).copied().unwrap_or_default(),
                mirror: mirrors.get(&ip).cloned(),
            }
        })
        .collect()
}

/// Classifies every target's latest link report at `now`.
///
/// A stale report is judged by whether the musician's control channel is still connected: the
/// reports travel over the same Wi-Fi as the audio, so a phone that lost its link is still connected
/// but can no longer report it. Each report is copied out under its own short lock and every
/// musician is judged against the same clock read.
pub(crate) fn link_statuses(
    entries: &[TargetEntry],
    connected: &HashSet<IpAddr>,
    now: Instant,
) -> HashMap<IpAddr, LinkStatus> {
    entries
        .iter()
        .map(|entry| {
            let ip = entry.address.ip();
            let report = *entry.link_report.lock().expect("link report lock poisoned");
            (
                ip,
                link_status(report.as_ref(), connected.contains(&ip), now),
            )
        })
        .collect()
}

/// How long a held full-scale peak takes to fall to silence, in milliseconds of audio.
///
/// The one place the meters' release is set. The fall is linear on the 0-100 scale and is
/// measured in audio, not in reads: the writer that raises a level also lowers it by the length of
/// each block, so every reader (the meter stream, the status, a poller) sees the same level however
/// often it reads, and reading never changes a meter.
const LEVEL_FALL_MS: u32 = 1_500;

/// Audio frames at the packaged rate per one-step fall on the 0-100 scale: 720 at 48 kHz.
///
/// Both meters see audio at the packaged rate (the capture decimates before measuring, and every
/// packet carries that rate), so a block's length in frames is its length in time.
const FRAMES_PER_LEVEL: u32 = TARGET_SAMPLE_RATE / 1_000 * LEVEL_FALL_MS / 100;

/// Lowers a held level by `frames` of audio and raises it to this block's `peak`:
/// `max(peak, held - frames)`, so a new peak shows at once and a quieter block lets it fall.
///
/// A held level is stored in frames, one unit per frame of fall, so the arithmetic is exact. Only
/// atomics are touched (no lock, clock or allocation), and `fetch_update` keeps a concurrent
/// writer's raise (the outgoing and incoming streams overlap for a moment during a device switch).
fn hold_peak(held: &AtomicU32, peak: u8, frames: usize) {
    let peak = u32::from(peak) * FRAMES_PER_LEVEL;
    let fall = u32::try_from(frames).unwrap_or(u32::MAX);
    // The closure always returns `Some`, so the update cannot fail.
    let _ = held.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(peak.max(value.saturating_sub(fall)))
    });
}

/// The 0-100 level a held value shows: a pure load. Rounded up, so a peak shows for a whole step
/// before it starts to fall, and only a level that has fallen all the way reads zero.
fn shown_level(held: &AtomicU32) -> u8 {
    held.load(Ordering::Relaxed)
        .div_ceil(FRAMES_PER_LEVEL)
        .min(100) as u8
}

/// How often the engine thread checks the meters for time no audio covered.
///
/// A capture that stops delivering (a device that vanished, a stopped stream) passes no audio time,
/// so an audio-time fall alone would freeze the last level. Every this often the engine thread
/// lowers every meter by the time that passed minus the audio that arrived in it (see
/// [crate::pipeline::Pipeline::tick]), so time is never counted twice.
pub(crate) const METER_IDLE_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// [METER_IDLE_WAIT] in frames at the packaged rate.
pub(crate) const METER_IDLE_FRAMES: usize =
    (TARGET_SAMPLE_RATE as usize) * (METER_IDLE_WAIT.as_millis() as usize) / 1_000;

/// Converts a full-scale-relative sample magnitude into a 0-100 peak.
///
/// Linear in amplitude, not decibels: it costs one integer multiply without a logarithm on the
/// audio thread, and the task only needs a cheap "is there signal here" reading. `i16::MAX`
/// (`32767`) maps to 100 and silence maps to 0.
fn peak_to_level(peak: u16) -> u8 {
    ((u32::from(peak) * 100) / 32767).min(100) as u8
}

/// The 16-bit magnitude of a float peak, the scale the meters have always read: `|x|` times
/// 32768, rounded. A 16-bit source reads exactly its own samples' magnitudes, so the levels are
/// unchanged by the float path.
fn pcm16_magnitude(peak: f32) -> u16 {
    (peak.clamp(0.0, 1.0) * 32_768.0).round() as u16
}

/// Per-source-channel peak levels, measured on the audio thread and held for the UI to read.
///
/// The audio callback raises and lowers each channel's held level with [observe], which is
/// allocation-free and does one max-scan over the already-decimated samples. Reading is a pure
/// load, so any number of readers see the same level.
pub(crate) struct LevelMeter {
    levels: [AtomicU32; MAX_MIX_CHANNELS],
}

impl LevelMeter {
    pub(crate) fn new() -> Self {
        Self {
            levels: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    /// Lets each channel's held level fall by this block's length and raises it to the block's
    /// peak (see [hold_peak]). Called from the audio callback.
    ///
    /// Memory order is `Relaxed` on both sides: a meter reading that lags one callback is
    /// irrelevant, and the value carries no other state, so no synchronisation is warranted. The
    /// scan is one pass over `samples` with no allocation and no clock or logarithm.
    pub(crate) fn observe(&self, samples: &[f32], channels: usize) {
        if channels == 0 || channels > MAX_MIX_CHANNELS || samples.len() % channels != 0 {
            return;
        }
        let frames = samples.len() / channels;
        for (channel, level) in self.levels.iter().enumerate().take(channels) {
            let mut peak = 0.0f32;
            for frame in samples.chunks_exact(channels) {
                peak = peak.max(frame[channel].abs());
            }
            hold_peak(level, peak_to_level(pcm16_magnitude(peak)), frames);
        }
    }

    /// Lets every channel fall by `frames` of silence, for a wait in which no audio arrived.
    pub(crate) fn fall(&self, frames: usize) {
        for level in &self.levels {
            hold_peak(level, 0, frames);
        }
    }

    /// Returns every channel to zero, for a capture that stopped or was replaced.
    pub(crate) fn clear(&self) {
        for level in &self.levels {
            level.store(0, Ordering::Relaxed);
        }
    }

    /// One level per source channel, in channel order; a pure load that changes nothing.
    ///
    /// Channels beyond the metered maximum read as zero, matching the mixer, which also ignores
    /// them.
    pub(crate) fn read(&self, channels: usize) -> Vec<u8> {
        (0..channels)
            .map(|channel| self.levels.get(channel).map_or(0, shown_level))
            .collect()
    }
}

/// The 0-100 peak of each side of an interleaved stereo buffer, left then right.
///
/// One pass, no allocation, the same linear scale as the input meters. A trailing sample that is
/// not a whole frame is left out.
fn stereo_peak(stereo: &[f32]) -> [u8; 2] {
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    for frame in stereo.chunks_exact(2) {
        left = left.max(frame[0].abs());
        right = right.max(frame[1].abs());
    }
    [
        peak_to_level(pcm16_magnitude(left)),
        peak_to_level(pcm16_magnitude(right)),
    ]
}

/// The left and right peak of one musician's mixed output, held for the meter read.
///
/// The send loop updates it right after mixing, so it reads what that musician's phone is sent:
/// after their gains, pans, solos, mutes, master and level cap. Like [LevelMeter], an update costs
/// one pass over the block and two atomic updates, with no allocation, lock or clock, the level
/// falls with the audio, and reading is a pure load.
///
/// A clone shares the same levels (they sit behind one [Arc]), so the network sink can take the
/// meter with the rest of a block's targets and update it after the target table is released.
#[derive(Clone, Default)]
pub(crate) struct OutputMeter {
    levels: Arc<[AtomicU32; 2]>,
}

impl OutputMeter {
    /// Lets the held levels fall by this block's length and raises them to its peaks (see
    /// [hold_peak]). A buffer that is not whole stereo frames is not a mixed output and is ignored,
    /// as [LevelMeter::observe] ignores incomplete frames.
    pub(crate) fn observe(&self, stereo: &[f32]) {
        if !stereo.len().is_multiple_of(2) {
            return;
        }
        let frames = stereo.len() / 2;
        for (level, peak) in self.levels.iter().zip(stereo_peak(stereo)) {
            hold_peak(level, peak, frames);
        }
    }

    /// Lets both sides fall by `frames` of silence, for a wait in which no audio arrived.
    pub(crate) fn fall(&self, frames: usize) {
        for level in self.levels.iter() {
            hold_peak(level, 0, frames);
        }
    }

    /// Returns both sides to zero, for a capture that stopped.
    pub(crate) fn clear(&self) {
        for level in self.levels.iter() {
            level.store(0, Ordering::Relaxed);
        }
    }

    /// The left and right levels; a pure load that changes nothing.
    pub(crate) fn read(&self) -> [u8; 2] {
        [shown_level(&self.levels[0]), shown_level(&self.levels[1])]
    }
}

/// The live levels, cheap enough to read many times a second.
///
/// The meter stream's read: it touches only the held peaks and the target list, never the mixes,
/// the control peers or the link reports, so it costs a fraction of [EngineHandle::status].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineMeters {
    /// One 0-100 input peak per source channel, as [EngineStatus::channel_levels] reports it.
    pub channel_levels: Vec<u8>,
    /// Each musician's output peak, in the order [EngineStatus::musicians] lists them.
    pub musicians: Vec<MusicianMeters>,
    /// The 0-100 peak of the engine's local mix, left then right: metered whether or not a local
    /// output plays it.
    pub local_levels: [u8; 2],
}

/// One musician's output peak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicianMeters {
    /// The musician's address as [EngineStatus::musicians] reports it.
    pub address: SocketAddr,
    /// The 0-100 peak of the stereo mix the musician is sent, left then right.
    pub output_levels: [u8; 2],
}

/// Reads the input levels of `channels` source channels, every musician's output level and the
/// local mix's. A pure read: it changes no meter.
pub(crate) fn read_meters(
    levels: &LevelMeter,
    channels: usize,
    local: &OutputMeter,
    entries: &[TargetEntry],
) -> EngineMeters {
    EngineMeters {
        channel_levels: levels.read(channels),
        musicians: entries
            .iter()
            .map(|entry| MusicianMeters {
                address: entry.address,
                output_levels: entry.output_meter.read(),
            })
            .collect(),
        local_levels: local.read(),
    }
}

/// Returns every input and output meter to zero, the local mix's included.
///
/// A stopped capture delivers no more audio, so no audio time passes for the levels to fall by,
/// and the send loop that would count the silence has ended too; the engine clears the meters
/// instead, so a reader never sees the last level frozen.
pub(crate) fn clear_meters(levels: &LevelMeter, local: &OutputMeter, entries: &[TargetEntry]) {
    levels.clear();
    local.clear();
    for entry in entries {
        entry.output_meter.clear();
    }
}

/// The musician at `address`'s IP, at the address the status reports, with the mix stored now.
///
/// The same IP join as a phone's `mix`, so an address from [EngineEvent::MixChanged] (a control
/// peer's for a phone's change) finds the row the status shows. `None` when the IP is no musician,
/// for instance one removed since the change.
pub(crate) fn musician_mix(
    targets: &TargetRegistry,
    address: SocketAddr,
) -> Option<(SocketAddr, MixValues)> {
    targets
        .read()
        .iter()
        .find(|entry| entry.address.ip() == address.ip())
        // With its exact revision, so a window can order it against the other copies it gets.
        .map(|entry| (entry.address, entry.mix_state.current()))
}

/// Snapshots the start-up group layout into the observation-friendly [GroupStatus] list.
///
/// Used at startup, where [validate_groups] has already proven every group valid.
fn group_statuses(layout: &GroupLayout) -> Vec<GroupStatus> {
    layout
        .groups()
        .iter()
        .map(|group| GroupStatus {
            id: group.id.clone(),
            name: group.name.clone(),
            channels: group.channels.clone(),
            invalid: false,
        })
        .collect()
}

/// The result of re-checking the engineer's groups against a channel count.
///
/// `statuses` preserves the groups for reporting, with invalidity flagged. `mixer_layout` is the
/// membership the mixer may act on: an invalid group contributes no channels, so it cannot affect
/// audio even though it is neither deleted nor modified.
pub(crate) struct GroupRevalidation {
    pub(crate) statuses: Vec<GroupStatus>,
    pub(crate) mixer_layout: GroupLayout,
}

/// Re-checks the engineer's groups against a channel count without failing or mutating them.
///
/// Unlike [validate_groups], which rejects an out-of-range group at startup, this is the lenient
/// check of a running engine (after a device switch or an edit): a channel the device does not have
/// marks the whole group invalid but keeps its definition, so switching back restores it.
/// Definitions are read only.
pub(crate) fn revalidate_groups(
    definitions: &[GroupDefinition],
    channel_count: usize,
) -> GroupRevalidation {
    let mut statuses = Vec::with_capacity(definitions.len());
    let mut mixer_groups = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let invalid = definition
            .channels
            .iter()
            .any(|&channel| channel == 0 || channel > channel_count);
        let channels: Vec<usize> = definition
            .channels
            .iter()
            .map(|&channel| channel.saturating_sub(1))
            .collect();
        statuses.push(GroupStatus {
            id: definition.id.clone(),
            name: definition.name.clone(),
            channels: channels.clone(),
            invalid,
        });
        // Invalid groups keep their slot in the list so group indices line up with `group_levels`,
        // but map no channel; the mixer then cannot apply their level to anything.
        mixer_groups.push(Group {
            id: definition.id.clone(),
            name: definition.name.clone(),
            channels: if invalid { Vec::new() } else { channels },
        });
    }
    GroupRevalidation {
        statuses,
        mixer_layout: GroupLayout::new(mixer_groups),
    }
}

/// Everything a successful device switch changes, computed before anything is mutated.
pub(crate) struct DeviceSwitchPlan {
    pub(crate) source_channels: u8,
    pub(crate) groups: GroupRevalidation,
}

/// Plans the state change for a capture device that now has `new_channels` channels.
///
/// Pure and non-mutating: the previous group definitions are read and re-validated. Whether
/// connected clients must be sent a new `config` is decided when the plan is published (see
/// [apply_groups]): only a configuration that differs from the one in force is re-sent.
pub(crate) fn plan_device_switch(
    definitions: &[GroupDefinition],
    new_channels: u8,
) -> DeviceSwitchPlan {
    DeviceSwitchPlan {
        source_channels: new_channels,
        groups: revalidate_groups(definitions, usize::from(new_channels)),
    }
}

/// Turns a fallible capture rebuild into the plan to commit, or leaves the previous state alone.
///
/// The rebuild touches real hardware and can fail; keeping the conversion to a plan in a pure
/// function is what makes the "a failed switch preserves the previous state" rule testable without
/// an audio device.
pub(crate) fn plan_switch_from_attempt(
    definitions: &[GroupDefinition],
    attempt: Result<u16, Box<dyn Error>>,
) -> Result<DeviceSwitchPlan, Box<dyn Error>> {
    let new_channels = attempt?.min(u16::from(u8::MAX)) as u8;
    Ok(plan_device_switch(definitions, new_channels))
}

/// Publishes `source_channels` and `groups` as the live configuration and moves every musician's
/// mix onto it. Returns whether anything changed; an unchanged configuration touches nothing and
/// makes no connection re-send `config`.
///
/// When every group keeps its position (a device switch, a rename, other channels for a group),
/// each mixer is only pointed at the new membership: every stored level is already in its place.
/// Otherwise every mix moves its group levels by group id ([MixState::regroup]): a kept group takes
/// its level to its new position, a new group starts neutral and a deleted group's level is
/// dropped. Each moved mix takes a new revision and is announced as a change made through the
/// engine, so the desktop and the musician's phone are sent it, after [EngineEvent::GroupsChanged].
///
/// The order is what keeps a phone's levels on the right groups: the new configuration is
/// published before any level moves, so a phone's connection, which reads the mix before it checks
/// the configuration (see [crate::control]), always sends the new `config` ahead of moved levels.
///
/// The registry is held for reading throughout, and a new musician's mix is created under its write
/// lock from the live configuration (see [crate::mix::Membership]). A phone registering at the same
/// moment therefore either exists already and is moved here, or comes after the publish and starts
/// with the new membership; none is left on the old one.
///
/// The engineer's channel details are kept whole and fitted to the new channel count (see
/// [crate::control::ConfigSnapshot::requested_channels]): clients see the details of the inputs the
/// device has, and those of inputs that come back are shown again. [EngineEvent::ChannelsChanged]
/// follows [EngineEvent::GroupsChanged] when what clients see of them changed.
///
/// [MixState::regroup]: crate::mix::MixState::regroup
pub(crate) fn apply_groups(
    targets: &TargetRegistry,
    config: &ConfigState,
    events: &EventBus,
    source_channels: u8,
    groups: GroupRevalidation,
) -> bool {
    let entries = targets.read();
    let Some((previous, live)) =
        config.publish(source_channels, groups.statuses, groups.mixer_layout)
    else {
        return false;
    };
    let layout = &live.layout;
    let mut moved: Vec<SocketAddr> = if previous.layout.same_positions(layout) {
        for entry in entries.iter() {
            entry.mix_state.set_group_layout(layout);
        }
        Vec::new()
    } else {
        let sources = group_sources(&previous.layout, layout);
        entries
            .iter()
            .map(|entry| {
                entry.mix_state.regroup(layout, &sources);
                entry.address
            })
            .collect()
    };
    for address in follow_channels(&entries, &live) {
        if !moved.contains(&address) {
            moved.push(address);
        }
    }
    // Announced once the registry is released, since a listener may read it at once: the new
    // configuration first, then every mix whose levels moved.
    drop(entries);
    events.emit(EngineEvent::GroupsChanged);
    if previous.channels != live.channels {
        events.emit(EngineEvent::ChannelsChanged);
    }
    for address in moved {
        events.emit(EngineEvent::MixChanged {
            address,
            origin: MixOrigin::Engine,
        });
    }
    true
}

/// Gives every mix the channel flags of the configuration `live`, taken from the details as set
/// (see [channel_flags] and [crate::mix::MixState::set_channel_flags]), and returns the musicians
/// whose stored mix changed: those a newly linked pair panned apart. A device switch never changes
/// the flags, so it never moves a pan.
fn follow_channels(entries: &[TargetEntry], live: &ConfigSnapshot) -> Vec<SocketAddr> {
    let flags = channel_flags(&live.requested_channels);
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .mix_state
                .set_channel_flags(flags)
                .map(|_| entry.address)
        })
        .collect()
}

/// Publishes the engineer's channel details `channels`, kept whole and fitted to the live channel
/// count, and moves every musician's mix onto them, as [EngineHandle::set_channels] does. Returns
/// whether anything changed; the details in force again touch nothing and make no connection
/// re-send `config`, and a change only to inputs the device lacks is kept without one.
///
/// The order is the one [apply_groups] keeps: the new `config` is published before any pan moves,
/// so a phone is sent the pair before the pans its link set, and the desktop is told
/// ([EngineEvent::ChannelsChanged], when what it shows changed) before each moved mix
/// ([EngineEvent::MixChanged] from [MixOrigin::Engine]). The registry is held for reading
/// throughout, so a musician registering at the same moment is either moved here or starts from
/// the new details.
pub(crate) fn apply_channels(
    targets: &TargetRegistry,
    config: &ConfigState,
    events: &EventBus,
    channels: &[ChannelMeta],
) -> bool {
    let entries = targets.read();
    let Some((previous, live)) = config.publish_channels(channels) else {
        return false;
    };
    let moved = follow_channels(&entries, &live);
    drop(entries);
    if previous.channels != live.channels {
        events.emit(EngineEvent::ChannelsChanged);
    }
    for address in moved {
        events.emit(EngineEvent::MixChanged {
            address,
            origin: MixOrigin::Engine,
        });
    }
    true
}

/// Replaces the engineer's channel details with `channels` (channel order, the first is CH 1), as
/// [EngineHandle::set_channels] does, and returns the details now in force: one entry per input.
///
/// The details are checked first ([check_channels]); details that fail the check are an error
/// naming the problem, and nothing changes.
pub(crate) fn edit_channels(
    targets: &TargetRegistry,
    config: &ConfigState,
    events: &EventBus,
    channels: &[ChannelMeta],
) -> Result<Vec<ChannelMeta>, String> {
    check_channels(channels)?;
    apply_channels(targets, config, events, channels);
    Ok(config.snapshot().channels.clone())
}

/// The engineer's channel details as `config` keeps them, whole: the list last set, in channel
/// order, entries for inputs the device lacks included (see
/// [crate::control::ConfigSnapshot::requested_channels]). What clients see is the part of it the
/// device has ([EngineStatus::channels]).
pub(crate) fn channel_table(config: &ConfigState) -> Vec<ChannelMeta> {
    config.snapshot().requested_channels.clone()
}

/// Replaces the engineer's groups with `definitions` on a device with `source_channels` inputs, as
/// [EngineHandle::set_groups] does, and returns the groups now in force.
///
/// The definitions are checked as at start-up ([check_groups]); a group naming a channel the device
/// lacks is kept but invalid, as after a device switch. A definition that fails the check is an
/// error naming the problem, and nothing changes.
pub(crate) fn edit_groups(
    targets: &TargetRegistry,
    config: &ConfigState,
    events: &EventBus,
    source_channels: u8,
    definitions: &[GroupDefinition],
) -> Result<Vec<GroupStatus>, String> {
    check_groups(definitions)?;
    let groups = revalidate_groups(definitions, usize::from(source_channels));
    apply_groups(targets, config, events, source_channels, groups);
    Ok(config.snapshot().groups.clone())
}

/// Moves the local mix from the configuration `previous` onto `live`, as [apply_groups] and
/// [apply_channels] move every musician's mix, and returns whether its stored values moved (so it
/// must be announced): group levels that followed their groups to new positions, or a pair that a
/// new link panned apart. A configuration that did not change touches nothing.
fn follow_configuration(
    local: &MixState,
    previous: &ConfigSnapshot,
    live: &ConfigSnapshot,
) -> bool {
    if previous.generation == live.generation {
        return false;
    }
    let mut moved = false;
    if previous.layout.same_positions(&live.layout) {
        local.set_group_layout(&live.layout);
    } else {
        local.regroup(&live.layout, &group_sources(&previous.layout, &live.layout));
        moved = true;
    }
    if local
        .set_channel_flags(channel_flags(&live.requested_channels))
        .is_some()
    {
        moved = true;
    }
    moved
}

/// The live engine. Dropping it does not stop the engine; call [EngineHandle::stop].
pub struct EngineHandle {
    device_name: String,
    capture_format: CaptureFormat,
    control_port: u16,
    /// The input buffer size requested for capture, reused by every device switch so a session keeps
    /// the same latency profile across devices. `None` means the device default.
    buffer_frames: Option<u32>,
    /// The backend the capture stream was opened on, reused by every device switch so a switch never
    /// silently changes the driver the engineer chose.
    host: AudioHost,
    source_channels: u8,
    /// The engineer's groups exactly as configured, kept so a device switch can re-validate them
    /// against the new channel count without ever rewriting them.
    group_definitions: Vec<GroupDefinition>,
    levels: Arc<LevelMeter>,
    /// The live routing table, shared with the network and control threads so targets can be added
    /// and removed while both keep running.
    targets: Arc<TargetRegistry>,
    stopped: Arc<AtomicBool>,
    /// What every capture stream counts: samples delivered and frames dropped.
    capture_stats: Arc<CaptureStats>,
    packet_stats: Arc<PacketStats>,
    peers: ControlPeers,
    /// The live configuration (channel count and groups), shared with the control connections:
    /// what every phone is sent as `config`, and the membership a new musician's mix starts with.
    config_state: Arc<ConfigState>,
    events: EventBus,
    /// The engine's own mix, for the engineer's local output: kept apart from the target table,
    /// since it is no musician and no phone, but moved with every musician's mix when the groups
    /// or the channel details change (see [follow_configuration]).
    local_mix: Arc<MixState>,
    /// The local mix's left and right peak, shared with the local output's sink.
    local_meter: OutputMeter,
    /// The local output: the line to its sink and the output that plays, if any.
    local_output: LocalOutput,
    /// The capture device, kept so an ASIO output can be opened on the driver the capture holds
    /// without listing ASIO devices (which loads every driver). `None` for a test source.
    capture_device: Option<cpal::Device>,
    /// The engine block of the current capture, which sizes a new output's ring.
    block_frames: usize,
    /// The recorder: the lines to its sink on the engine thread, and the take being recorded.
    recorder: Recorder,
    /// The clock a take played back follows: the local output's callbacks, else the timer.
    playback_clock: Arc<PlaybackClock>,
    /// The take playing in place of the inputs, if any.
    soundcheck: Soundcheck,
    /// The running capture stream. Dropping it stops its callback, the only producer of its ring.
    stream: Option<cpal::Stream>,
    /// The engine thread: it reads the current stream's ring and feeds the sinks, and keeps the
    /// block numbering going across device switches.
    pipeline: Option<PipelineHandle>,
    control_thread: Option<JoinHandle<()>>,
    discovery_thread: Option<JoinHandle<()>>,
}

/// A capture source opened and started, not yet the engine's: what [EngineHandle::install_capture]
/// commits. A device opens one through cpal; a test opens one with a [crate::pipeline::CaptureWriter]
/// it drives itself, with no stream.
pub(crate) struct OpenedCapture {
    pub(crate) stream: Option<cpal::Stream>,
    /// The device the stream is open on; `None` for a test source.
    pub(crate) device: Option<cpal::Device>,
    pub(crate) reader: SourceReader,
    pub(crate) device_name: String,
    pub(crate) format: CaptureFormat,
}

/// The engine's side of the local output: the lines to its sink on the engine thread, and the
/// output that plays.
pub(crate) struct LocalOutput {
    /// Behind a lock so a status read (`&self`) can free the feeds the sink handed back.
    control: Mutex<OutputControl>,
    active: Option<ActiveOutput>,
}

/// The output the local mix plays on.
struct ActiveOutput {
    /// The device stream; dropping it stops the output. `None` for a test output, and once a
    /// switch to another ASIO driver stopped it.
    stream: Option<cpal::Stream>,
    request: LocalOutputRequest,
    shared: Arc<OutputShared>,
}

impl LocalOutput {
    /// No output yet, with `control` the engine's end of the local output sink's lines.
    pub(crate) fn new(control: OutputControl) -> Self {
        Self {
            control: Mutex::new(control),
            active: None,
        }
    }

    /// Frees, here, the feeds the sink has let go of.
    fn reap(&self) {
        if let Ok(mut control) = self.control.lock() {
            control.reap();
        }
    }
}

impl OpenedCapture {
    /// The channel count the session would switch to: the reported format's, which must be the
    /// layout of the frames in the ring, or every block would be read with the wrong stride.
    fn channels(&self) -> Result<u16, Box<dyn Error>> {
        let ring = self.reader.format().channels;
        if usize::from(self.format.channels) != ring {
            return Err(format!(
                "the capture reports {} channels but its frames carry {ring}",
                self.format.channels
            )
            .into());
        }
        Ok(self.format.channels)
    }
}

impl EngineHandle {
    /// The flag the caller sets to stop the engine, for use from a signal handler.
    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stopped)
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Subscribes to the engine's events. Each call returns a fresh receiver.
    pub fn subscribe(&self) -> Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn capture_format(&self) -> &CaptureFormat {
        &self.capture_format
    }

    pub fn control_port(&self) -> u16 {
        self.control_port
    }

    /// A snapshot of the current state, including the joined per-musician view.
    pub fn status(&self) -> EngineStatus {
        let connected: HashSet<IpAddr> = self
            .peers
            .lock()
            .expect("control peer registry poisoned")
            .keys()
            .copied()
            .collect();
        let entries = self.targets.read();
        let targets: Vec<SocketAddr> = entries.iter().map(|entry| entry.address).collect();
        let counters: HashMap<IpAddr, MusicianCounters> = entries
            .iter()
            .map(|entry| {
                (
                    entry.address.ip(),
                    MusicianCounters {
                        packets_sent: entry.counters.sent.load(Ordering::Relaxed),
                        packets_discarded: entry.counters.discarded.load(Ordering::Relaxed),
                    },
                )
            })
            .collect();
        let mixes: HashMap<IpAddr, MixValues> = entries
            .iter()
            .map(|entry| (entry.address.ip(), entry.mix_state.current()))
            .collect();
        let announced = announced_identities(&entries);
        let links = link_statuses(&entries, &connected, Instant::now());
        let mirrors = self.targets.mirror_statuses(&entries);
        let config = self.config_state.snapshot();
        EngineStatus {
            device_name: self.device_name.clone(),
            capture_format: self.capture_format.clone(),
            control_port: self.control_port,
            sample_rate: TARGET_SAMPLE_RATE,
            source_channels: self.source_channels,
            groups: config.groups.clone(),
            channels: config.channels.clone(),
            channel_levels: self.levels.read(usize::from(self.source_channels)),
            stopped: self.is_stopped(),
            samples_received: self.capture_stats.samples.load(Ordering::Relaxed),
            packets_sent: self.packet_stats.sent.load(Ordering::Relaxed),
            // Audio the phones never got: capture the ring had no room for, and failed sends.
            packets_discarded: self.packet_stats.discarded.load(Ordering::Relaxed)
                + self.capture_stats.dropped_blocks.load(Ordering::Relaxed),
            musicians: join_musicians(
                &targets, &connected, &counters, &mixes, &announced, &links, &mirrors,
            ),
            local_mix: self.local_mix.current(),
            local_output: self.local_output(),
            recording: self.recording(),
            soundcheck: self.soundcheck(),
        }
    }

    /// The live input and output levels, without building the rest of the status.
    ///
    /// A pure read, like the status: any number of readers can poll it at any rate without
    /// changing the levels (see [EngineStatus::channel_levels]).
    pub fn meters(&self) -> EngineMeters {
        read_meters(
            &self.levels,
            usize::from(self.source_channels),
            &self.local_meter,
            &self.targets.read(),
        )
    }

    /// The musician at `address`'s IP, at the address [Self::status] reports, and their current mix.
    ///
    /// This is the join a consumer of [EngineEvent::MixChanged] needs: a phone's change names the
    /// control peer, whose port is not the musician's. `None` when that IP is no musician.
    pub fn musician_mix(&self, address: SocketAddr) -> Option<(SocketAddr, MixValues)> {
        musician_mix(&self.targets, address)
    }

    /// Switches the input device while the rest of the engine keeps running.
    ///
    /// Only the capture stream is rebuilt: the engine thread, the control WebSocket server and the
    /// discovery beacon are untouched, and the block numbering continues from where it was. The
    /// new stream is built and started before the old one is dropped, so a device that has vanished
    /// or a format that cannot be opened leaves the previous capture exactly as it was and returns
    /// the error. The new stream feeds its own ring, and the engine thread moves to it between two
    /// blocks, so the phones never get a block that mixes the two devices. On success, groups are
    /// re-validated (never rewritten), the channel details are shown for the inputs the new device
    /// has and kept for the rest ([Self::set_channels]), and connected clients are sent the new
    /// `config` when any of it changed.
    ///
    /// A switch is refused while a take records ([Self::start_recording]): a take never mixes two
    /// devices, so the take is stopped first. It is refused too while a take plays in place of the
    /// inputs ([Self::start_soundcheck]), which was checked against this device's inputs.
    pub fn switch_device(&mut self, filter: Option<&str>) -> Result<CaptureFormat, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        self.recorder.refuse_source_change()?;
        self.soundcheck.refuse_source_change()?;
        let device = {
            let host = input_host(self.host);
            let devices = host
                .input_devices()
                .map_err(|error| format!("could not enumerate input devices: {error}"))?
                .collect::<Vec<_>>();
            match filter {
                Some(name) => select_device_by_name(devices, name)?,
                None => select_device(devices, None)?,
            }
        };
        let supported_config = select_input_config(&device)?;
        let links = self.capture_links()?;

        // Build and start the replacement before touching the current capture, so any failure below
        // is a no-op for the running session.
        let opened = build_input_stream(&device, &supported_config, self.buffer_frames, &links)?;
        opened
            .stream
            .play()
            .map_err(|error| format!("could not start input capture: {error}"))?;

        self.install_capture(OpenedCapture {
            stream: Some(opened.stream),
            device: Some(device.clone()),
            reader: opened.reader,
            device_name: device.to_string(),
            format: CaptureFormat {
                channels: supported_config.channels(),
                sample_rate: supported_config.sample_rate(),
                sample_format: format!("{:?}", supported_config.sample_format()),
                buffer_size: describe_buffer(opened.buffer_frames),
            },
        })
    }

    /// What a new capture stream writes to: the input meters, the capture counters and the engine
    /// thread to wake.
    fn capture_links(&self) -> Result<CaptureLinks, Box<dyn Error>> {
        let pipeline = self.pipeline.as_ref().ok_or("the engine is stopping")?;
        Ok(CaptureLinks {
            levels: Arc::clone(&self.levels),
            stats: Arc::clone(&self.capture_stats),
            wake: Some(pipeline.waker()),
        })
    }

    /// Makes an opened, running capture the engine's: the engine thread reads its ring from the
    /// next block on, the previous stream is dropped, and the session takes its channel count.
    ///
    /// A plan that cannot be made (a capture whose format and frames disagree), or an engine
    /// thread that is no longer running, leaves everything as it was: the running capture keeps
    /// feeding the engine and the new one is dropped. So does a take that records, or one that
    /// plays in place of the inputs.
    pub(crate) fn install_capture(
        &mut self,
        capture: OpenedCapture,
    ) -> Result<CaptureFormat, Box<dyn Error>> {
        self.recorder.refuse_source_change()?;
        self.soundcheck.refuse_source_change()?;
        let plan = plan_switch_from_attempt(&self.group_definitions, capture.channels())?;
        let pipeline = self.pipeline.as_ref().ok_or("the engine is stopping")?;

        // Commit: the engine thread moves to the new ring between two blocks, then the old stream,
        // the only producer of the old ring, is stopped.
        let block_frames = capture.reader.format().block_frames;
        pipeline.set_source(capture.reader)?;
        self.stream = capture.stream;
        self.capture_device = capture.device;
        self.block_frames = block_frames;
        self.device_name = capture.device_name;
        self.capture_format = capture.format;
        self.source_channels = plan.source_channels;
        self.keep_asio_output_on_the_capture_driver();
        // Every phone is sent the new `config` (channel count and groups) when it differs, and every
        // musician, connected now or registering later, mixes with the new membership.
        self.publish(|| {
            apply_groups(
                &self.targets,
                &self.config_state,
                &self.events,
                plan.source_channels,
                plan.groups,
            )
        });
        // The old device's levels describe a stream that is gone, and channels the new device lacks
        // would keep theirs frozen until a switch back; the new stream raises its own at once.
        self.levels.clear();
        Ok(self.capture_format.clone())
    }

    /// Replaces the engineer's groups while the engine runs, and returns the groups now in force.
    ///
    /// The definitions are checked as at start-up: at most 16 groups, each with a non-empty id no
    /// other group has and a non-empty name, and no channel in two groups. A group naming a channel
    /// the current device lacks is kept but invalid (it maps no channel), exactly as after a device
    /// switch, so the definition survives until a device that has the channel is back.
    ///
    /// Every musician's group levels follow their groups by id: a renamed group, or one given other
    /// channels, keeps its level; a moved one takes it to its new position; a new group starts at
    /// 100; a deleted group's level is dropped. Each phone is sent the new `config` and then, when
    /// levels moved, its mix as `mix_state`, in that order (see [apply_groups]); each moved mix is
    /// also announced as [EngineEvent::MixChanged] from [MixOrigin::Engine]. Capture, the network,
    /// the control server and the discovery beacon keep running.
    ///
    /// Definitions that fail the check are an error naming the problem, and nothing changes.
    pub fn set_groups(
        &mut self,
        definitions: Vec<GroupDefinition>,
    ) -> Result<Vec<GroupStatus>, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        let groups = self.publish(|| {
            edit_groups(
                &self.targets,
                &self.config_state,
                &self.events,
                self.source_channels,
                &definitions,
            )
        })?;
        self.group_definitions = definitions;
        Ok(groups)
    }

    /// Replaces the engineer's channel details while the engine runs, and returns the details now
    /// in force: one entry per input of the current device, in channel order.
    ///
    /// `channels` is in channel order (the first entry is CH 1) and is checked first
    /// ([check_channels]): at most 32 entries, names of at most [MAX_CHANNEL_NAME_LENGTH]
    /// characters, stereo pairs only between two listed channels and never overlapping, and both
    /// sides of a pair hidden or both shown. The engine keeps the list whole, as it keeps the group
    /// definitions: clients are shown the entries of the inputs the device has, inputs past the
    /// list get the defaults ([ChannelMeta::default]), and after a device switch the entries of
    /// inputs that come back are shown again, names, pairs and hidden flags included. While its
    /// input is missing a hidden spare stays hidden; a pair whose second side is missing is shown
    /// as its first input alone, stays linked in every mix and returns with its balance. A change
    /// only to entries past the device is kept without sending `config`.
    ///
    /// Every phone is sent the details at once in `config`. A hidden channel is silent in every
    /// musician's mix from the next packet on; showing it again restores it, since hiding changes
    /// no stored level. Linking a pair pans it apart in every musician's mix (the first channel 0,
    /// the second 100) and gives its second side the first side's level, mute and solo; each such
    /// mix takes a new revision, is sent to its phone as `mix_state` after the new `config`, and is
    /// announced as [EngineEvent::MixChanged] after [EngineEvent::ChannelsChanged]. From then on,
    /// whoever changes a pair's level, mute or solo (a phone or [Self::set_mix]) changes both
    /// sides: a change to one side moves the other, and when one update changes both sides to
    /// different values the first channel's value wins. A pair's pans stay as written: its balance
    /// is the client's, which writes both pans.
    ///
    /// Details that fail the check are an error naming the problem, and nothing changes.
    pub fn set_channels(
        &mut self,
        channels: Vec<ChannelMeta>,
    ) -> Result<Vec<ChannelMeta>, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        Ok(self.publish(|| {
            edit_channels(&self.targets, &self.config_state, &self.events, &channels)
        })?)
    }

    /// Runs `change`, which may publish a new configuration (groups, channel details or a device's
    /// channel count) and move every musician's mix onto it, then moves the local mix onto it too
    /// and announces it when its values moved: after the change's own events.
    fn publish<T>(&self, change: impl FnOnce() -> T) -> T {
        let previous = self.config_state.snapshot();
        let result = change();
        let live = self.config_state.snapshot();
        if follow_configuration(&self.local_mix, &previous, &live) {
            self.events.emit(EngineEvent::LocalMixChanged);
        }
        result
    }

    /// The engine's local mix, with its revision: the engineer's own mix for the local output
    /// (see [crate::EngineHandle::set_local_mix]).
    pub fn local_mix(&self) -> MixValues {
        self.local_mix.current()
    }

    /// Applies a partial update to the engine's local mix and returns the mix now stored.
    ///
    /// It is merged exactly as the desk merges a musician's ([Self::set_mix]): absent fields keep
    /// their values, levels are clamped, a list longer than its table is refused, both sides of a
    /// linked pair stay equal, a hidden input stays silent and the groups are the live ones. It has
    /// no engineer's controls and follows nobody. Every stored change takes a new revision and is
    /// announced as [EngineEvent::LocalMixChanged]; an update that cannot be applied is an error
    /// naming the problem, and then nothing changes and nothing is announced.
    pub fn set_local_mix(&self, update: &MixUpdate) -> Result<MixValues, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        let applied = self.local_mix.apply(update)?;
        self.events.emit(EngineEvent::LocalMixChanged);
        Ok(applied)
    }

    /// The output devices the local mix can be played on, with their output channel counts.
    ///
    /// Under ASIO only the capture driver's own outputs are offered, read from the device the
    /// capture holds: ASIO holds one driver at a time, and listing the others would load each of
    /// them. Under the system host (the default; WASAPI on Windows) every output device is listed.
    /// No ASIO driver is ever opened for this list.
    pub fn output_devices(&self) -> Result<Vec<OutputDevice>, Box<dyn Error>> {
        Ok(offered_outputs(
            self.host,
            self.capture_output(),
            system_output_devices,
        )?)
    }

    /// The capture device as an output, when the capture runs on ASIO: its name and output
    /// channels, from the metadata the driver gave when the capture opened it.
    fn capture_output(&self) -> Option<OutputDevice> {
        if self.host != AudioHost::Asio {
            return None;
        }
        self.capture_device.as_ref().map(|device| OutputDevice {
            name: device.to_string(),
            channels: output_channels(device),
        })
    }

    /// Plays the local mix on `request`'s device and stereo pair, or turns the local output off
    /// with `None` (the default: the engine starts with no local output). Returns the local
    /// output's status.
    ///
    /// The device is named exactly and must be one [Self::output_devices] offers (under ASIO, the
    /// capture's own driver), with the pair within its outputs; it must run at 48 kHz, as there is
    /// no resampler yet. The change is live: the new output is opened and started first, the
    /// engine thread moves the local mix to it between two blocks, then the previous output is
    /// closed. Capture and the phones never notice. A request that is refused or a device that
    /// cannot be opened is an error, and the output in use keeps playing. A device that fails
    /// later stops only the local output, which the status reports with the reason.
    pub fn set_local_output(
        &mut self,
        request: Option<LocalOutputRequest>,
    ) -> Result<LocalOutputStatus, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        let Some(request) = request else {
            self.install_output(None)?;
            return Ok(self.local_output());
        };
        check_request(self.host, &self.output_devices()?, &request)?;
        let device = match self.host {
            AudioHost::Asio => self
                .capture_device
                .clone()
                .ok_or("no ASIO driver is open for the local output")?,
            AudioHost::Auto | AudioHost::Wasapi => system_output_device(&request.device)?,
        };
        let opened = open_output(
            &device,
            self.host,
            &request,
            self.buffer_frames,
            self.block_frames,
            Arc::clone(&self.playback_clock),
        )?;
        self.install_output(Some(opened))?;
        Ok(self.local_output())
    }

    /// Makes `output` the local output, or turns it off with `None`: the local output's sink takes
    /// the new feed before its next block, then the previous output's stream is dropped. The feed
    /// the sink lets go of comes back and is freed here, at a later call, never on the engine
    /// thread. A take played back follows the new output's callbacks, or the timer without one.
    pub(crate) fn install_output(
        &mut self,
        output: Option<OpenedOutput>,
    ) -> Result<(), Box<dyn Error>> {
        let follow = output.is_some();
        let (feed, active) = match output {
            Some(output) => (
                Some(output.feed),
                Some(ActiveOutput {
                    stream: output.stream,
                    request: output.request,
                    shared: output.shared,
                }),
            ),
            None => (None, None),
        };
        self.local_output
            .control
            .get_mut()
            .map_err(|_| "the local output's lines were poisoned")?
            .send(feed)?;
        self.local_output.active = active;
        self.playback_clock.follow_output(follow);
        Ok(())
    }

    /// Where the local mix plays and how: off, running, or failed with the reason, and the frames
    /// padded and dropped to hold its delay. Also frees any output the sink has let go of.
    pub fn local_output(&self) -> LocalOutputStatus {
        self.local_output.reap();
        match &self.local_output.active {
            Some(active) => active.shared.status(&active.request),
            None => LocalOutputStatus::default(),
        }
    }

    /// Under ASIO the local output must be on the driver the capture holds: after a switch to
    /// another driver it is stopped and reported as failed, with the reason, until the engineer
    /// chooses an output of the new driver.
    fn keep_asio_output_on_the_capture_driver(&mut self) {
        if self.host != AudioHost::Asio {
            return;
        }
        let Some(active) = &mut self.local_output.active else {
            return;
        };
        if active.request.device != self.device_name {
            active.stream = None;
            active.shared.fail(format!(
                "the capture moved to the ASIO driver \"{}\"; an ASIO output plays only on the capture's driver",
                self.device_name
            ));
        }
    }

    /// Starts recording a take: every armed input to its own file, and the local mix when asked
    /// (see [RecordRequest]). Returns the recorder's status.
    ///
    /// The take is a new folder in `request.folder`, `<YYYY-MM-DD_HHMMSS>_<take name>` (local time
    /// by `utc_offset_minutes`), never an existing one: a mono file per armed input, a stereo file
    /// per armed linked pair (arming either side arms the pair) and a stereo file of the local mix,
    /// named after the inputs' names at the start, and `take.json`. The track layout is fixed
    /// here: the input device cannot be switched until the take stops ([Self::switch_device]).
    /// Recording starts with the next block and never disturbs capture, the phones or the local
    /// output: a disk that falls behind costs dropped frames (counted), and a disk error stops
    /// only the take ([RecordingState::Failed]).
    ///
    /// An error, and nothing recorded or left behind, when a take already records, an armed input
    /// is not on the device, nothing is armed, the format is not 24-bit integer or 32-bit float,
    /// or the folder or a file cannot be made.
    pub fn start_recording(
        &mut self,
        request: RecordRequest,
    ) -> Result<RecordingStatus, Box<dyn Error>> {
        self.start_recording_with(request, FileOpener, TakeOptions::default())
    }

    /// [Self::start_recording] with the files opened by `opener` and the take set up with
    /// `options`, so a test can fail a disk on demand.
    pub(crate) fn start_recording_with<O: TrackOpener>(
        &mut self,
        request: RecordRequest,
        opener: O,
        options: TakeOptions,
    ) -> Result<RecordingStatus, Box<dyn Error>> {
        if self.is_stopped() || self.pipeline.is_none() {
            return Err("the engine is stopping".into());
        }
        self.soundcheck.refuse_recording()?;
        self.recorder.check_idle()?;
        let source_channels = usize::from(self.source_channels);
        let tracks = plan_tracks(
            &self.config_state.snapshot().channels,
            source_channels,
            &request.armed,
            request.include_local,
        )?;
        let (feed, take) = open_take(
            &request,
            &self.device_name,
            tracks,
            source_channels,
            std::time::SystemTime::now(),
            opener,
            options,
        )?;
        self.recorder.start(feed, take)?;
        Ok(self.recording())
    }

    /// Stops the take and returns it once its files are finished and its `take.json` written.
    /// A take that stopped on its own (a disk error) is collected the same way, with its reason.
    /// An error when nothing records, or when the files are still being finished after a few
    /// seconds (ask again).
    pub fn stop_recording(&mut self) -> Result<TakeSummary, Box<dyn Error>> {
        self.recorder.request_stop(TakeEnd::Stopped)?;
        Ok(self.recorder.wait(STOP_WAIT)?)
    }

    /// The take being recorded: its state, folder, frames, dropped frames, bytes, tracks and
    /// markers, the free space on its disk and the minutes of it that space holds, and the last
    /// take that ended.
    pub fn recording(&self) -> RecordingStatus {
        self.recorder.status()
    }

    /// Marks the take being recorded with `name` (`Marker n` when blank, at most 64 characters)
    /// at the first frame of the block being recorded, and returns the marker. `take.json` lists
    /// the markers when the take ends. An error when no take records.
    pub fn add_marker(&self, name: &str) -> Result<TakeMarker, Box<dyn Error>> {
        Ok(self.recorder.add_marker(name)?)
    }

    /// What a take of `request` would take on the disk: its bytes per second (the armed tracks
    /// as [Self::start_recording] would lay them out), the free space on `request.folder`'s disk
    /// and the minutes it holds. An error when the request could not start a take (nothing
    /// armed, an input the device lacks).
    pub fn record_estimate(&self, request: &RecordRequest) -> Result<DiskEstimate, Box<dyn Error>> {
        let tracks = plan_tracks(
            &self.config_state.snapshot().channels,
            usize::from(self.source_channels),
            &request.armed,
            request.include_local,
        )?;
        let bytes_per_second = take_bytes_per_second(&tracks, request.format);
        let free_bytes = free_space(&request.folder);
        Ok(DiskEstimate {
            bytes_per_second,
            free_bytes,
            minutes_left: free_bytes.and_then(|free| minutes_left(free, bytes_per_second)),
        })
    }

    /// Plays the take in `request.take_folder` in place of the inputs (a virtual soundcheck), from
    /// `request.start_frame`, looping or not, and returns the soundcheck's status.
    ///
    /// The take's input tracks feed the mixer as the capture device did, in the same blocks and
    /// with the same channels: every phone's mix, the local mix and output and the meters behave
    /// as live, and inputs the take did not record are silent. Its local-mix track is not played.
    /// The capture device keeps running, set aside: its audio is neither heard nor metered until
    /// [Self::stop_soundcheck] takes the engine back to it. The take follows the local output's
    /// clock when one plays, else a timer (see [crate::soundcheck]).
    ///
    /// A [SoundcheckError], and nothing changed, when the engine is stopping, a take records, a
    /// soundcheck already plays, `take.json` or a track's file is missing or unreadable, the take
    /// or a file is not at 48 kHz, its inputs do not fit the device's inputs now (an input past
    /// them, or a pair that is not a linked pair now, or a lone input that now is), it holds no
    /// input audio, or the start is past its end. While it plays, recording and device switches
    /// are refused.
    pub fn start_soundcheck(
        &mut self,
        request: SoundcheckRequest,
    ) -> Result<SoundcheckStatus, Box<dyn Error>> {
        if self.is_stopped() {
            return Err(SoundcheckError::EngineStopping.into());
        }
        let Some(pipeline) = self.pipeline.as_ref() else {
            return Err(SoundcheckError::EngineStopping.into());
        };
        if self.soundcheck.is_active() {
            return Err(SoundcheckError::AlreadyPlaying.into());
        }
        if self.recorder.refuse_source_change().is_err() {
            return Err(SoundcheckError::Recording.into());
        }
        let follow_output = self.local_output.active.as_ref().is_some_and(|active| {
            active.shared.status(&active.request).state == LocalOutputState::Running
        });
        self.playback_clock.follow_output(follow_output);
        let (reader, active) = crate::soundcheck::prepare(
            &request,
            &self.config_state.snapshot().channels,
            usize::from(self.source_channels),
            Arc::clone(&self.playback_clock),
            Arc::clone(&self.capture_stats),
        )?;
        self.playback_clock.set_active(true);
        if let Err(reason) = pipeline.play(reader) {
            self.playback_clock.set_active(false);
            active.end(std::time::Duration::ZERO);
            return Err(SoundcheckError::Unavailable { reason }.into());
        }
        self.soundcheck.begin(active);
        self.events.emit(EngineEvent::SoundcheckChanged);
        Ok(self.soundcheck())
    }

    /// Stops the take playing in place of the inputs and takes the engine back to the capture
    /// device, which never stopped: from the next block the phones, the local output and the
    /// meters hear the inputs again. No device is opened or listed. Returns the soundcheck's
    /// status (off). A [SoundcheckError::NotPlaying] when no take plays.
    pub fn stop_soundcheck(&mut self) -> Result<SoundcheckStatus, Box<dyn Error>> {
        let active = self.soundcheck.take().ok_or(SoundcheckError::NotPlaying)?;
        self.playback_clock.set_active(false);
        if let Some(pipeline) = &self.pipeline {
            // An engine thread that has ended let go of the take with everything else.
            let _ = pipeline.go_live();
        }
        active.end(crate::soundcheck::STOP_WAIT);
        self.events.emit(EngineEvent::SoundcheckChanged);
        Ok(self.soundcheck())
    }

    /// The take playing in place of the inputs: its state, folder, name, position and length in
    /// frames, whether it loops, the frames the disk fell behind by and the clock it follows; the
    /// default (off) when the inputs are live.
    pub fn soundcheck(&self) -> SoundcheckStatus {
        self.soundcheck.status()
    }

    /// The engineer's channel details the engine keeps, whole: the list last set with
    /// [Self::set_channels], entries for inputs the current device lacks included, so an editor
    /// that changes one entry can send the whole list back without losing the others.
    /// [EngineStatus::channels] holds only the details of the device's inputs.
    pub fn channel_table(&self) -> Vec<ChannelMeta> {
        channel_table(&self.config_state)
    }

    /// Adds a UDP target while the engine runs.
    ///
    /// The value is resolved and validated before anything changes; a value that cannot be resolved,
    /// or whose IP is already configured, is rejected with an error naming the value and the table
    /// is left exactly as it was. On success the target joins the live routing table with a neutral
    /// mix carrying the current group layout and zero counters, and the next packet is sent to it.
    /// The capture stream, the UDP network thread, the control server and the discovery beacon all
    /// keep running, exactly as they do across a device switch.
    pub fn add_target(&self, value: &str) -> Result<SocketAddr, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        self.targets
            .add(value, &*self.config_state)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Removes a UDP target while the engine runs.
    ///
    /// ## What happens to the removed musician
    ///
    /// - Their audio stops immediately: the address leaves the routing table, so the next packet is
    ///   sent to the remaining targets only. The bound is one packet interval, tens of milliseconds
    ///   at 48 kHz packaging, not a restart or a timeout.
    /// - Their control channel is **not** forcibly closed. The connection stays open and the next
    ///   `mix` message is answered with the existing `no UDP target configured for client IP`
    ///   error, which the Android client already surfaces. Nothing is silently dropped and no new
    ///   protocol message is introduced: the address is simply no longer in the routing table,
    ///   which is the same answer an unknown IP has always received.
    /// - No other target is disturbed: every other entry keeps its mix state, its counters and its
    ///   stream, and the control server, the discovery beacon and the capture stream keep running.
    pub fn remove_target(&self, value: &str) -> Result<SocketAddr, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        self.targets
            .remove(value)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Changes a musician's mix from outside their phone, as the desktop does.
    ///
    /// `value` is the musician's address as [Self::status] reports it; the musician is found by IP.
    /// The update is partial: every absent field keeps its value, lists replace from the first
    /// channel on, and levels are clamped, exactly as for a phone's `mix`. The change is applied to
    /// the audio at once, emitted as [EngineEvent::MixChanged] with [MixOrigin::Engine], and pushed
    /// to the musician's phone as `mix_state` so its screen follows. Every musician who follows
    /// this one's mix ([Self::set_mirror]) takes it at once, announced the same way. Returns the mix
    /// now stored.
    ///
    /// The desk may change any value of any musician, locked channels and the mix of a follower
    /// included; a follower's channels and groups then stay so only until their leader's mix next
    /// changes.
    ///
    /// An address that does not parse, a musician that is not configured, or a list longer than the
    /// mix holds is an error naming the problem, and the mix is left exactly as it was.
    pub fn set_mix(&self, value: &str, update: &MixUpdate) -> Result<MixValues, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        set_target_mix(&self.targets, &self.events, value, update)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// The mix of the musician at `value`'s IP as a preset ([MixPreset]): the level, pan, mute and
    /// solo of each input of the current device, and every group level by the group's id. It holds
    /// neither their master volume, their phone's level cap, their master mute nor the engineer's
    /// controls. An address that does not parse or is no musician is an error naming it.
    pub fn mix_preset(&self, value: &str) -> Result<MixPreset, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        target_preset(&self.targets, &self.config_state, value)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Puts `preset` on the mix of every musician `musicians` names (addresses as [Self::status]
    /// reports them), from the desk: each listed channel's level, pan, mute and solo, locked
    /// channels included, and the level of every live group the preset names by id, so a preset
    /// saved before the groups were edited still lands on the right groups. Both sides of a stereo
    /// pair stay equal (the first side wins where the preset holds them apart). The master volume,
    /// the phone's level cap, the master mute and the engineer's controls are left alone.
    ///
    /// Each mix takes a new revision, is announced as [EngineEvent::MixChanged] from
    /// [MixOrigin::Engine] and pushed to the phone, and every musician who follows one of them
    /// takes it too. Returns each musician's address and mix now stored, in the order named (a
    /// musician named twice once).
    ///
    /// An address that does not parse, a musician who is not here, or one whose mix follows
    /// another's ([Self::set_mirror]) is an error naming them, and nobody's mix changes.
    pub fn apply_preset(
        &self,
        preset: &MixPreset,
        musicians: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        apply_target_preset(
            &self.targets,
            &self.config_state,
            &self.events,
            preset,
            musicians,
        )
        .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Copies the mix of the musician at `from`'s IP onto every musician `to` names, from the
    /// desk: every channel's level, pan, mute and solo and every group level, as [Self::apply_preset]
    /// puts a preset on them, never the master volume, the phone's level cap, the master mute or the
    /// engineer's controls. A musician whose mix already held those values is left alone. Returns
    /// each musician's address and mix now stored, in the order named.
    ///
    /// `to` may not name `from`; otherwise the errors are those of [Self::apply_preset], and nobody's
    /// mix changes.
    pub fn copy_mix(
        &self,
        from: &str,
        to: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        copy_target_mix(&self.targets, &self.events, from, to)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Makes the mix of the musician at `follower`'s IP follow the mix of `leader` (a mirror), or
    /// stops it with `None`. A mirror is one-way:
    ///
    /// - Starting it copies the leader's mix at once (as [Self::copy_mix] does), and from then on
    ///   every change of the leader's channels and group levels, from their phone, the desk, a
    ///   preset or a copy, is copied at once to the follower, and on to whoever follows the
    ///   follower. Each copy is a change through the engine with a new revision, pushed to the
    ///   follower's phone.
    /// - While it runs, the follower's phone cannot change any channel or group level (the server
    ///   keeps them, as it keeps a locked channel), while its master volume, level cap and mute stay
    ///   its own; every report to it names the leader (`following`) so it can say so.
    /// - Stopping it leaves the follower's mix as it is and frees their phone.
    ///
    /// The engine keeps the mirror for the follower (under their phone's client id when it announced
    /// one, else their address, the key it returns), so it is put in force again whenever they
    /// register, and they take the leader's mix then. A leader who is not here keeps the follower
    /// held to the last mix they took; the status says the leader is away
    /// ([MirrorStatus::leader_address]).
    ///
    /// A musician cannot follow their own mix, nor one that already follows theirs, directly or
    /// through others: a mirror never goes round in a circle. That, an address that does not parse
    /// or a musician who is not here is an error naming it, and nothing changes.
    pub fn set_mirror(
        &self,
        follower: &str,
        leader: Option<MirrorSource<'_>>,
    ) -> Result<MirrorOutcome, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        set_target_mirror(&self.targets, &self.events, follower, leader)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Keeps a mirror the desk saved: the musician `follower` names follows the one `leader` names,
    /// shown to the follower's phone as `name`, whether or not either is here, as
    /// [Self::keep_engineer_controls] keeps controls. A follower who is here is put on it at once
    /// (taking the leader's mix when the leader is here too), one who registers later when they do.
    /// A mirror on oneself or one that would go round in a circle is an error, and nothing changes.
    pub fn keep_mirror(
        &self,
        follower: &MusicianKey,
        leader: &MusicianKey,
        name: &str,
    ) -> Result<(), Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        keep_target_mirror(&self.targets, &self.events, follower, leader, name)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Sets what the engineer allows one musician, beyond their mix: a limit on what they hear and
    /// the channels their phone cannot change (see [EngineerControls]). Only this engine path sets
    /// them; a phone's `mix` never can.
    ///
    /// `value` is the musician's address as [Self::status] reports it; the musician is found by IP.
    /// The update is partial (see [EngineerUpdate]): an absent field keeps its value, `locked`
    /// replaces each channel's lock from CH 1 on, and both sides of a stereo pair lock together.
    ///
    /// - The limit caps the musician's output at once: the mixer's ceiling is the lower of it and
    ///   the phone's own level cap (`max_level_percent`), which the phone keeps setting.
    /// - From the phone's next `mix` on, a locked channel's level, pan, mute and solo keep their
    ///   stored values while the rest of that mix applies, and its `mix_ack` carries the kept
    ///   values. [Self::set_mix] still changes locked channels.
    ///
    /// The change takes a new revision, is announced as [EngineEvent::MixChanged] from
    /// [MixOrigin::Engine] and is pushed to the phone as `mix_state`, which carries the limit and the
    /// locks. The engine keeps the controls for the musician (under their phone's client id when it
    /// announced one, else their address, the key it returns), so a musician whose mix is created
    /// again, back after the grace period or from a new IP, gets them back when they register.
    ///
    /// An address that does not parse, a musician that is not here, or a lock list longer than the
    /// mix is an error naming the problem, and nothing changes.
    pub fn set_engineer_controls(
        &self,
        value: &str,
        update: &EngineerUpdate,
    ) -> Result<(MusicianKey, MixValues), Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        set_target_controls(&self.targets, &self.events, value, update)
            .map_err(|error| -> Box<dyn Error> { error.into() })
    }

    /// Keeps the engineer's `controls` for the musician `key` names, whether or not they are here,
    /// as a desk does with the controls it saved: a musician with that key who is here gets them at
    /// once (announced as [EngineEvent::MixChanged] from [MixOrigin::Engine] when their mix changed),
    /// and one who registers later gets them when they do. Controls kept for a phone's client id
    /// win over controls kept for its IP.
    pub fn keep_engineer_controls(
        &self,
        key: &MusicianKey,
        controls: EngineerControls,
    ) -> Result<(), Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        keep_target_controls(&self.targets, &self.events, key, controls);
        Ok(())
    }

    /// Replaces every engineer's control and every mirror the engine keeps with `controls` and
    /// `mirrors` (keys as [Self::keep_engineer_controls] and [Self::keep_mirror] take them), as a
    /// desk does when it loads a saved session: what was kept before and is not in them is
    /// forgotten, for musicians who are here and for those who are not, so a lock, a limit or a
    /// mirror the loaded session dropped never comes back when a musician registers.
    ///
    /// The controls are replaced first: every musician here takes those now kept for them (client
    /// id first, then IP) or the defaults. Then the mirrors: each is admitted in order as
    /// [Self::keep_mirror] admits one (a mirror on oneself, or one closing a circle with those
    /// admitted before it, is left out and its refusal returned), and every musician here is put on
    /// their mirror or freed from the one they lost. Each mix that changed is announced as
    /// [EngineEvent::MixChanged] from [MixOrigin::Engine], so its phone is told.
    pub fn replace_kept(
        &self,
        controls: &[(MusicianKey, EngineerControls)],
        mirrors: &[(MusicianKey, MusicianKey, String)],
    ) -> Result<Vec<String>, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
        Ok(replace_target_kept(
            &self.targets,
            &self.events,
            controls,
            mirrors,
        ))
    }

    /// Stops capture and joins the engine threads, the same clean shutdown Ctrl+C triggers.
    pub fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        self.stopped.store(true, Ordering::SeqCst);
        // Stop producing before the engine thread: drop the capture stream first, then stop the
        // thread that reads its ring and feeds the phones.
        self.stream = None;
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.stop()?;
        }
        // Nothing feeds the take any more: its files are finished and it ends as the engine's.
        self.recorder.engine_stopped();
        // Nothing plays a take back any more: its disk thread ends.
        self.playback_clock.set_active(false);
        self.soundcheck.engine_stopped();
        // Nothing feeds the local output any more: close it, and free what the sink handed back.
        self.local_output.active = None;
        self.local_output.reap();
        // Capture and the engine thread have ended, so nothing would ever lower the last levels.
        clear_meters(&self.levels, &self.local_meter, &self.targets.read());
        if let Some(thread) = self.control_thread.take() {
            thread
                .join()
                .map_err(|_| "control WebSocket thread panicked")?;
        }
        if let Some(thread) = self.discovery_thread.take() {
            thread
                .join()
                .map_err(|_| "discovery beacon thread panicked")?;
        }
        Ok(())
    }
}

/// Starts the engine and returns its handle, or `None` when no input device is available.
///
/// `None` is not an error: the CLI prints a friendly line and exits cleanly for a machine with no
/// audio interface, and a UI can treat it the same way. Every other startup failure is an error.
///
/// The narrative the CLI prints travels with the engine, so the command-line output is unchanged
/// even though the binary no longer owns the wiring.
pub fn start(config: EngineConfig) -> Result<Option<EngineHandle>, Box<dyn Error>> {
    let targets = resolve_targets(&config.targets)?;
    println!("UDP targets:");
    for target in &targets {
        println!("  {target}");
    }
    let host = input_host(config.host);
    let devices = host
        .input_devices()
        .map_err(|error| format!("could not enumerate input devices: {error}"))?
        .collect::<Vec<_>>();

    if devices.is_empty() {
        println!("No input device is available. Connect an audio interface and try again.");
        return Ok(None);
    }

    println!("Available input devices:");
    for (index, device) in devices.iter().enumerate() {
        let name = device.to_string();
        println!("  {}. {name}", index + 1);
    }

    let device = select_device(devices, config.device_filter.as_deref())?;
    let device_name = device.to_string();
    let supported_config = select_input_config(&device)?;
    let buffer_frames = config.buffer_frames.or(Some(DEFAULT_BUFFER_FRAMES));

    println!("Selected input: {device_name}");
    println!(
        "Capture format: channels={}, sample rate={} Hz, buffer={}, sample format={:?}",
        supported_config.channels(),
        supported_config.sample_rate(),
        describe_buffer(buffer_frames),
        supported_config.sample_format()
    );
    if supported_config.sample_rate() != TARGET_SAMPLE_RATE {
        println!(
            "Input is {} Hz; low-pass filtering and decimating to {TARGET_SAMPLE_RATE} Hz before packaging.",
            supported_config.sample_rate()
        );
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let source_channels = supported_config.channels().min(u16::from(u8::MAX)) as u8;
    let group_layout = validate_groups(&config.groups, usize::from(source_channels))?;
    let target_registry = Arc::new(TargetRegistry::new(&targets, &group_layout)?);
    let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
    let config_state = Arc::new(ConfigState::new(
        source_channels,
        group_statuses(&group_layout),
        group_layout,
    ));
    let events = EventBus::default();
    let local_mix = Arc::new(config_state.new_mix());

    let control_thread = spawn_control_thread(
        config.control_port,
        Arc::clone(&target_registry),
        Arc::clone(&stopped),
        Arc::clone(&config_state),
        Arc::clone(&peers),
        events.clone(),
    );
    let discovery_thread = spawn_discovery_thread(
        Arc::clone(&target_registry),
        config.control_port,
        TARGET_SAMPLE_RATE,
        Arc::clone(&stopped),
    );
    let packet_stats = Arc::new(PacketStats {
        sent: AtomicU64::new(0),
        discarded: AtomicU64::new(0),
    });
    let levels = Arc::new(LevelMeter::new());
    let capture_stats = Arc::new(CaptureStats::default());
    let network = NetworkSink::new(Arc::clone(&target_registry), Arc::clone(&packet_stats))?;
    let local_meter = OutputMeter::default();
    let (local, output_control) = local_output_sink(Arc::clone(&local_mix), local_meter.clone());
    let (recorder, recorder_lines) = recorder_sink(Arc::clone(&local_mix));
    let pipeline = PipelineHandle::spawn(Pipeline::new(
        vec![Box::new(network), Box::new(local), Box::new(recorder)],
        Arc::clone(&levels),
    ))?;
    let playback_clock = Arc::new(PlaybackClock::new());
    playback_clock.set_waker(pipeline.waker());
    let links = CaptureLinks {
        levels: Arc::clone(&levels),
        stats: Arc::clone(&capture_stats),
        wake: Some(pipeline.waker()),
    };
    let opened = build_input_stream(&device, &supported_config, buffer_frames, &links)?;
    let block_frames = opened.reader.format().block_frames;
    pipeline.set_source(opened.reader)?;
    opened
        .stream
        .play()
        .map_err(|error| format!("could not start input capture: {error}"))?;

    let capture_format = CaptureFormat {
        channels: supported_config.channels(),
        sample_rate: supported_config.sample_rate(),
        sample_format: format!("{:?}", supported_config.sample_format()),
        buffer_size: describe_buffer(opened.buffer_frames),
    };

    Ok(Some(EngineHandle {
        device_name,
        capture_format,
        control_port: config.control_port,
        buffer_frames,
        host: config.host,
        source_channels,
        group_definitions: config.groups,
        levels,
        targets: target_registry,
        stopped,
        capture_stats,
        packet_stats,
        peers,
        config_state,
        events,
        local_mix,
        local_meter,
        local_output: LocalOutput::new(output_control),
        capture_device: Some(device),
        block_frames,
        recorder: Recorder::new(recorder_lines),
        playback_clock,
        soundcheck: Soundcheck::default(),
        stream: Some(opened.stream),
        pipeline: Some(pipeline),
        control_thread: Some(control_thread),
        discovery_thread: Some(discovery_thread),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GroupDefinition;
    use crate::output::LocalOutputState;
    use crate::pipeline::DEFAULT_BLOCK_FRAMES;

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("test IP should parse")
    }

    /// 16-bit samples as the float frames capture produces from them.
    fn pcm(samples: &[i16]) -> Vec<f32> {
        use crate::pipeline::CaptureSample;
        samples.iter().map(|sample| sample.to_f32()).collect()
    }

    const PHONE_ID: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    #[test]
    fn a_registered_phone_announces_its_name_and_client_id_and_a_configured_target_nothing() {
        let layout = GroupLayout::default();
        let registry = TargetRegistry::new(&["192.168.1.3:50000".parse().unwrap()], &layout)
            .expect("registry should build");
        registry.register(
            ip("192.168.1.4"),
            50000,
            Some("Ana".to_owned()),
            Some(PHONE_ID),
            &layout,
        );
        registry.register(ip("192.168.1.5"), 50000, None, None, &layout);

        let announced = announced_identities(&registry.read());

        assert_eq!(
            announced
                .get(&ip("192.168.1.3"))
                .cloned()
                .unwrap_or_default(),
            Announced::default(),
            "a configured target is the engineer's and announced nothing"
        );
        assert_eq!(
            announced[&ip("192.168.1.4")],
            Announced {
                name: Some("Ana".to_owned()),
                client_id: Some(PHONE_ID.to_owned()),
            }
        );
        assert_eq!(
            announced
                .get(&ip("192.168.1.5"))
                .and_then(|phone| phone.client_id.clone()),
            None,
            "an older phone sends no id"
        );
    }

    #[test]
    fn a_phone_back_from_a_new_ip_reports_the_same_client_id_at_its_new_address() {
        let layout = GroupLayout::default();
        let registry = TargetRegistry::new(&[], &layout).expect("empty registry");
        registry.register(ip("192.168.1.10"), 50000, None, Some(PHONE_ID), &layout);
        registry.register(ip("192.168.1.22"), 50000, None, Some(PHONE_ID), &layout);
        let entries = registry.read();
        let targets: Vec<SocketAddr> = entries.iter().map(|entry| entry.address).collect();

        let musicians = join_musicians(
            &targets,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &announced_identities(&entries),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert_eq!(musicians.len(), 1, "the row moved instead of doubling");
        assert_eq!(musicians[0].address, "192.168.1.22:50000".parse().unwrap());
        assert_eq!(musicians[0].client_id.as_deref(), Some(PHONE_ID));
    }

    #[test]
    fn joins_targets_and_connected_peer_ips_into_one_row_per_musician() {
        let targets = vec![
            "192.168.1.3:50000".parse().unwrap(),
            "192.168.1.4:50000".parse().unwrap(),
            "192.168.1.5:50000".parse().unwrap(),
        ];
        let connected: HashSet<IpAddr> =
            [ip("192.168.1.3"), ip("192.168.1.5")].into_iter().collect();

        let mut counters = HashMap::new();
        counters.insert(
            ip("192.168.1.3"),
            MusicianCounters {
                packets_sent: 10,
                packets_discarded: 1,
            },
        );
        counters.insert(
            ip("192.168.1.4"),
            MusicianCounters {
                packets_sent: 20,
                packets_discarded: 2,
            },
        );

        let mut mixes = HashMap::new();
        mixes.insert(
            ip("192.168.1.3"),
            MixValues {
                volume_percent: 42,
                ..MixValues::default()
            },
        );

        let mut announced = HashMap::new();
        announced.insert(
            ip("192.168.1.4"),
            Announced {
                name: Some("Ana".to_owned()),
                client_id: Some(PHONE_ID.to_owned()),
            },
        );

        let reporting = LinkStatus {
            health: crate::health::LinkHealth::Warning,
            packets_per_second: Some(200.0),
            underruns_per_second: Some(1.0),
            loss_percent: Some(0.0),
            receive_timeouts_per_second: Some(0.0),
        };
        let mut links = HashMap::new();
        links.insert(ip("192.168.1.3"), reporting);

        let musicians = join_musicians(
            &targets,
            &connected,
            &counters,
            &mixes,
            &announced,
            &links,
            &HashMap::new(),
        );

        assert_eq!(musicians.len(), 3);

        let connected_with_mix = &musicians[0];
        assert_eq!(connected_with_mix.address, targets[0]);
        assert_eq!(connected_with_mix.name, None);
        assert_eq!(connected_with_mix.client_id, None);
        assert!(connected_with_mix.control_connected);
        assert_eq!(connected_with_mix.mix.volume_percent, 42);
        assert_eq!(
            connected_with_mix.counters,
            MusicianCounters {
                packets_sent: 10,
                packets_discarded: 1,
            }
        );
        assert_eq!(connected_with_mix.link, reporting);

        let disconnected = &musicians[1];
        assert_eq!(disconnected.address, targets[1]);
        assert_eq!(disconnected.name.as_deref(), Some("Ana"));
        assert_eq!(disconnected.client_id.as_deref(), Some(PHONE_ID));
        assert!(!disconnected.control_connected);
        assert_eq!(disconnected.mix, MixValues::default());
        assert_eq!(
            disconnected.counters,
            MusicianCounters {
                packets_sent: 20,
                packets_discarded: 2,
            }
        );

        let connected_without_data = &musicians[2];
        assert_eq!(connected_without_data.address, targets[2]);
        assert_eq!(connected_without_data.name, None);
        assert!(connected_without_data.control_connected);
        assert_eq!(connected_without_data.counters, MusicianCounters::default());
        assert_eq!(connected_without_data.mix, MixValues::default());
        // A phone that never reported (or predates reports) is unknown, with no rates to show.
        assert_eq!(connected_without_data.link, LinkStatus::default());
        assert_eq!(
            connected_without_data.link.health,
            crate::health::LinkHealth::Unknown
        );
    }

    #[test]
    fn a_stale_report_is_judged_by_whether_the_musicians_control_channel_is_still_connected() {
        use crate::health::{LinkHealth, LinkReport, REPORT_STALE_AFTER};
        use std::time::Duration;

        let registry = TargetRegistry::new(
            &[
                "192.168.1.3:50000".parse().unwrap(),
                "192.168.1.4:50000".parse().unwrap(),
                "192.168.1.5:50000".parse().unwrap(),
            ],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let received_at = Instant::now();
        for reporting in ["192.168.1.3", "192.168.1.4"] {
            let slot = registry.link_report(ip(reporting)).expect("configured");
            *slot.lock().expect("report lock") = Some(LinkReport {
                interval_ms: 1_000,
                packets_received: 200,
                packets_lost: 0,
                underruns: 0,
                receive_timeouts: 0,
                received_at,
            });
        }
        let connected: HashSet<IpAddr> =
            [ip("192.168.1.3"), ip("192.168.1.5")].into_iter().collect();
        let later = received_at + REPORT_STALE_AFTER + Duration::from_secs(1);

        let links = link_statuses(&registry.read(), &connected, later);

        assert_eq!(
            links[&ip("192.168.1.3")].health,
            LinkHealth::Bad,
            "still connected but its reports stopped: the link is lost"
        );
        assert_eq!(
            links[&ip("192.168.1.4")].health,
            LinkHealth::Unknown,
            "its control channel closed: the musician stopped or left"
        );
        assert_eq!(
            links[&ip("192.168.1.5")].health,
            LinkHealth::Unknown,
            "connected but never reported: a phone without stats reports"
        );
    }

    #[test]
    fn setting_a_mix_applies_a_partial_update_and_announces_it_as_an_engine_change() {
        let registry = TargetRegistry::new(
            &[
                "192.168.1.3:50000".parse().unwrap(),
                "192.168.1.4:50000".parse().unwrap(),
            ],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let state = registry.mix_state(ip("192.168.1.3")).expect("configured");
        state.update(MixValues {
            channel_gains: std::array::from_fn(|index| if index == 0 { 40 } else { 70 }),
            volume_percent: 60,
            muted: true,
            ..MixValues::default()
        });
        let before = state.snapshot();
        let other_before = registry
            .mix_state(ip("192.168.1.4"))
            .expect("configured")
            .snapshot();
        let events = EventBus::default();
        let observed = events.subscribe();

        let applied = set_target_mix(
            &registry,
            &events,
            "192.168.1.3:50000",
            &MixUpdate {
                channel_solo: Some(vec![false, true]),
                group_levels: Some(vec![-5]),
                max_level_percent: Some(75),
                ..MixUpdate::default()
            },
        )
        .expect("a configured target's mix is set");

        assert_eq!(applied, state.current());
        assert!(
            applied.revision > before.revision,
            "the change takes a new revision"
        );
        assert!(applied.channel_solo[1]);
        assert_eq!(applied.group_levels[0], 0, "clamped like a phone's value");
        assert_eq!(applied.max_level_percent, 75);
        assert_eq!(applied.channel_gains, before.channel_gains);
        assert_eq!(applied.pans, before.pans);
        assert_eq!(applied.channel_muted, before.channel_muted);
        assert_eq!(applied.volume_percent, 60);
        assert!(applied.muted);
        assert_eq!(
            registry
                .mix_state(ip("192.168.1.4"))
                .expect("configured")
                .snapshot(),
            other_before,
            "no other musician is touched"
        );
        assert_eq!(
            observed.try_recv(),
            Ok(EngineEvent::MixChanged {
                address: "192.168.1.3:50000".parse().unwrap(),
                origin: MixOrigin::Engine,
            })
        );
    }

    #[test]
    fn setting_the_mix_of_an_unknown_target_errors_and_announces_nothing() {
        let registry = TargetRegistry::new(
            &["192.168.1.3:50000".parse().unwrap()],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let events = EventBus::default();
        let observed = events.subscribe();
        let update = MixUpdate {
            volume_percent: Some(10),
            ..MixUpdate::default()
        };

        let unknown = set_target_mix(&registry, &events, "192.168.1.9:50000", &update)
            .expect_err("an unknown target is rejected");
        let invalid = set_target_mix(&registry, &events, "not an address", &update)
            .expect_err("an invalid address is rejected");

        assert!(
            unknown.contains("192.168.1.9:50000") && unknown.contains("not configured"),
            "the error names the target: {unknown}"
        );
        assert!(
            invalid.contains("not an address"),
            "the error names the value: {invalid}"
        );
        assert!(observed.try_recv().is_err(), "nothing changed");
    }

    #[test]
    fn the_engineers_controls_are_kept_for_the_musician_and_announced_as_an_engine_change() {
        let layout = GroupLayout::default();
        let registry = TargetRegistry::new(&["192.168.1.3:50000".parse().unwrap()], &layout)
            .expect("registry should build");
        registry.register(ip("192.168.1.4"), 50000, None, Some(PHONE_ID), &layout);
        let events = EventBus::default();
        let observed = events.subscribe();

        let (rack, rack_mix) = set_target_controls(
            &registry,
            &events,
            "192.168.1.3:50000",
            &EngineerUpdate {
                limit_percent: Some(70),
                ..EngineerUpdate::default()
            },
        )
        .expect("a configured target's controls are set");
        let (phone, phone_mix) = set_target_controls(
            &registry,
            &events,
            "192.168.1.4:50000",
            &EngineerUpdate {
                locked: Some(vec![false, true]),
                ..EngineerUpdate::default()
            },
        )
        .expect("a registered phone's controls are set");

        assert_eq!(
            rack,
            MusicianKey::Address("192.168.1.3:50000".parse().unwrap())
        );
        assert_eq!(phone, MusicianKey::Client(PHONE_ID.to_owned()));
        assert_eq!(rack_mix.engineer.limit_percent, 70);
        assert_eq!(
            rack_mix,
            registry
                .mix_state(ip("192.168.1.3"))
                .expect("configured")
                .current()
        );
        assert_eq!(phone_mix.engineer.locked, 0b10);
        assert_eq!(phone_mix.engineer.limit_percent, 100);
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            ["192.168.1.3:50000", "192.168.1.4:50000"].map(|address| EngineEvent::MixChanged {
                address: address.parse().unwrap(),
                origin: MixOrigin::Engine,
            })
        );
    }

    #[test]
    fn engineer_controls_for_an_unknown_musician_or_too_many_channels_change_nothing() {
        let registry = TargetRegistry::new(
            &["192.168.1.3:50000".parse().unwrap()],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let before = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .current();
        let events = EventBus::default();
        let observed = events.subscribe();

        let unknown = set_target_controls(
            &registry,
            &events,
            "192.168.1.9:50000",
            &EngineerUpdate::default(),
        )
        .expect_err("an unknown musician is rejected");
        let oversized = set_target_controls(
            &registry,
            &events,
            "192.168.1.3:50000",
            &EngineerUpdate {
                limit_percent: Some(10),
                locked: Some(vec![true; MAX_MIX_CHANNELS + 1]),
            },
        )
        .expect_err("a list longer than the mix is rejected");

        assert!(unknown.contains("not configured"), "{unknown}");
        assert!(oversized.contains("locked"), "{oversized}");
        assert_eq!(
            registry
                .mix_state(ip("192.168.1.3"))
                .expect("configured")
                .current(),
            before
        );
        assert!(observed.try_recv().is_err(), "nothing changed");
    }

    #[test]
    fn kept_controls_reach_a_musician_here_now_and_one_who_registers_later() {
        let layout = GroupLayout::default();
        let registry = TargetRegistry::new(&["192.168.1.3:50000".parse().unwrap()], &layout)
            .expect("registry should build");
        let events = EventBus::default();
        let observed = events.subscribe();
        let limited = EngineerControls {
            limit_percent: 50,
            locked: 0,
        };
        let locked = EngineerControls {
            limit_percent: 100,
            locked: 0b1,
        };

        keep_target_controls(
            &registry,
            &events,
            &MusicianKey::Address("192.168.1.3:50000".parse().unwrap()),
            limited,
        );
        keep_target_controls(
            &registry,
            &events,
            &MusicianKey::Client(PHONE_ID.to_owned()),
            locked,
        );
        keep_target_controls(
            &registry,
            &events,
            &MusicianKey::Address("192.168.1.3:50000".parse().unwrap()),
            limited,
        );

        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [EngineEvent::MixChanged {
                address: "192.168.1.3:50000".parse().unwrap(),
                origin: MixOrigin::Engine,
            }],
            "only the musician here changed, and only once"
        );
        let mix_of = |musician: &str| {
            registry
                .mix_state(ip(musician))
                .expect("a musician")
                .current()
        };
        assert_eq!(mix_of("192.168.1.3").engineer, limited);
        registry.register(ip("192.168.1.4"), 50000, None, Some(PHONE_ID), &layout);
        assert_eq!(
            mix_of("192.168.1.4").engineer,
            locked,
            "the phone gets its controls when it registers"
        );
    }

    #[test]
    fn replacing_the_kept_controls_and_mirrors_announces_each_change_and_reaches_later_phones() {
        let layout = GroupLayout::default();
        let registry = TargetRegistry::new(
            &[
                "192.168.1.3:50000".parse().unwrap(),
                "192.168.1.4:50000".parse().unwrap(),
            ],
            &layout,
        )
        .expect("registry should build");
        let events = EventBus::default();
        let rack = MusicianKey::Address("192.168.1.3:50000".parse().unwrap());
        let keys = MusicianKey::Address("192.168.1.4:50000".parse().unwrap());
        let locked = EngineerControls {
            limit_percent: 100,
            locked: 0b1,
        };
        keep_target_controls(
            &registry,
            &events,
            &rack,
            EngineerControls {
                limit_percent: 50,
                locked: 0,
            },
        );
        keep_target_mirror(&registry, &events, &rack, &keys, "Keys").expect("the rack follows");
        let observed = events.subscribe();

        let refused = replace_target_kept(
            &registry,
            &events,
            &[(MusicianKey::Client(PHONE_ID.to_owned()), locked)],
            &[(keys.clone(), keys.clone(), "Keys".to_owned())],
        );

        assert_eq!(
            refused.len(),
            1,
            "a mirror on oneself is refused: {refused:?}"
        );
        let rack_changed = EngineEvent::MixChanged {
            address: "192.168.1.3:50000".parse().unwrap(),
            origin: MixOrigin::Engine,
        };
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [rack_changed.clone(), rack_changed],
            "the rack's limit is lifted, then its mirror"
        );
        let rack_mix = registry.mix_state(ip("192.168.1.3")).expect("the rack");
        assert_eq!(rack_mix.current().engineer, EngineerControls::default());
        assert_eq!(rack_mix.following(), None);
        registry.register(ip("192.168.1.5"), 50000, None, Some(PHONE_ID), &layout);
        assert_eq!(
            registry
                .mix_state(ip("192.168.1.5"))
                .expect("the phone")
                .current()
                .engineer,
            locked,
            "a phone that registers later gets the loaded controls"
        );
    }

    #[test]
    fn an_invalid_mix_update_is_rejected_and_announces_nothing() {
        let registry = TargetRegistry::new(
            &["192.168.1.3:50000".parse().unwrap()],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let before = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .snapshot();
        let events = EventBus::default();
        let observed = events.subscribe();

        let error = set_target_mix(
            &registry,
            &events,
            "192.168.1.3:50000",
            &MixUpdate {
                pans: Some(vec![50; MAX_MIX_CHANNELS + 1]),
                ..MixUpdate::default()
            },
        )
        .expect_err("an oversized list is rejected");

        assert!(error.contains("pans"), "the error names the list: {error}");
        assert_eq!(
            registry
                .mix_state(ip("192.168.1.3"))
                .expect("configured")
                .snapshot(),
            before
        );
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn peak_to_level_maps_silence_and_full_scale() {
        assert_eq!(peak_to_level(0), 0);
        assert_eq!(peak_to_level(16_384), 50);
        assert_eq!(peak_to_level(32_767), 100);
        assert_eq!(peak_to_level(i16::MIN.unsigned_abs()), 100);
    }

    #[test]
    fn observe_records_a_peak_per_source_channel() {
        let meter = LevelMeter::new();

        // Two frames, two channels: channel 0 peaks at half scale, channel 1 at full scale.
        meter.observe(&pcm(&[16_384, 32_767, -16_384, -1]), 2);

        assert_eq!(meter.read(2), vec![50, 100]);
    }

    /// Readers only look: the meter stream, the status events and any poller can read as often as
    /// they like without changing what the others see.
    #[test]
    fn reading_a_meter_never_lowers_it() {
        let inputs = LevelMeter::new();
        inputs.observe(&pcm(&[16_384, 32_767]), 2);
        let output = OutputMeter::default();
        output.observe(&pcm(&[32_767, 16_384]));

        for _ in 0..50 {
            assert_eq!(inputs.read(2), vec![50, 100]);
            assert_eq!(output.read(), [100, 50]);
        }
    }

    /// Full scale falls to silence in 1.5 s of audio: at 48 kHz, one step of the 0-100 scale every
    /// 720 frames, however often the level is read.
    #[test]
    fn a_held_level_falls_one_step_every_720_frames_of_audio() {
        let inputs = LevelMeter::new();
        let output = OutputMeter::default();
        inputs.observe(&pcm(&[32_767]), 1);
        output.observe(&pcm(&[32_767, 32_767]));

        inputs.observe(&vec![0.0; 7_200], 1);
        output.observe(&vec![0.0; 2 * 7_200]);
        assert_eq!(inputs.read(1), vec![90], "0.15 s after a full-scale peak");
        assert_eq!(output.read(), [90, 90]);

        inputs.observe(&vec![0.0; 64_080], 1);
        output.observe(&vec![0.0; 2 * 64_080]);
        assert_eq!(inputs.read(1), vec![1], "one step short of 1.5 s");
        assert_eq!(output.read(), [1, 1]);

        inputs.observe(&vec![0.0; 720], 1);
        output.observe(&vec![0.0; 2 * 720]);
        assert_eq!(inputs.read(1), vec![0], "1.5 s after the peak");
        assert_eq!(output.read(), [0, 0]);
    }

    /// A louder block shows at once; a quieter one does not pull the level below its fall.
    #[test]
    fn a_new_peak_above_the_fallen_level_shows_at_once() {
        let inputs = LevelMeter::new();
        let output = OutputMeter::default();
        inputs.observe(&pcm(&[32_767, 32_767]), 2);
        output.observe(&pcm(&[32_767, 32_767]));
        inputs.observe(&vec![0.0; 2 * 36_000], 2);
        output.observe(&vec![0.0; 2 * 36_000]);
        assert_eq!(inputs.read(2), vec![50, 50], "half way down after 0.75 s");
        assert_eq!(output.read(), [50, 50]);

        // One frame: 80 on the first side, 30 on the second.
        inputs.observe(&pcm(&[26_214, 9_830]), 2);
        output.observe(&pcm(&[26_214, 9_830]));

        assert_eq!(inputs.read(2), vec![80, 50]);
        assert_eq!(output.read(), [80, 50]);
    }

    #[test]
    fn a_stereo_peak_reads_each_side_of_an_interleaved_buffer() {
        // Left peaks at half scale (on its negative swing), right at full scale.
        assert_eq!(stereo_peak(&pcm(&[8_000, 100, -16_384, 32_767])), [50, 100]);
        assert_eq!(stereo_peak(&[]), [0, 0]);
        assert_eq!(stereo_peak(&pcm(&[0, i16::MIN])), [0, 100]);
    }

    #[test]
    fn an_output_meter_holds_each_sides_peak() {
        let meter = OutputMeter::default();

        meter.observe(&pcm(&[16_384, 32_767]));
        // A quieter block does not pull the held peak below its fall.
        meter.observe(&pcm(&[100, 100]));
        assert_eq!(meter.read(), [50, 100]);
        // A buffer that is not whole stereo frames is ignored.
        meter.observe(&pcm(&[32_767, 32_767, 32_767]));
        assert_eq!(meter.read(), [50, 100]);
    }

    /// A stopped stream delivers no more audio, so no audio time passes for the meters to fall by:
    /// the engine clears them when it stops capturing.
    #[test]
    fn clearing_the_meters_returns_every_level_to_zero() {
        let address: SocketAddr = "192.168.1.3:50000".parse().unwrap();
        let registry = TargetRegistry::new(&[address], &GroupLayout::default())
            .expect("registry should build");
        registry.read()[0]
            .output_meter
            .observe(&pcm(&[32_767, 16_384]));
        let inputs = LevelMeter::new();
        inputs.observe(&pcm(&[32_767, 16_384]), 2);
        let local = OutputMeter::default();
        local.observe(&pcm(&[16_384, 16_384]));

        clear_meters(&inputs, &local, &registry.read());

        assert_eq!(
            read_meters(&inputs, 2, &local, &registry.read()),
            EngineMeters {
                channel_levels: vec![0, 0],
                musicians: vec![MusicianMeters {
                    address,
                    output_levels: [0, 0],
                }],
                local_levels: [0, 0],
            }
        );
    }

    #[test]
    fn the_meter_read_carries_input_peaks_and_each_musicians_output_peaks() {
        let first: SocketAddr = "192.168.1.3:50000".parse().unwrap();
        let second: SocketAddr = "192.168.1.4:50000".parse().unwrap();
        let registry = TargetRegistry::new(&[first, second], &GroupLayout::default())
            .expect("registry should build");
        registry.read()[0]
            .output_meter
            .observe(&pcm(&[32_767, -16_384]));
        let inputs = LevelMeter::new();
        inputs.observe(&pcm(&[16_384, 0, -32_767, 0]), 2);
        let local = OutputMeter::default();
        local.observe(&pcm(&[8_192, 32_767]));

        let meters = read_meters(&inputs, 2, &local, &registry.read());

        assert_eq!(
            meters,
            EngineMeters {
                channel_levels: vec![100, 0],
                musicians: vec![
                    MusicianMeters {
                        address: first,
                        output_levels: [100, 50],
                    },
                    MusicianMeters {
                        address: second,
                        output_levels: [0, 0],
                    },
                ],
                local_levels: [25, 100],
            }
        );
        assert_eq!(
            read_meters(&inputs, 2, &local, &registry.read()),
            meters,
            "a read changes nothing"
        );
    }

    #[test]
    fn a_musicians_mix_is_found_by_ip_at_the_address_the_status_shows() {
        let registry = TargetRegistry::new(
            &[
                "192.168.1.3:50000".parse().unwrap(),
                "192.168.1.4:50000".parse().unwrap(),
            ],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let stored = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .apply(&MixUpdate {
                volume_percent: Some(42),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        // A phone's change names its control peer: same IP, another port.
        let found = musician_mix(&registry, "192.168.1.3:41000".parse().unwrap());

        assert_eq!(found, Some(("192.168.1.3:50000".parse().unwrap(), stored)));
        assert_eq!(
            musician_mix(&registry, "192.168.1.9:41000".parse().unwrap()),
            None,
            "an IP with no musician has no mix"
        );
    }

    #[test]
    fn observe_ignores_incomplete_frames_and_zero_channels() {
        let meter = LevelMeter::new();

        meter.observe(&pcm(&[1, 2, 3]), 2);
        meter.observe(&[], 0);

        assert_eq!(meter.read(2), vec![0, 0]);
    }

    #[test]
    fn group_statuses_carry_names_and_zero_based_channels() {
        use crate::mix::Group;

        let layout = GroupLayout::new(vec![Group {
            id: "kit".to_owned(),
            name: "Drums".to_owned(),
            channels: vec![0, 1],
        }]);

        assert_eq!(
            group_statuses(&layout),
            vec![GroupStatus {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![0, 1],
                invalid: false,
            }]
        );
    }

    fn definition(id: &str, name: &str, channels: &[usize]) -> GroupDefinition {
        GroupDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    #[test]
    fn revalidating_groups_reports_out_of_range_groups_without_changing_them() {
        let definitions = vec![
            definition("kit", "Drums", &[1, 2]),
            definition("vox", "Vocals", &[5]),
        ];

        let revalidation = revalidate_groups(&definitions, 2);

        assert_eq!(revalidation.statuses.len(), 2);
        assert_eq!(revalidation.statuses[0].id, "kit");
        assert_eq!(revalidation.mixer_layout.groups()[1].id, "vox");
        assert_eq!(revalidation.statuses[0].name, "Drums");
        assert_eq!(revalidation.statuses[0].channels, vec![0, 1]);
        assert!(!revalidation.statuses[0].invalid);
        assert_eq!(revalidation.statuses[1].name, "Vocals");
        assert!(revalidation.statuses[1].invalid);
        // The engineer's definitions are read, never rewritten.
        assert_eq!(definitions[1].channels, vec![5]);
    }

    #[test]
    fn an_invalid_group_is_excluded_from_the_mixer_but_kept_for_reporting() {
        let definitions = vec![
            definition("1", "Drums", &[1, 2]),
            definition("2", "Vocals", &[3, 4]),
        ];

        // A two-channel device: "Vocals" names channels 3 and 4, so it is invalid.
        let revalidation = revalidate_groups(&definitions, 2);
        let mapping = revalidation.mixer_layout.channel_group();

        assert_eq!(mapping[0], Some(0));
        assert_eq!(mapping[1], Some(0));
        assert_eq!(mapping[2], None);
        assert_eq!(mapping[3], None);
        assert!(revalidation.statuses[1].invalid);
        assert_eq!(revalidation.statuses[1].channels, vec![2, 3]);
    }

    #[test]
    fn a_smaller_device_invalidates_a_group_and_switching_back_restores_it() {
        let definitions = vec![definition("1", "Drums", &[1, 2, 3, 4])];

        let smaller = plan_device_switch(&definitions, 2);
        assert_eq!(smaller.source_channels, 2);
        assert!(smaller.groups.statuses[0].invalid);
        assert_eq!(smaller.groups.mixer_layout.channel_group()[0], None);

        let restored = plan_device_switch(&definitions, 4);
        assert_eq!(restored.source_channels, 4);
        assert!(!restored.groups.statuses[0].invalid);
        assert_eq!(restored.groups.mixer_layout.channel_group()[0], Some(0));
        assert_eq!(restored.groups.mixer_layout.channel_group()[3], Some(0));
    }

    /// The engine's start-up configuration for `definitions` on a device with `channels` inputs.
    fn config_for(definitions: &[GroupDefinition], channels: u8) -> ConfigState {
        let groups = revalidate_groups(definitions, usize::from(channels));
        ConfigState::new(channels, groups.statuses, groups.mixer_layout)
    }

    #[test]
    fn a_switch_to_the_same_channel_count_does_not_ask_for_a_config_resend() {
        let definitions = vec![definition("1", "Drums", &[1, 2])];
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        let config = config_for(&definitions, 2);
        let before = config.generation();

        let plan = plan_device_switch(&definitions, 2);
        let changed = apply_groups(
            &registry,
            &config,
            &EventBus::default(),
            plan.source_channels,
            plan.groups,
        );

        assert!(!changed);
        assert_eq!(config.generation(), before, "no connection re-sends config");
    }

    /// The latent bug of the start-up layout: after a switch, the live configuration (what every
    /// `config` and every new musician's mix is built from) is the switched one, and the musicians
    /// already there mix with it too.
    #[test]
    fn a_device_switch_publishes_the_live_groups_and_moves_every_mix_onto_them() {
        let definitions = vec![
            definition("1", "Drums", &[1, 2]),
            definition("2", "Vocals", &[3, 4]),
        ];
        let config = config_for(&definitions, 4);
        let registry = TargetRegistry::new(
            &["192.168.1.3:50000".parse().unwrap()],
            &config.snapshot().layout,
        )
        .expect("registry should build");
        let before = config.generation();

        let plan = plan_device_switch(&definitions, 2);
        assert!(apply_groups(
            &registry,
            &config,
            &EventBus::default(),
            plan.source_channels,
            plan.groups
        ));

        let live = config.snapshot();
        assert!(live.generation > before, "every connection re-sends config");
        assert_eq!(live.source_channels, 2);
        assert!(live.groups[1].invalid);
        assert_eq!(live.layout.groups()[1].channels, Vec::<usize>::new());
        let existing = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .snapshot();
        assert_eq!(existing.channel_group[0], Some(0));
        assert_eq!(
            existing.channel_group[2], None,
            "Vocals no longer maps CH 3"
        );
        registry
            .add("192.168.1.4:50000", &config)
            .expect("a fresh address is accepted");
        let added = registry
            .mix_state(ip("192.168.1.4"))
            .expect("added")
            .snapshot();
        assert_eq!(added.channel_group, existing.channel_group);
    }

    #[test]
    fn a_failed_capture_rebuild_yields_no_plan_and_leaves_the_previous_state() {
        let definitions = vec![definition("1", "Drums", &[1, 2, 3, 4])];
        let error: Box<dyn Error> = "input device \"Gone\" is no longer available".into();

        let result = plan_switch_from_attempt(&definitions, Err(error));

        assert!(result.is_err());
        // Nothing was planned or mutated: the previous configuration stands.
        assert_eq!(definitions[0].channels, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_successful_capture_rebuild_plans_the_new_channels_and_group_validity() {
        let definitions = vec![definition("1", "Drums", &[1, 2, 3, 4])];

        let plan = plan_switch_from_attempt(&definitions, Ok(2)).expect("plan should build");

        assert_eq!(plan.source_channels, 2);
        assert!(plan.groups.statuses[0].invalid);
    }

    /// Drums (CH 1), Bass (CH 2) and Keys (CH 3), by ids `drums`, `bass` and `keys`.
    fn trio() -> Vec<GroupDefinition> {
        vec![
            definition("drums", "Drums", &[1]),
            definition("bass", "Bass", &[2]),
            definition("keys", "Keys", &[3]),
        ]
    }

    /// Two musicians on a four-channel engine with [trio], the first with Drums at 10, Bass at 20
    /// and Keys at 30, the second with every group at 50.
    fn trio_session() -> (TargetRegistry, ConfigState) {
        let config = config_for(&trio(), 4);
        let registry = TargetRegistry::new(
            &[
                "192.168.1.3:50000".parse().unwrap(),
                "192.168.1.4:50000".parse().unwrap(),
            ],
            &config.snapshot().layout,
        )
        .expect("registry should build");
        for (musician, levels) in [
            ("192.168.1.3", vec![10, 20, 30]),
            ("192.168.1.4", vec![50; 3]),
        ] {
            registry
                .mix_state(ip(musician))
                .expect("configured")
                .apply(&MixUpdate {
                    group_levels: Some(levels),
                    ..MixUpdate::default()
                })
                .expect("a valid update applies");
        }
        (registry, config)
    }

    fn group_levels(registry: &TargetRegistry, musician: &str) -> Vec<u8> {
        registry
            .mix_state(ip(musician))
            .expect("configured")
            .current()
            .group_levels[..4]
            .to_vec()
    }

    /// The engineer moves Keys to the front, deletes Bass and adds Vocals on CH 4: every musician's
    /// levels follow their groups by id, the new configuration is live, and each moved mix is
    /// announced as an engine change so the desktop and the phone are sent it.
    #[test]
    fn editing_groups_moves_every_musicians_levels_with_their_groups() {
        let (registry, config) = trio_session();
        let events = EventBus::default();
        let observed = events.subscribe();
        let revision_before = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .current()
            .revision;
        let edited = vec![
            definition("keys", "Keys", &[3]),
            definition("drums", "Drums", &[1]),
            definition("vocals", "Vocals", &[4]),
        ];

        let groups =
            edit_groups(&registry, &config, &events, 4, &edited).expect("the edit is valid");

        assert_eq!(
            groups
                .iter()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            ["keys", "drums", "vocals"]
        );
        assert_eq!(group_levels(&registry, "192.168.1.3"), [30, 10, 100, 100]);
        assert_eq!(group_levels(&registry, "192.168.1.4"), [50, 50, 100, 100]);
        let first = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .current();
        assert!(
            first.revision > revision_before,
            "the move takes a revision"
        );
        assert_eq!(
            first.channel_group[2],
            Some(0),
            "CH 3 is in Keys, now first"
        );
        assert_eq!(first.channel_group[1], None, "CH 2 was in the deleted Bass");
        assert_eq!(first.channel_group[3], Some(2));
        assert_eq!(config.snapshot().groups, groups);
        let announced: Vec<EngineEvent> = observed.try_iter().collect();
        let mut expected = vec![EngineEvent::GroupsChanged];
        expected.extend(["192.168.1.3:50000", "192.168.1.4:50000"].map(|address| {
            EngineEvent::MixChanged {
                address: address.parse().unwrap(),
                origin: MixOrigin::Engine,
            }
        }));
        assert_eq!(
            announced, expected,
            "the new groups are announced before the levels placed for them"
        );
    }

    /// Renaming a group and giving it another channel keeps every level where it is: nothing is
    /// moved and no mix is announced, but the phones are sent the new names and membership.
    #[test]
    fn an_edit_that_keeps_every_position_moves_no_level() {
        let (registry, config) = trio_session();
        let events = EventBus::default();
        let observed = events.subscribe();
        let generation = config.generation();
        let revision = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .current()
            .revision;
        let renamed = vec![
            definition("drums", "Kit", &[1, 4]),
            definition("bass", "Bass", &[2]),
            definition("keys", "Keys", &[3]),
        ];

        edit_groups(&registry, &config, &events, 4, &renamed).expect("the edit is valid");

        let mix = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .current();
        assert_eq!(mix.revision, revision, "the stored mix did not change");
        assert_eq!(&mix.group_levels[..3], &[10, 20, 30]);
        assert_eq!(mix.channel_group[3], Some(0), "CH 4 joined the Kit");
        assert!(
            config.generation() > generation,
            "the phones get the new names"
        );
        assert_eq!(config.snapshot().groups[0].name, "Kit");
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [EngineEvent::GroupsChanged],
            "the groups changed, no mix did"
        );
    }

    /// A group naming a channel the device lacks is kept, invalid and mapping nothing, exactly as
    /// after a device switch; definitions that cannot work anywhere are refused and change nothing.
    #[test]
    fn an_edit_is_checked_as_at_start_up_and_a_missing_channel_only_invalidates_its_group() {
        let (registry, config) = trio_session();
        let events = EventBus::default();

        let groups = edit_groups(
            &registry,
            &config,
            &events,
            4,
            &[
                definition("drums", "Drums", &[1]),
                definition("x", "Rack", &[7]),
            ],
        )
        .expect("a missing channel is not an error while running");
        assert!(groups[1].invalid);
        assert_eq!(groups[1].channels, vec![6]);
        assert_eq!(
            config.snapshot().layout.groups()[1].channels,
            Vec::<usize>::new()
        );

        let before = config.snapshot();
        let levels = group_levels(&registry, "192.168.1.3");
        for refused in [
            vec![
                definition("a", "Drums", &[1]),
                definition("a", "Bass", &[2]),
            ],
            vec![
                definition("a", "Drums", &[1]),
                definition("b", "Bass", &[1]),
            ],
            vec![definition("a", " ", &[1])],
            (0..17)
                .map(|index| definition(&index.to_string(), "Group", &[]))
                .collect(),
        ] {
            assert!(edit_groups(&registry, &config, &events, 4, &refused).is_err());
            assert_eq!(config.snapshot(), before, "nothing changed");
            assert_eq!(group_levels(&registry, "192.168.1.3"), levels);
        }
    }

    fn channel(name: &str, kind: ChannelKind) -> ChannelMeta {
        ChannelMeta {
            name: name.to_owned(),
            kind,
            ..ChannelMeta::default()
        }
    }

    fn hidden(channel: ChannelMeta) -> ChannelMeta {
        ChannelMeta {
            hidden: true,
            ..channel
        }
    }

    fn linked(channel: ChannelMeta) -> ChannelMeta {
        ChannelMeta {
            linked_to_next: true,
            ..channel
        }
    }

    #[test]
    fn instrument_kinds_have_fixed_names_that_read_back() {
        let names: Vec<&str> = ChannelKind::ALL.iter().map(|kind| kind.name()).collect();

        assert_eq!(
            names,
            [
                "voice",
                "guitar",
                "bass",
                "keys",
                "drums",
                "percussion",
                "horns",
                "strings",
                "playback",
                "click",
                "other"
            ]
        );
        for kind in ChannelKind::ALL {
            assert_eq!(ChannelKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(ChannelKind::from_name("kazoo"), None);
        assert_eq!(ChannelKind::default(), ChannelKind::Other);
        assert_eq!(
            ChannelMeta::default(),
            channel("", ChannelKind::Other),
            "unnamed, other, shown and on its own"
        );
    }

    #[test]
    fn channel_details_are_checked_before_anything_changes() {
        use ChannelKind::{Drums, Keys, Other};
        let accepted = vec![
            channel("Kick", Drums),
            linked(channel("Keys L", Keys)),
            channel("Keys R", Keys),
            hidden(linked(channel("Spare L", Other))),
            hidden(channel("Spare R", Other)),
        ];
        assert_eq!(check_channels(&accepted), Ok(()));

        for (refused, named) in [
            (vec![ChannelMeta::default(); MAX_MIX_CHANNELS + 1], "33"),
            (
                vec![channel(&"x".repeat(MAX_CHANNEL_NAME_LENGTH + 1), Other)],
                "CH 1",
            ),
            (
                vec![channel("Kick", Drums), linked(channel("Pad", Keys))],
                "CH 2",
            ),
            (
                vec![
                    linked(channel("A", Keys)),
                    linked(channel("B", Keys)),
                    channel("C", Keys),
                ],
                "CH 2",
            ),
            (
                vec![linked(channel("L", Keys)), hidden(channel("R", Keys))],
                "CH 1",
            ),
        ] {
            let error = check_channels(&refused).expect_err("refused");
            assert!(error.contains(named), "names {named}: {error}");
        }
        assert_eq!(
            check_channels(&[channel(&"é".repeat(MAX_CHANNEL_NAME_LENGTH), Other)]),
            Ok(()),
            "a name is measured in characters, not bytes"
        );
    }

    #[test]
    fn channel_details_fit_the_device_keeping_what_exists_and_dropping_the_rest() {
        use ChannelKind::{Drums, Keys, Other};
        let details = vec![
            channel("Kick", Drums),
            hidden(channel("Spare", Other)),
            linked(channel("Keys L", Keys)),
            channel("Keys R", Keys),
        ];

        assert_eq!(fit_channels(&details, 4), details);
        assert_eq!(
            fit_channels(&details, 3),
            vec![
                channel("Kick", Drums),
                hidden(channel("Spare", Other)),
                channel("Keys L", Keys),
            ],
            "a pair whose second side is gone is no pair"
        );
        assert_eq!(
            fit_channels(&details[..1], 3),
            vec![
                channel("Kick", Drums),
                ChannelMeta::default(),
                ChannelMeta::default()
            ],
            "an input with no details gets the defaults"
        );
    }

    const DUO: [&str; 2] = ["192.168.1.3:50000", "192.168.1.4:50000"];

    /// Two musicians on a four-channel engine with no groups.
    fn duo() -> (TargetRegistry, ConfigState) {
        let config = config_for(&[], 4);
        let registry = TargetRegistry::new(
            &DUO.map(|address| address.parse().unwrap()),
            &config.snapshot().layout,
        )
        .expect("registry should build");
        (registry, config)
    }

    /// What `musician` is sent for one four-channel frame, as the send loop mixes it.
    fn heard(registry: &TargetRegistry, musician: &str, frame: [i16; 4]) -> Vec<i16> {
        let address: SocketAddr = musician.parse().unwrap();
        let mix = registry
            .mix_state(address.ip())
            .expect("configured")
            .snapshot();
        crate::mix::mix_channels(&frame, 4, mix)
    }

    #[test]
    fn a_hidden_channel_is_silent_in_every_mix_and_showing_it_restores_it() {
        let (registry, config) = duo();
        let events = EventBus::default();
        let observed = events.subscribe();
        // Only CH 2 carries signal; its default pan is hard right.
        let frame = [0, 10_000, 0, 0];
        for musician in DUO {
            assert_eq!(heard(&registry, musician, frame), [0, 10_000]);
        }

        let channels = edit_channels(
            &registry,
            &config,
            &events,
            &[
                ChannelMeta::default(),
                hidden(channel("Spare", ChannelKind::Other)),
            ],
        )
        .expect("the details are valid");

        assert_eq!(channels.len(), 4, "one entry per input");
        assert!(channels[1].hidden);
        assert_eq!(config.snapshot().channels, channels);
        for musician in DUO {
            assert_eq!(
                heard(&registry, musician, frame),
                [0, 0],
                "{musician} hears nothing of it"
            );
        }
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [EngineEvent::ChannelsChanged],
            "no stored mix changed"
        );

        edit_channels(&registry, &config, &events, &[]).expect("the details are valid");
        for musician in DUO {
            assert_eq!(heard(&registry, musician, frame), [0, 10_000]);
        }
    }

    #[test]
    fn linking_a_pair_pans_it_apart_for_every_musician_and_keeps_its_sides_equal() {
        let (registry, config) = duo();
        registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .apply(&MixUpdate {
                channel_gains: Some(vec![100, 60, 30]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let events = EventBus::default();
        let observed = events.subscribe();
        let generation = config.generation();

        edit_channels(
            &registry,
            &config,
            &events,
            &[
                channel("Kick", ChannelKind::Drums),
                linked(channel("Keys L", ChannelKind::Keys)),
                channel("Keys R", ChannelKind::Keys),
            ],
        )
        .expect("the details are valid");

        assert!(config.generation() > generation, "every phone is sent them");
        assert!(config.snapshot().channels[1].linked_to_next);
        for (musician, levels) in [("192.168.1.3", [60, 60]), ("192.168.1.4", [100, 100])] {
            let mix = registry
                .mix_state(ip(musician))
                .expect("configured")
                .current();
            assert_eq!(&mix.pans[1..3], &[0, 100], "{musician}");
            assert_eq!(&mix.channel_gains[1..3], &levels, "{musician}");
        }
        let mut expected = vec![EngineEvent::ChannelsChanged];
        expected.extend(DUO.map(|address| EngineEvent::MixChanged {
            address: address.parse().unwrap(),
            origin: MixOrigin::Engine,
        }));
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            expected,
            "the new details are announced before the pans placed for them"
        );

        // A phone moves one side: the other follows.
        let phone = registry
            .mix_state(ip("192.168.1.4"))
            .expect("configured")
            .apply(&MixUpdate {
                channel_gains: Some(vec![100, 100, 45]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        assert_eq!(&phone.channel_gains[1..3], &[45, 45]);

        // A musician who joins later starts with the pair panned apart.
        registry
            .add("192.168.1.5:50000", &config)
            .expect("a fresh address is accepted");
        let joined = registry
            .mix_state(ip("192.168.1.5"))
            .expect("added")
            .current();
        assert_eq!(&joined.pans[1..3], &[0, 100]);
    }

    /// What a device switch to `channels` inputs does to the live configuration.
    fn switch_to(registry: &TargetRegistry, config: &ConfigState, events: &EventBus, channels: u8) {
        let plan = plan_device_switch(&[], channels);
        apply_groups(registry, config, events, plan.source_channels, plan.groups);
    }

    /// Kick, Keys L and Keys R linked, and a hidden spare on CH 4.
    fn keys_and_spare() -> Vec<ChannelMeta> {
        vec![
            channel("Kick", ChannelKind::Drums),
            linked(channel("Keys L", ChannelKind::Keys)),
            channel("Keys R", ChannelKind::Keys),
            hidden(channel("Spare", ChannelKind::Other)),
        ]
    }

    /// The engineer's details survive a smaller device the way groups do: the inputs it lacks are
    /// left out of what clients see, and when they come back so do their names, their pair and,
    /// above all, the hidden spare's silence.
    #[test]
    fn a_device_switch_restores_the_details_of_inputs_that_come_back() {
        let (registry, config) = duo();
        let events = EventBus::default();
        edit_channels(&registry, &config, &events, &keys_and_spare())
            .expect("the details are valid");
        let observed = events.subscribe();

        switch_to(&registry, &config, &events, 2);

        assert_eq!(
            config.snapshot().channels,
            vec![
                channel("Kick", ChannelKind::Drums),
                channel("Keys L", ChannelKind::Keys),
            ],
            "clients see the inputs the device has, and no pair whose second side is missing"
        );
        assert!(observed
            .try_iter()
            .any(|event| event == EngineEvent::ChannelsChanged));
        let state = registry.mix_state(ip("192.168.1.3")).expect("configured");
        state
            .apply(&MixUpdate {
                channel_gains: Some(vec![100, 45]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let before = state.current();

        switch_to(&registry, &config, &events, 4);

        assert_eq!(config.snapshot().channels, keys_and_spare(), "all restored");
        for musician in DUO {
            assert_eq!(
                heard(&registry, musician, [0, 0, 0, 10_000]),
                [0, 0],
                "the spare is silent again for {musician}"
            );
        }
        let after = state.current();
        assert_eq!(after.revision, before.revision, "the switch moved no level");
        assert_eq!(&after.pans[1..3], &[0, 100], "the pair keeps its balance");
        assert_eq!(
            &after.channel_gains[1..3],
            &[45, 45],
            "the pair comes back with one level"
        );

        switch_to(&registry, &config, &events, 6);
        let live = config.snapshot().channels.clone();
        assert_eq!(live[..4], keys_and_spare()[..]);
        assert_eq!(
            live[4..],
            [ChannelMeta::default(), ChannelMeta::default()],
            "only inputs that never had details show the defaults"
        );
    }

    /// An editor reads the details back whole on a smaller device, entries for the inputs it lacks
    /// included, so sending the list back with one entry changed loses nothing; clients still see
    /// only the device's part.
    #[test]
    fn the_whole_channel_table_reads_back_on_a_smaller_device() {
        let (registry, config) = duo();
        let events = EventBus::default();
        edit_channels(&registry, &config, &events, &keys_and_spare())
            .expect("the details are valid");
        switch_to(&registry, &config, &events, 2);

        let mut table = channel_table(&config);
        assert_eq!(
            table,
            keys_and_spare(),
            "the inputs the device lacks are in it"
        );
        assert_eq!(
            config.snapshot().channels.len(),
            2,
            "clients see the device's part"
        );

        table[0].name = "Kick in".to_owned();
        edit_channels(&registry, &config, &events, &table).expect("the details are valid");
        assert_eq!(channel_table(&config)[1..], keys_and_spare()[1..]);
        switch_to(&registry, &config, &events, 4);
        assert_eq!(
            config.snapshot().channels[3],
            hidden(channel("Spare", ChannelKind::Other))
        );
    }

    /// Details for inputs the device lacks are kept for later; clients are only sent `config`
    /// when what they see changed.
    #[test]
    fn details_past_the_device_are_kept_for_later_without_a_new_config() {
        let config = config_for(&[], 2);
        let registry = TargetRegistry::new(
            &["192.168.1.3:50000".parse().unwrap()],
            &config.snapshot().layout,
        )
        .expect("registry should build");
        let events = EventBus::default();
        let shown = vec![
            channel("Kick", ChannelKind::Drums),
            channel("Snare", ChannelKind::Drums),
        ];
        edit_channels(&registry, &config, &events, &shown).expect("the details are valid");
        let generation = config.generation();
        let observed = events.subscribe();

        let mut later = shown.clone();
        later.push(hidden(channel("Spare", ChannelKind::Other)));
        let live =
            edit_channels(&registry, &config, &events, &later).expect("the details are valid");

        assert_eq!(live, shown, "one entry per input of this device");
        assert_eq!(config.generation(), generation, "no phone is sent anything");
        assert!(
            observed.try_recv().is_err(),
            "nothing the window shows changed"
        );

        switch_to(&registry, &config, &events, 3);
        assert_eq!(config.snapshot().channels, later);
        let mix = registry
            .mix_state(ip("192.168.1.3"))
            .expect("configured")
            .snapshot();
        assert_eq!(
            crate::mix::mix_channels(&[0, 0, 10_000], 3, mix),
            [0, 0],
            "the spare is hidden from the moment its input exists"
        );
    }

    #[test]
    fn refused_channel_details_change_nothing() {
        let (registry, config) = duo();
        let events = EventBus::default();
        let observed = events.subscribe();
        let before = config.snapshot();

        let error = edit_channels(
            &registry,
            &config,
            &events,
            &[linked(channel("Pad", ChannelKind::Keys))],
        )
        .expect_err("a pair cannot run past the last channel");

        assert!(error.contains("CH 1"), "{error}");
        assert_eq!(config.snapshot(), before);
        assert!(observed.try_recv().is_err(), "nothing is announced");
    }

    fn engine_change(address: &str) -> EngineEvent {
        EngineEvent::MixChanged {
            address: address.parse().unwrap(),
            origin: MixOrigin::Engine,
        }
    }

    fn mix_of(registry: &TargetRegistry, musician: &str) -> MixValues {
        registry
            .mix_state(ip(musician))
            .expect("a musician")
            .current()
    }

    /// [trio_session] and a third musician at .5 with a master, cap and limit of their own, the
    /// first musician's channels moved from the desk.
    fn trio_of_three() -> (TargetRegistry, ConfigState) {
        let (registry, config) = trio_session();
        registry
            .add("192.168.1.5:50000", &config)
            .expect("a fresh address");
        set_target_mix(
            &registry,
            &EventBus::default(),
            "192.168.1.3:50000",
            &MixUpdate {
                channel_gains: Some(vec![40, 50, 60]),
                pans: Some(vec![10, 90]),
                channel_solo: Some(vec![false, true]),
                volume_percent: Some(95),
                ..MixUpdate::default()
            },
        )
        .expect("a valid update applies");
        set_target_mix(
            &registry,
            &EventBus::default(),
            "192.168.1.5:50000",
            &MixUpdate {
                volume_percent: Some(60),
                max_level_percent: Some(80),
                ..MixUpdate::default()
            },
        )
        .expect("a valid update applies");
        set_target_controls(
            &registry,
            &EventBus::default(),
            "192.168.1.5:50000",
            &EngineerUpdate {
                limit_percent: Some(50),
                locked: Some(vec![true]),
            },
        )
        .expect("valid controls apply");
        (registry, config)
    }

    #[test]
    fn copying_a_mix_carries_its_channels_and_groups_never_the_master_cap_or_controls() {
        let (registry, _) = trio_of_three();
        let events = EventBus::default();
        let observed = events.subscribe();
        let source = mix_of(&registry, "192.168.1.3");

        let copied = copy_target_mix(
            &registry,
            &events,
            "192.168.1.3:50000",
            &[
                "192.168.1.4:50000".to_owned(),
                "192.168.1.5:50000".to_owned(),
            ],
        )
        .expect("the copy is valid");

        assert_eq!(
            copied
                .iter()
                .map(|(address, _)| *address)
                .collect::<Vec<_>>(),
            ["192.168.1.4:50000", "192.168.1.5:50000"].map(|a| a.parse::<SocketAddr>().unwrap())
        );
        for (address, mix) in &copied {
            assert_eq!(*mix, mix_of(&registry, &address.ip().to_string()));
            assert_eq!(mix.channel_gains, source.channel_gains, "{address}");
            assert_eq!(mix.pans, source.pans);
            assert_eq!(mix.channel_solo, source.channel_solo);
            assert_eq!(mix.group_levels, source.group_levels);
        }
        let third = &copied[1].1;
        assert_eq!(
            third.volume_percent, 60,
            "the master volume stays the musician's"
        );
        assert_eq!(third.max_level_percent, 80, "so does the phone's cap");
        assert_eq!(
            third.engineer.limit_percent, 50,
            "the engineer's controls stay"
        );
        assert_eq!(third.engineer.locked, 0b1);
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [
                engine_change("192.168.1.4:50000"),
                engine_change("192.168.1.5:50000")
            ]
        );
        let onto_itself = copy_target_mix(
            &registry,
            &events,
            "192.168.1.3:50000",
            &["192.168.1.3:50000".to_owned()],
        )
        .expect_err("a mix is not copied onto itself");
        assert!(onto_itself.contains("itself"), "{onto_itself}");
    }

    #[test]
    fn a_preset_saved_from_one_musician_lands_on_another_by_group_id_after_an_edit() {
        let (registry, config) = trio_of_three();
        let events = EventBus::default();
        let preset = target_preset(&registry, &config, "192.168.1.3:50000").expect("a musician");
        assert_eq!(preset.channels.len(), 4, "one per input of the device");
        assert_eq!(
            preset.groups,
            [("drums", 10), ("bass", 20), ("keys", 30)].map(|(id, level)| (id.to_owned(), level))
        );
        edit_groups(
            &registry,
            &config,
            &events,
            4,
            &[
                definition("keys", "Keys", &[3]),
                definition("vocals", "Vocals", &[4]),
                definition("drums", "Drums", &[1]),
            ],
        )
        .expect("the edit is valid");
        let observed = events.subscribe();

        let applied = apply_target_preset(
            &registry,
            &config,
            &events,
            &preset,
            &["192.168.1.5:50000".to_owned()],
        )
        .expect("the preset is valid");

        let mix = &applied[0].1;
        assert_eq!(
            &mix.group_levels[..3],
            &[30, 100, 10],
            "Keys and Drums by id"
        );
        assert_eq!(&mix.channel_gains[..3], &[40, 50, 60]);
        assert_eq!(mix.channel_gains[0], 40, "the desk sets a locked channel");
        assert_eq!(mix.volume_percent, 60);
        assert_eq!(mix.engineer.limit_percent, 50);
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [engine_change("192.168.1.5:50000")]
        );
    }

    #[test]
    fn a_desk_change_of_a_leader_reaches_its_follower_announced_after_it() {
        let (registry, _) = trio_of_three();
        let events = EventBus::default();
        let observed = events.subscribe();

        let started = set_target_mirror(
            &registry,
            &events,
            "192.168.1.4:50000",
            Some(MirrorSource {
                address: "192.168.1.3:50000",
                name: "Ana",
            }),
        )
        .expect("the mirror is valid");
        assert_eq!(started.name.as_deref(), Some("Ana"));
        assert_eq!(
            started.mix.channel_gains,
            mix_of(&registry, "192.168.1.3").channel_gains
        );
        set_target_mix(
            &registry,
            &events,
            "192.168.1.3:50000",
            &MixUpdate {
                channel_gains: Some(vec![5]),
                ..MixUpdate::default()
            },
        )
        .expect("a valid update applies");

        assert_eq!(mix_of(&registry, "192.168.1.4").channel_gains[0], 5);
        assert_eq!(
            observed.try_iter().collect::<Vec<_>>(),
            [
                engine_change("192.168.1.4:50000"),
                engine_change("192.168.1.3:50000"),
                engine_change("192.168.1.4:50000"),
            ]
        );
    }

    #[test]
    fn a_preset_or_a_copy_onto_a_follower_is_refused_and_changes_nobody() {
        let (registry, config) = trio_of_three();
        let events = EventBus::default();
        set_target_mirror(
            &registry,
            &events,
            "192.168.1.4:50000",
            Some(MirrorSource {
                address: "192.168.1.3:50000",
                name: "Ana",
            }),
        )
        .expect("the mirror is valid");
        let before = [
            mix_of(&registry, "192.168.1.4"),
            mix_of(&registry, "192.168.1.5"),
        ];
        let observed = events.subscribe();
        let both = [
            "192.168.1.5:50000".to_owned(),
            "192.168.1.4:50000".to_owned(),
        ];

        let copy = copy_target_mix(&registry, &events, "192.168.1.3:50000", &both)
            .expect_err("a follower is not copied onto");
        let preset = apply_target_preset(&registry, &config, &events, &MixPreset::default(), &both)
            .expect_err("nor given a preset");

        for refused in [copy, preset] {
            assert!(refused.contains("follows Ana's mix"), "{refused}");
        }
        assert_eq!(
            [
                mix_of(&registry, "192.168.1.4"),
                mix_of(&registry, "192.168.1.5")
            ],
            before
        );
        assert!(observed.try_recv().is_err(), "nothing is announced");
    }

    /// A capture format for a test source with `channels` channels.
    fn test_format(channels: u16) -> CaptureFormat {
        CaptureFormat {
            channels,
            sample_rate: TARGET_SAMPLE_RATE,
            sample_format: "F32".to_owned(),
            buffer_size: describe_buffer(Some(64)),
        }
    }

    /// A capture source the test drives itself, writing into the engine like a device's callback.
    fn test_capture(
        engine: &EngineHandle,
        channels: u16,
    ) -> (crate::pipeline::CaptureWriter, OpenedCapture) {
        let (writer, reader) = crate::pipeline::CaptureWriter::new(
            TARGET_SAMPLE_RATE,
            crate::pipeline::SourceFormat {
                channels: usize::from(channels),
                block_frames: 64,
            },
            engine.capture_links().expect("the engine is running"),
        )
        .expect("a 48 kHz source builds");
        let opened = OpenedCapture {
            stream: None,
            device: None,
            reader,
            device_name: format!("Test input ({channels})"),
            format: test_format(channels),
        };
        (writer, opened)
    }

    /// A running engine with no audio device: the engine thread, the network sink and the local
    /// output's sink run as in a session, with one phone at `phone`, and the capture is a stereo
    /// source the test drives. One group spans both inputs, so a switch to a mono device would
    /// invalidate it.
    fn engine_without_device(phone: SocketAddr) -> (EngineHandle, crate::pipeline::CaptureWriter) {
        let targets = Arc::new(
            TargetRegistry::new(&[phone], &GroupLayout::default()).expect("registry should build"),
        );
        let levels = Arc::new(LevelMeter::new());
        let packet_stats = Arc::new(PacketStats {
            sent: AtomicU64::new(0),
            discarded: AtomicU64::new(0),
        });
        let network = NetworkSink::new(Arc::clone(&targets), Arc::clone(&packet_stats))
            .expect("the sink binds");
        let config_state = Arc::new(ConfigState::new(2, Vec::new(), GroupLayout::default()));
        let local_mix = Arc::new(config_state.new_mix());
        let local_meter = OutputMeter::default();
        let (local, output_control) =
            local_output_sink(Arc::clone(&local_mix), local_meter.clone());
        let (recorder, recorder_lines) = recorder_sink(Arc::clone(&local_mix));
        let pipeline = PipelineHandle::spawn(Pipeline::new(
            vec![Box::new(network), Box::new(local), Box::new(recorder)],
            Arc::clone(&levels),
        ))
        .expect("the engine thread starts");
        let playback_clock = Arc::new(PlaybackClock::new());
        playback_clock.set_waker(pipeline.waker());
        let mut engine = EngineHandle {
            device_name: String::new(),
            capture_format: test_format(0),
            control_port: 0,
            buffer_frames: Some(64),
            host: AudioHost::Auto,
            source_channels: 0,
            group_definitions: vec![definition("pair", "Pair", &[1, 2])],
            levels,
            targets,
            stopped: Arc::new(AtomicBool::new(false)),
            capture_stats: Arc::new(CaptureStats::default()),
            packet_stats,
            peers: Arc::new(Mutex::new(HashMap::new())),
            config_state: Arc::clone(&config_state),
            events: EventBus::default(),
            local_mix,
            local_meter,
            local_output: LocalOutput::new(output_control),
            capture_device: None,
            block_frames: DEFAULT_BLOCK_FRAMES,
            recorder: Recorder::new(recorder_lines),
            playback_clock,
            soundcheck: Soundcheck::default(),
            stream: None,
            pipeline: Some(pipeline),
            control_thread: None,
            discovery_thread: None,
        };
        let (capture, opened) = test_capture(&engine, 2);
        engine
            .install_capture(opened)
            .expect("the test source installs");
        (engine, capture)
    }

    /// Polls the engine's meters until `done` holds, for at most two seconds.
    fn meters_until(engine: &EngineHandle, done: impl Fn(&EngineMeters) -> bool) -> EngineMeters {
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let meters = engine.meters();
            if done(&meters) || Instant::now() > deadline {
                return meters;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Stopping ends capture and the engine thread, which would leave the last levels frozen, so
    /// the engine clears every input and output meter itself.
    #[test]
    fn stopping_the_engine_clears_every_meter() {
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let address = phone.local_addr().expect("phone address");
        let (mut engine, mut capture) = engine_without_device(address);

        capture.write(&[0.5f32; 2 * 64]);
        let lit = meters_until(&engine, |meters| {
            meters.musicians[0]
                .output_levels
                .iter()
                .all(|level| *level > 0)
        });
        assert!(lit.channel_levels.iter().all(|level| *level > 0), "{lit:?}");
        assert!(lit.musicians[0]
            .output_levels
            .iter()
            .all(|level| *level > 0));

        engine.stop().expect("the engine stops");

        assert_eq!(
            engine.meters(),
            EngineMeters {
                channel_levels: vec![0, 0],
                musicians: vec![MusicianMeters {
                    address,
                    output_levels: [0, 0],
                }],
                local_levels: [0, 0],
            }
        );
    }

    /// A switch clears the input meters at once (the old device's levels describe a stream that is
    /// gone), and the new device raises its own from its first block.
    #[test]
    fn switching_the_input_clears_the_input_meters_and_the_new_one_raises_its_own() {
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let (mut engine, mut old) = engine_without_device(phone.local_addr().expect("address"));
        // The capture meters each block as it writes it (a fall the engine thread counts for idle
        // time can only lower a level, so a lit meter is checked as lit, not at an exact value).
        old.write(&[0.5f32; 2 * 64]);
        assert!(engine
            .meters()
            .channel_levels
            .iter()
            .all(|level| *level > 0));

        let (mut new, opened) = test_capture(&engine, 2);
        drop(old);
        let format = engine.install_capture(opened).expect("the switch commits");

        assert_eq!(format.channels, 2);
        assert_eq!(engine.meters().channel_levels, vec![0, 0]);
        new.write(&[0.25f32; 2 * 64]);
        let raised = engine.meters().channel_levels;
        assert!(
            raised.iter().all(|level| *level > 0 && *level <= 25),
            "{raised:?}"
        );

        engine.stop().expect("the engine stops");
    }

    /// What a switch would change: the device, its format, the channel count and the groups.
    fn session(engine: &EngineHandle) -> (String, CaptureFormat, u8, Vec<GroupStatus>) {
        (
            engine.device_name().to_owned(),
            engine.capture_format().clone(),
            engine.source_channels,
            engine.config_state.snapshot().groups.clone(),
        )
    }

    /// A switch whose plan is refused (here a capture whose format reports one channel while its
    /// frames carry two) changes nothing: the device, format, channel count and groups stay, the
    /// running capture keeps feeding the phones, and the refused capture's audio goes nowhere. A
    /// stopped engine then refuses `switch_device` before it touches any audio device.
    #[test]
    fn a_refused_switch_leaves_the_running_capture_untouched() {
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        phone
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("read timeout should set");
        let (mut engine, mut running) = engine_without_device(phone.local_addr().expect("address"));
        let before = session(&engine);
        assert_eq!(before.2, 2);
        assert!(before.3.iter().all(|group| !group.invalid));
        let (mut refused, mut opened) = test_capture(&engine, 2);
        opened.format = test_format(1);

        let error = engine
            .install_capture(opened)
            .expect_err("a capture whose format and frames disagree is refused");

        assert!(
            error
                .to_string()
                .contains("reports 1 channels but its frames carry 2"),
            "{error}"
        );
        assert_eq!(session(&engine), before);
        refused.write(&[-0.25f32; 2 * 64]);
        running.write(&[0.5f32; 2 * 64]);
        let mut packet = [0u8; 1_024];
        phone
            .recv(&mut packet)
            .expect("the running capture still reaches the phone");
        assert_eq!(i16::from_le_bytes([packet[20], packet[21]]), 16_384);

        engine.stop().expect("the engine stops");
        let error = engine
            .switch_device(None)
            .expect_err("a stopped engine does not switch");
        assert!(error.to_string().contains("stopping"), "{error}");
        assert_eq!(session(&engine), before);
    }

    /// A sink that fails on its first block, which ends the engine thread.
    struct Failing;

    impl crate::pipeline::Sink for Failing {
        fn process(&mut self, _block: &crate::pipeline::Block<'_>) {
            panic!("a sink failed (expected by this test)");
        }
    }

    /// When the engine thread is no longer running (here a sink panicked and ended it), a switch
    /// fails without committing anything: the device, format, channel count and groups stay, so a
    /// mono device that would invalidate the stereo group leaves it valid.
    #[test]
    fn a_switch_fails_without_committing_when_the_engine_thread_has_ended() {
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let (mut engine, _running) = engine_without_device(phone.local_addr().expect("address"));
        let failing = PipelineHandle::spawn(Pipeline::new(
            vec![Box::new(Failing)],
            Arc::clone(&engine.levels),
        ))
        .expect("the engine thread starts");
        engine
            .pipeline
            .replace(failing)
            .expect("the engine was running")
            .stop()
            .expect("the first engine thread stops");
        let (mut doomed, opened) = test_capture(&engine, 2);
        let pipeline = engine.pipeline.as_ref().expect("the failing engine thread");
        pipeline
            .set_source(opened.reader)
            .expect("the failing thread takes a source");
        doomed.write(&[0.1f32; 2 * 64]);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !pipeline.has_ended() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            pipeline.has_ended(),
            "the failing sink ended the engine thread"
        );
        let before = session(&engine);

        let (_mono, mono) = test_capture(&engine, 1);
        let error = engine
            .install_capture(mono)
            .expect_err("no engine thread takes the new capture");

        assert!(error.to_string().contains("has stopped"), "{error}");
        assert_eq!(session(&engine), before);
        assert!(session(&engine).3.iter().all(|group| !group.invalid));
    }

    /// [engine_without_device] with its phone socket kept, so the musician stays at that address.
    fn local_engine() -> (
        EngineHandle,
        crate::pipeline::CaptureWriter,
        std::net::UdpSocket,
    ) {
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let (engine, capture) = engine_without_device(phone.local_addr().expect("address"));
        (engine, capture, phone)
    }

    /// Everything on the bus so far.
    fn drain(events: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
        events.try_iter().collect()
    }

    /// The local mix and the musician's agree on what is server configuration: the group each
    /// channel feeds and the hidden channels.
    fn assert_same_configuration(engine: &EngineHandle) {
        let local = engine.local_mix();
        let musician = engine.status().musicians[0].mix;
        assert_eq!(local.channel_group, musician.channel_group);
        assert_eq!(local.hidden_channels, musician.hidden_channels);
    }

    #[test]
    fn the_local_mix_starts_neutral_on_the_live_groups_and_is_no_musician() {
        let (mut engine, _capture, _phone) = local_engine();

        let local = engine.local_mix();

        assert_eq!(local.channel_gains, MixValues::default().channel_gains);
        assert_eq!(local.volume_percent, 100);
        assert_eq!(
            &local.channel_group[..2],
            &[Some(0), Some(0)],
            "the stereo group"
        );
        assert!(local.revision > 0);
        let status = engine.status();
        assert_eq!(status.local_mix, local);
        assert_eq!(status.musicians.len(), 1, "the local mix is no musician");
        assert_same_configuration(&engine);
        engine.stop().expect("the engine stops");
    }

    /// The desk's path, as for a musician: a partial update merged and stored under a new
    /// revision, announced as the local mix's own event, and no musician touched. An update that
    /// cannot be applied changes and announces nothing.
    #[test]
    fn setting_the_local_mix_changes_only_it_and_announces_it() {
        let (mut engine, _capture, _phone) = local_engine();
        let events = engine.subscribe();
        let musician = engine.status().musicians[0].mix;
        let before = engine.local_mix();

        let applied = engine
            .set_local_mix(&MixUpdate {
                volume_percent: Some(40),
                channel_gains: Some(vec![80]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        assert_eq!(applied.volume_percent, 40);
        assert_eq!(&applied.channel_gains[..2], &[80, 100]);
        assert!(applied.revision > before.revision);
        assert_eq!(engine.local_mix(), applied);
        assert_eq!(drain(&events), vec![EngineEvent::LocalMixChanged]);
        assert_eq!(engine.status().musicians[0].mix, musician);

        let refused = engine
            .set_local_mix(&MixUpdate {
                channel_gains: Some(vec![100; 33]),
                ..MixUpdate::default()
            })
            .expect_err("an oversized list is refused");
        assert!(refused.to_string().contains("at most 32"), "{refused}");
        assert_eq!(engine.local_mix(), applied);
        assert!(drain(&events).is_empty());
        engine.stop().expect("the engine stops");
    }

    /// Linking a pair reaches the local mix as it reaches every musician's: the pair is panned
    /// apart and its second side takes the first side's level under a new revision, announced
    /// after the channel details. A hidden input is silent in it.
    #[test]
    fn channel_details_move_the_local_mix_like_a_musicians() {
        let (mut engine, _capture, _phone) = local_engine();
        engine
            .set_local_mix(&MixUpdate {
                channel_gains: Some(vec![30, 70]),
                pans: Some(vec![40, 60]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let events = engine.subscribe();

        engine
            .set_channels(vec![
                linked(channel("Keys L", ChannelKind::Keys)),
                channel("Keys R", ChannelKind::Keys),
            ])
            .expect("valid details");

        let local = engine.local_mix();
        assert_eq!(&local.pans[..2], &[0, 100]);
        assert_eq!(&local.channel_gains[..2], &[30, 30]);
        let announced = drain(&events);
        assert_eq!(announced.first(), Some(&EngineEvent::ChannelsChanged));
        assert_eq!(announced.last(), Some(&EngineEvent::LocalMixChanged));
        assert_same_configuration(&engine);

        engine
            .set_channels(vec![
                hidden(channel("Spare", ChannelKind::Other)),
                channel("Voice", ChannelKind::Voice),
            ])
            .expect("valid details");
        assert_eq!(engine.local_mix().hidden_channels, 1);
        assert_same_configuration(&engine);
        let mut heard = [0.0; 2];
        crate::mix::MixPlan::new(&engine.local_mix(), 2).mix_into(&[0.5, 0.0], &mut heard);
        assert_eq!(
            heard,
            [0.0, 0.0],
            "the hidden input is silent in the local mix"
        );
        engine.stop().expect("the engine stops");
    }

    /// A group edit moves the local mix's group levels by group id, as every musician's, and a
    /// device switch points it at the groups the new device can feed.
    #[test]
    fn groups_move_the_local_mix_like_a_musicians() {
        let (mut engine, _capture, _phone) = local_engine();
        engine
            .set_local_mix(&MixUpdate {
                group_levels: Some(vec![40]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let events = engine.subscribe();

        engine
            .set_groups(vec![
                definition("keys", "Keys", &[1]),
                definition("pair", "Pair", &[2]),
            ])
            .expect("valid groups");

        assert_eq!(&engine.local_mix().group_levels[..2], &[100, 40]);
        let announced = drain(&events);
        assert_eq!(announced.first(), Some(&EngineEvent::GroupsChanged));
        assert_eq!(announced.last(), Some(&EngineEvent::LocalMixChanged));
        assert_same_configuration(&engine);

        let (_mono, opened) = test_capture(&engine, 1);
        engine.install_capture(opened).expect("the switch commits");
        assert_eq!(engine.local_mix().channel_group[1], None, "input 2 is gone");
        assert_eq!(engine.local_mix().group_levels[1], 40, "its level is kept");
        assert_same_configuration(&engine);
        engine.stop().expect("the engine stops");
    }

    /// An output the test plays itself: the device callback's end of the ring and the output to
    /// install, named `device`, with `channels` outputs and the pair at `first`.
    fn test_output(
        device: &str,
        channels: usize,
        first: u16,
    ) -> (crate::output::OutputPlayer, crate::output::OpenedOutput) {
        let (feed, player, shared) = crate::output::output_link(channels, usize::from(first), 64);
        let opened = crate::output::OpenedOutput {
            stream: None,
            request: LocalOutputRequest {
                device: device.to_owned(),
                first_channel: first,
            },
            feed,
            shared,
        };
        (player, opened)
    }

    /// Waits until `done` holds, for at most five seconds.
    fn until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::yield_now();
        }
        done()
    }

    /// One packet at the phone, within two seconds.
    fn phone_hears(phone: &std::net::UdpSocket) -> bool {
        phone
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("read timeout should set");
        let mut packet = [0u8; 1_024];
        phone.recv(&mut packet).is_ok()
    }

    #[test]
    fn the_local_output_is_off_by_default_and_the_local_mix_is_metered() {
        let (mut engine, mut capture, _phone) = local_engine();

        assert_eq!(engine.local_output(), LocalOutputStatus::default());
        assert_eq!(engine.status().local_output.state, LocalOutputState::Off);
        capture.write(&[0.5f32; 2 * 64]);
        assert!(until(|| engine
            .meters()
            .local_levels
            .iter()
            .all(|level| *level > 0)));

        engine.stop().expect("the engine stops");
        assert_eq!(engine.meters().local_levels, [0, 0]);
    }

    /// The local mix (here at half volume) plays on the chosen pair; another output takes over
    /// live and the first is fed no more; turning it off stops it. The phone hears every block
    /// throughout: neither capture nor the phones notice.
    #[test]
    fn the_local_output_plays_the_local_mix_and_changes_live_without_touching_the_phones() {
        let (mut engine, mut capture, phone) = local_engine();
        engine
            .set_local_mix(&MixUpdate {
                volume_percent: Some(50),
                ..MixUpdate::default()
            })
            .expect("a valid update");
        let (mut first, opened) = test_output("Speakers", 4, 2);
        engine
            .install_output(Some(opened))
            .expect("the output installs");
        let status = engine.local_output();
        assert_eq!(status.state, LocalOutputState::Running);
        assert_eq!(status.output.map(|output| output.first_channel), Some(2));

        for _ in 0..4 {
            capture.write(&[0.5f32; 2 * 64]);
        }
        assert!(until(|| first.queued() == 4 * 64));
        assert!(phone_hears(&phone));
        let mut buffer = [9.0f32; 4 * 64];
        first.fill(&mut buffer);
        for frame in buffer.chunks_exact(4) {
            // Outputs 1-2 silent; 3-4 the half-volume mix (within the pan law's float residue).
            assert_eq!(&frame[..2], &[0.0, 0.0]);
            assert!((frame[2] - 0.25).abs() < 1e-6 && (frame[3] - 0.25).abs() < 1e-6);
        }

        let (second, opened) = test_output("Headphones", 2, 0);
        engine
            .install_output(Some(opened))
            .expect("the output changes live");
        for _ in 0..2 {
            capture.write(&[0.5f32; 2 * 64]);
        }
        assert!(until(|| second.queued() == 2 * 64));
        assert_eq!(first.queued(), 3 * 64, "the previous output is fed no more");
        assert!(phone_hears(&phone));

        let off = engine
            .set_local_output(None)
            .expect("turning the output off always works");
        assert_eq!(off, LocalOutputStatus::default());
        capture.write(&[0.5f32; 2 * 64]);
        capture.write(&[0.5f32; 2 * 64]);
        // The eighth packet means the engine thread has finished with the seventh block.
        assert!(until(|| engine.status().packets_sent >= 8));
        assert!(phone_hears(&phone));
        assert_eq!(second.queued(), 2 * 64, "a removed output is fed no more");
        engine.stop().expect("the engine stops");
    }

    /// A device failure (what the stream's error callback reports) stops only the local output:
    /// it is reported with its reason and fed no more, and the phones keep hearing.
    #[test]
    fn a_failed_local_output_stops_only_itself_and_is_reported() {
        let (mut engine, mut capture, phone) = local_engine();
        let (player, opened) = test_output("Speakers", 2, 0);
        let shared = Arc::clone(&opened.shared);
        engine
            .install_output(Some(opened))
            .expect("the output installs");
        capture.write(&[0.5f32; 2 * 64]);
        assert!(until(|| player.queued() == 64));

        shared.fail("the output device failed: unplugged".to_owned());
        capture.write(&[0.5f32; 2 * 64]);
        capture.write(&[0.5f32; 2 * 64]);

        // The phone hears all three blocks; the third one's packet means the engine thread has
        // finished with the second block, the first after the failure.
        for _ in 0..3 {
            assert!(phone_hears(&phone));
        }
        assert_eq!(player.queued(), 64);
        assert_eq!(
            engine.status().local_output.state,
            LocalOutputState::Failed {
                reason: "the output device failed: unplugged".to_owned()
            }
        );
        engine.stop().expect("the engine stops");
    }

    /// Under ASIO the only output offered is the capture driver's own, read from the device the
    /// capture already holds (here none, as no driver is open): nothing is listed and another
    /// driver is refused before anything is opened.
    #[test]
    fn under_asio_no_other_driver_is_offered_or_opened() {
        let (mut engine, _capture, _phone) = local_engine();
        engine.host = AudioHost::Asio;

        assert!(engine.output_devices().expect("listing works").is_empty());
        let refused = engine
            .set_local_output(Some(LocalOutputRequest {
                device: "Realtek ASIO".to_owned(),
                first_channel: 0,
            }))
            .expect_err("another driver is refused");
        assert!(
            refused.to_string().contains("only the capture driver"),
            "{refused}"
        );
        assert_eq!(engine.local_output(), LocalOutputStatus::default());
        engine.stop().expect("the engine stops");
    }

    /// An ASIO output lives on the capture driver: a switch to the same driver keeps it, a switch
    /// to another driver stops it (reported with the reason), since ASIO holds one driver.
    #[test]
    fn under_asio_a_switch_to_another_driver_stops_the_local_output() {
        let (mut engine, _capture, _phone) = local_engine();
        engine.host = AudioHost::Asio;
        let driver = engine.device_name().to_owned();
        let (_player, opened) = test_output(&driver, 2, 0);
        engine
            .install_output(Some(opened))
            .expect("the output installs");

        let (_same, opened) = test_capture(&engine, 2);
        engine.install_capture(opened).expect("the switch commits");
        assert_eq!(engine.local_output().state, LocalOutputState::Running);

        let (_other, opened) = test_capture(&engine, 1);
        engine.install_capture(opened).expect("the switch commits");
        let LocalOutputState::Failed { reason } = engine.local_output().state else {
            panic!("the output on the previous driver should have stopped");
        };
        assert!(reason.contains("ASIO driver"), "{reason}");
        engine.stop().expect("the engine stops");
    }

    /// A take of input `armed` (0-based) in `parent`, 24-bit.
    fn take_of(parent: &std::path::Path, armed: Vec<usize>) -> RecordRequest {
        crate::recorder::tests::request(parent, armed, crate::wav::SampleFormat::Int24)
    }

    /// Every frame of the WAV file at `path`.
    fn wav_samples(path: &std::path::Path) -> Vec<f32> {
        let mut reader = crate::wav::WavReader::open(path).expect("the file reads");
        let mut samples = vec![0.0; reader.frames() as usize * usize::from(reader.spec().channels)];
        let frames = reader.read_frames(&mut samples).expect("the audio reads");
        samples.truncate(frames * usize::from(reader.spec().channels));
        samples
    }

    /// The armed input records to its own file, named after the input, while the phones keep
    /// hearing every block; stopping finishes the take and reports it.
    #[test]
    fn a_take_records_the_armed_input_while_the_phones_keep_hearing() {
        let parent = crate::recorder::tests::scratch_directory("engine-take");
        let (mut engine, mut capture, phone) = local_engine();
        engine
            .set_channels(vec![
                channel("Vocal", ChannelKind::Voice),
                channel("Guitar", ChannelKind::Guitar),
            ])
            .expect("valid details");

        let status = engine
            .start_recording(take_of(&parent, vec![1]))
            .expect("the take starts");
        assert_eq!(status.state, crate::recorder::RecordingState::Recording);
        assert_eq!(status.tracks, vec!["Guitar".to_owned()]);
        for _ in 0..3 {
            capture.write(&[0.5f32; 2 * 64]);
        }
        assert!(until(|| engine.recording().frames == 192));
        assert!(phone_hears(&phone));
        assert_eq!(engine.status().recording.frames, 192);

        let summary = engine.stop_recording().expect("the take stops");

        assert_eq!(summary.frames, 192);
        assert_eq!(summary.ended, TakeEnd::Stopped);
        assert_eq!(summary.device, "Test input (2)");
        assert_eq!(summary.tracks.len(), 1);
        assert_eq!(summary.tracks[0].file, "02 Guitar.wav");
        assert_eq!(summary.tracks[0].channels, vec![2]);
        assert_eq!(summary.tracks[0].instrument, Some(ChannelKind::Guitar));
        assert_eq!(
            wav_samples(&summary.folder.join("02 Guitar.wav")),
            vec![0.5; 192]
        );
        let after = engine.recording();
        assert_eq!(after.state, crate::recorder::RecordingState::Idle);
        assert_eq!(after.last_take, Some(summary));
        assert!(engine.stop_recording().is_err(), "nothing records now");
        engine.stop().expect("the engine stops");
    }

    /// While a take records the input device cannot change (a take never mixes two devices),
    /// whether through `switch_device` or a capture already opened; after the take it can.
    #[test]
    fn a_device_switch_is_refused_while_recording() {
        let parent = crate::recorder::tests::scratch_directory("engine-switch");
        let (mut engine, _capture, _phone) = local_engine();
        engine
            .start_recording(take_of(&parent, vec![0]))
            .expect("the take starts");
        let before = session(&engine);

        let refused = engine
            .switch_device(None)
            .expect_err("no switch while recording");
        assert!(refused.to_string().contains("recording"), "{refused}");
        let (_mono, opened) = test_capture(&engine, 1);
        let refused = engine
            .install_capture(opened)
            .expect_err("no switch while recording");
        assert!(refused.to_string().contains("recording"), "{refused}");
        assert_eq!(session(&engine), before);

        engine.stop_recording().expect("the take stops");
        let (_mono, opened) = test_capture(&engine, 1);
        engine
            .install_capture(opened)
            .expect("after the take the switch commits");
        engine.stop().expect("the engine stops");
    }

    /// Stopping the engine ends the take cleanly: its files are finished and readable, and the
    /// take says the engine ended it.
    #[test]
    fn stopping_the_engine_ends_the_take_cleanly() {
        let parent = crate::recorder::tests::scratch_directory("engine-stop");
        let (mut engine, mut capture, _phone) = local_engine();
        engine
            .start_recording(take_of(&parent, vec![0, 1]))
            .expect("the take starts");
        capture.write(&[0.25f32; 2 * 64]);
        capture.write(&[0.25f32; 2 * 64]);
        assert!(until(|| engine.recording().frames == 128));

        engine.stop().expect("the engine stops");

        let status = engine.recording();
        assert_eq!(status.state, crate::recorder::RecordingState::Idle);
        let take = status.last_take.expect("the take ended");
        assert_eq!(take.ended, TakeEnd::EngineStopped);
        assert_eq!(take.frames, 128);
        assert_eq!(wav_samples(&take.folder.join("01 CH 1.wav")).len(), 128);
        let json: serde_json::Value = serde_json::from_reader(
            std::fs::File::open(take.folder.join("take.json")).expect("take.json exists"),
        )
        .expect("take.json is JSON");
        assert_eq!(json["ended"]["how"], "engineStopped");
    }

    /// A disk error stops only the recording: the status says why, and capture, the phones and
    /// the local output carry on.
    #[test]
    fn a_disk_error_stops_only_the_recording() {
        let parent = crate::recorder::tests::scratch_directory("engine-disk");
        let (mut engine, mut capture, phone) = local_engine();
        let (player, opened) = test_output("Speakers", 2, 0);
        engine
            .install_output(Some(opened))
            .expect("the output installs");
        let fail = Arc::new(AtomicBool::new(false));
        engine
            .start_recording_with(
                take_of(&parent, vec![0]),
                crate::recorder::tests::FailingOpener {
                    failing: "01",
                    fail: Arc::clone(&fail),
                },
                crate::recorder::tests::eager(),
            )
            .expect("the take starts");
        capture.write(&[0.5f32; 2 * 64]);
        // The block is written and its header refreshed before the disk fails.
        assert!(until(|| engine.recorder.written() == 64));

        fail.store(true, Ordering::Relaxed);
        capture.write(&[0.5f32; 2 * 64]);
        assert!(until(|| matches!(
            engine.recording().state,
            crate::recorder::RecordingState::Failed { .. }
        )));

        capture.write(&[0.5f32; 2 * 64]);
        for _ in 0..3 {
            assert!(phone_hears(&phone), "the phones keep hearing");
        }
        assert!(
            until(|| player.queued() == 3 * 64),
            "the local output plays"
        );
        assert_eq!(engine.local_output().state, LocalOutputState::Running);
        let summary = engine
            .stop_recording()
            .expect("the failed take is collected");
        let TakeEnd::DiskError { reason } = summary.ended else {
            panic!("the disk ended the take: {:?}", summary.ended);
        };
        assert!(reason.contains("no space left"), "{reason}");
        engine.stop().expect("the engine stops");
    }

    /// A marker added through the engine lands on the first frame of the block being recorded:
    /// after three blocks of 64, frame 192.
    #[test]
    fn a_marker_lands_on_the_block_being_recorded() {
        let parent = crate::recorder::tests::scratch_directory("engine-marker");
        let (mut engine, mut capture, _phone) = local_engine();
        assert!(engine.add_marker("Early").is_err(), "no take records");
        engine
            .start_recording(take_of(&parent, vec![0]))
            .expect("the take starts");
        for _ in 0..3 {
            capture.write(&[0.5f32; 2 * 64]);
        }
        assert!(until(|| engine.recording().frames == 192));

        let marker = engine.add_marker("Chorus").expect("a take records");

        assert_eq!(marker.frame, 192);
        assert_eq!(engine.recording().markers, vec![marker.clone()]);
        let summary = engine.stop_recording().expect("the take stops");
        assert_eq!(summary.markers, vec![marker]);
        engine.stop().expect("the engine stops");
    }

    /// The estimate lays out the tracks as a take would (a linked pair is one stereo file) and
    /// reads the folder's disk; a request that could not start a take is refused.
    #[test]
    fn the_disk_estimate_follows_the_armed_tracks() {
        let parent = crate::recorder::tests::scratch_directory("engine-estimate");
        let (mut engine, _capture, _phone) = local_engine();
        engine
            .set_channels(vec![
                ChannelMeta {
                    linked_to_next: true,
                    ..channel("Keys", ChannelKind::Keys)
                },
                channel("", ChannelKind::Keys),
            ])
            .expect("valid details");

        let estimate = engine
            .record_estimate(&RecordRequest {
                include_local: true,
                ..take_of(&parent, vec![0])
            })
            .expect("a take could start");

        assert_eq!(
            estimate.bytes_per_second,
            4 * 3 * 48_000,
            "the pair and the mix"
        );
        assert_eq!(estimate.free_bytes.is_some(), cfg!(windows));
        assert_eq!(
            estimate.minutes_left,
            estimate
                .free_bytes
                .and_then(|bytes| minutes_left(bytes, estimate.bytes_per_second))
        );
        let refused = engine
            .record_estimate(&take_of(&parent, vec![]))
            .expect_err("nothing is armed");
        assert!(refused.to_string().contains("arm"), "{refused}");
        engine.stop().expect("the engine stops");
    }

    /// A soundcheck of the take in `folder` from its start, not looping.
    fn soundcheck_of(folder: &std::path::Path) -> SoundcheckRequest {
        SoundcheckRequest {
            take_folder: folder.to_owned(),
            start_frame: 0,
            looping: false,
        }
    }

    /// The soundcheck refusal in `error`.
    fn soundcheck_refusal<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a SoundcheckError> {
        error.downcast_ref::<SoundcheckError>()
    }

    /// The take (CH 1 at 0.5) plays in place of the inputs, paced by the local output's
    /// callbacks: the output plays the local mix of the take exactly as it would of the same live
    /// input, the meters show the take, and live audio is neither heard nor metered. Stopping
    /// takes the engine back to the same capture, which is heard again at once.
    #[test]
    fn a_take_plays_in_place_of_the_inputs_and_stopping_returns_to_live() {
        let (mut engine, mut capture, phone) = local_engine();
        let (mut player, opened) = test_output("Speakers", 2, 0);
        player.set_clock(Arc::clone(&engine.playback_clock));
        engine
            .install_output(Some(opened))
            .expect("the output installs");
        // Only the output's callbacks pace the take in this test.
        engine
            .playback_clock
            .set_grace(std::time::Duration::from_secs(600));
        let folder = crate::soundcheck::tests::constant_take("engine-plays", 4_800, 0.5);
        let events = engine.subscribe();

        let status = engine
            .start_soundcheck(soundcheck_of(&folder))
            .expect("the take plays");

        assert_eq!(status.state, crate::soundcheck::SoundcheckState::Playing);
        assert_eq!(status.take_folder.as_deref(), Some(folder.as_path()));
        assert_eq!(status.take_name.as_deref(), Some("Soundcheck"));
        assert_eq!(status.length_frames, 4_800);
        assert_eq!(status.position_frames, 0);
        assert_eq!(status.clock, crate::soundcheck::SoundcheckClock::Output);
        assert_eq!(engine.status().soundcheck, status);
        assert_eq!(drain(&events), vec![EngineEvent::SoundcheckChanged]);
        // The whole take is queued (it is shorter than the preroll) and the capture set aside.
        assert!(until(|| engine.soundcheck.queued() == 4_800));
        assert!(until(|| engine
            .capture_stats
            .suspended
            .load(Ordering::Acquire)));
        capture.write(&[1.0f32; 2 * 64]);

        // Each callback plays 64 frames, and one block of the take follows it; the output plays
        // once its ring holds its callback and two blocks.
        let mut buffer = [9.0f32; 2 * 64];
        for callback in 1..=3 {
            player.fill(&mut buffer);
            assert!(until(|| player.queued() == 64 * callback));
        }
        player.fill(&mut buffer);
        assert!(until(|| engine.soundcheck().position_frames == 256));
        let mut heard = [0.0f32; 2];
        crate::mix::MixPlan::new(&engine.local_mix(), 2).mix_into(&[0.5, 0.0], &mut heard);
        for frame in buffer.chunks_exact(2) {
            assert_eq!(frame, &heard[..], "the output plays the take's local mix");
        }
        assert!(phone_hears(&phone), "the phones hear the take");
        let levels = engine.meters().channel_levels;
        assert!(levels[0] > 0 && levels[0] <= 50, "{levels:?}");
        assert_eq!(levels[1], 0, "live audio is not metered");
        // No callback, no audio: the timer stays out of it while the output plays.
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(engine.soundcheck().position_frames, 256);

        let off = engine.stop_soundcheck().expect("the soundcheck stops");

        assert_eq!(off, SoundcheckStatus::default());
        assert_eq!(engine.soundcheck(), SoundcheckStatus::default());
        assert_eq!(drain(&events), vec![EngineEvent::SoundcheckChanged]);
        assert!(!engine.capture_stats.suspended.load(Ordering::Acquire));
        capture.write(&[-0.25f32; 2 * 64]);
        assert!(until(|| engine.meters().channel_levels[1] > 0));
        assert_eq!(engine.device_name(), "Test input (2)", "the same capture");
        assert!(engine.stop_soundcheck().is_err(), "nothing plays now");
        engine.stop().expect("the engine stops");
    }

    /// Without a local output the engine's timer paces the take: it never runs ahead of the time
    /// that passed since it started, and the phones hear it.
    #[test]
    fn without_a_local_output_a_take_plays_on_the_timer() {
        let (mut engine, _capture, phone) = local_engine();
        let folder = crate::soundcheck::tests::constant_take("engine-timer", 48_000, 0.5);
        let started = Instant::now();

        engine
            .start_soundcheck(soundcheck_of(&folder))
            .expect("the take plays");

        assert!(until(|| engine.soundcheck().position_frames >= 4_800));
        let position = engine.soundcheck().position_frames;
        let elapsed = started.elapsed();
        assert!(
            position as usize <= crate::pipeline::frames_in(elapsed) + 64,
            "the timer ran ahead: {position} frames after {elapsed:?}"
        );
        assert_eq!(
            engine.soundcheck().clock,
            crate::soundcheck::SoundcheckClock::Timer
        );
        assert!(phone_hears(&phone));
        engine.stop().expect("the engine stops");
        assert_eq!(engine.soundcheck(), SoundcheckStatus::default());
    }

    /// A take never records playback, and the device a take plays on cannot change: recording,
    /// switches and a second soundcheck are refused while one plays, and a soundcheck while a
    /// take records.
    #[test]
    fn recording_switches_and_a_second_take_wait_for_the_soundcheck() {
        let parent = crate::recorder::tests::scratch_directory("engine-soundcheck-refusals");
        let folder = crate::soundcheck::tests::constant_take("engine-refusals", 4_800, 0.5);
        let (mut engine, _capture, _phone) = local_engine();
        engine
            .start_recording(take_of(&parent, vec![0]))
            .expect("the take starts");

        let refused = engine
            .start_soundcheck(soundcheck_of(&folder))
            .expect_err("no soundcheck while recording");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::Recording)
        );
        engine.stop_recording().expect("the take stops");

        engine
            .start_soundcheck(soundcheck_of(&folder))
            .expect("the take plays");
        let before = session(&engine);
        let refused = engine
            .start_recording(take_of(&parent, vec![0]))
            .expect_err("no recording during a soundcheck");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::RecordingRefused)
        );
        assert_eq!(
            std::fs::read_dir(&parent).unwrap().count(),
            1,
            "no take folder was made"
        );
        let refused = engine
            .switch_device(None)
            .expect_err("no switch during a soundcheck");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::SwitchRefused)
        );
        let (_mono, opened) = test_capture(&engine, 1);
        let refused = engine
            .install_capture(opened)
            .expect_err("no switch during a soundcheck");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::SwitchRefused)
        );
        assert_eq!(session(&engine), before);
        let refused = engine
            .start_soundcheck(soundcheck_of(&folder))
            .expect_err("one take at a time");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::AlreadyPlaying)
        );

        engine.stop_soundcheck().expect("the soundcheck stops");
        let refused = engine.stop_soundcheck().expect_err("nothing plays");
        assert_eq!(
            soundcheck_refusal(&*refused),
            Some(&SoundcheckError::NotPlaying)
        );
        engine
            .start_recording(take_of(&parent, vec![0]))
            .expect("after the soundcheck a take records");
        engine.stop().expect("the engine stops");
    }

    /// A take that does not fit the device (here CH 3 on a stereo device) or cannot be read is
    /// refused with its reason, and the inputs stay live.
    #[test]
    fn a_take_that_does_not_fit_is_refused_and_the_inputs_stay_live() {
        let (mut engine, mut capture, _phone) = local_engine();
        let wide = crate::soundcheck::tests::take_of_input("engine-wide", 3);

        let refused = engine
            .start_soundcheck(soundcheck_of(&wide))
            .expect_err("CH 3 is past the device");
        assert!(
            matches!(
                soundcheck_refusal(&*refused),
                Some(SoundcheckError::Layout { .. })
            ),
            "{refused}"
        );
        let missing = engine
            .start_soundcheck(soundcheck_of(&wide.join("no such take")))
            .expect_err("no take there");
        assert!(
            matches!(
                soundcheck_refusal(&*missing),
                Some(SoundcheckError::TakeUnreadable { .. })
            ),
            "{missing}"
        );

        assert_eq!(engine.soundcheck(), SoundcheckStatus::default());
        assert!(!engine.capture_stats.suspended.load(Ordering::Acquire));
        capture.write(&[0.5f32; 2 * 64]);
        assert!(engine
            .meters()
            .channel_levels
            .iter()
            .all(|level| *level > 0));
        engine.stop().expect("the engine stops");
    }
}
