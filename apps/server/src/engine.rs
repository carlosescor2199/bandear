use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use cpal::traits::{HostTrait, StreamTrait};

use crate::capture::{
    build_input_stream, describe_buffer, input_host, select_device, select_device_by_name,
    select_input_config, AudioHost, TARGET_SAMPLE_RATE,
};
use crate::cli::{resolve_targets, validate_groups, EngineConfig, GroupDefinition};
use crate::control::{spawn_control_thread, ConfigState, ControlPeers};
use crate::discovery::spawn_discovery_thread;
use crate::health::{link_status, LinkStatus};
use crate::mix::{Group, GroupLayout, MixUpdate, MixValues, MAX_MIX_CHANNELS};
use crate::network::{spawn_network_thread, PacketStats};
use crate::protocol::AudioPacket;
use crate::targets::{TargetEntry, TargetRegistry};

const CHANNEL_CAPACITY: usize = 8;

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
}

/// One configured group of source channels, as the observation API exposes it.
///
/// Channels are 0-based source indices, the same numbering the protocol's `config` message and the
/// mixer use. Membership is fixed at startup and is never rewritten by a device switch; instead
/// [Self::invalid] reports when the current device no longer has one of the channels the group names,
/// so a UI can warn without the engineer's configuration being destroyed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupStatus {
    pub name: String,
    pub channels: Vec<usize>,
    /// True when the current capture device is missing a channel this group names. An invalid group
    /// is reported and kept, but is excluded from the mixer so it cannot affect audio.
    pub invalid: bool,
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
/// exactly as a phone's is. An invalid address, an unknown musician or an update that cannot be
/// applied is an error naming the problem, and then nothing changes and nothing is announced.
pub(crate) fn set_target_mix(
    targets: &TargetRegistry,
    events: &EventBus,
    value: &str,
    update: &MixUpdate,
) -> Result<MixValues, String> {
    let requested = value
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid target `{value}`: {error}"))?;
    let (address, mix_state) = targets
        .read()
        .iter()
        .find(|entry| entry.address.ip() == requested.ip())
        .map(|entry| (entry.address, Arc::clone(&entry.mix_state)))
        .ok_or_else(|| format!("target `{value}` is not configured"))?;
    let applied = mix_state.apply(update)?;
    events.emit(EngineEvent::MixChanged {
        address,
        origin: MixOrigin::Engine,
    });
    Ok(applied)
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

/// How long the send loop waits for a packet before it counts the wait as silence for the meters.
///
/// A capture that stops delivering (a device that vanished, a stopped stream) passes no audio time,
/// so an audio-time fall alone would freeze the last level. The send loop waits on the packet queue
/// anyway; when nothing arrives for this long, it lowers every meter by this much audio.
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
    pub(crate) fn observe(&self, samples: &[i16], channels: usize) {
        if channels == 0 || channels > MAX_MIX_CHANNELS || samples.len() % channels != 0 {
            return;
        }
        let frames = samples.len() / channels;
        for (channel, level) in self.levels.iter().enumerate().take(channels) {
            let mut peak = 0u16;
            for frame in 0..frames {
                let magnitude = samples[frame * channels + channel].unsigned_abs();
                if magnitude > peak {
                    peak = magnitude;
                }
            }
            hold_peak(level, peak_to_level(peak), frames);
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
fn stereo_peak(stereo: &[i16]) -> [u8; 2] {
    let mut left = 0u16;
    let mut right = 0u16;
    for frame in stereo.chunks_exact(2) {
        left = left.max(frame[0].unsigned_abs());
        right = right.max(frame[1].unsigned_abs());
    }
    [peak_to_level(left), peak_to_level(right)]
}

/// The left and right peak of one musician's mixed output, held for the meter read.
///
/// The send loop updates it right after mixing, so it reads what that musician's phone is sent:
/// after their gains, pans, solos, mutes, master and level cap. Like [LevelMeter], an update costs
/// one pass over the block and two atomic updates, with no allocation, lock or clock, the level
/// falls with the audio, and reading is a pure load.
#[derive(Default)]
pub(crate) struct OutputMeter {
    levels: [AtomicU32; 2],
}

impl OutputMeter {
    /// Lets the held levels fall by this block's length and raises them to its peaks (see
    /// [hold_peak]). A buffer that is not whole stereo frames is not a mixed output and is ignored,
    /// as [LevelMeter::observe] ignores incomplete frames.
    pub(crate) fn observe(&self, stereo: &[i16]) {
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
        for level in &self.levels {
            hold_peak(level, 0, frames);
        }
    }

    /// Returns both sides to zero, for a capture that stopped.
    pub(crate) fn clear(&self) {
        for level in &self.levels {
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
}

/// One musician's output peak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicianMeters {
    /// The musician's address as [EngineStatus::musicians] reports it.
    pub address: SocketAddr,
    /// The 0-100 peak of the stereo mix the musician is sent, left then right.
    pub output_levels: [u8; 2],
}

/// Reads the input levels of `channels` source channels and every musician's output level. A pure
/// read: it changes no meter.
pub(crate) fn read_meters(
    levels: &LevelMeter,
    channels: usize,
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
    }
}

/// Returns every input and output meter to zero.
///
/// A stopped capture delivers no more audio, so no audio time passes for the levels to fall by,
/// and the send loop that would count the silence has ended too; the engine clears the meters
/// instead, so a reader never sees the last level frozen.
pub(crate) fn clear_meters(levels: &LevelMeter, entries: &[TargetEntry]) {
    levels.clear();
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

/// Snapshots the fixed group layout into the observation-friendly [GroupStatus] list.
///
/// Used at startup, where [validate_groups] has already proven every group valid.
fn group_statuses(layout: &GroupLayout) -> Vec<GroupStatus> {
    layout
        .groups()
        .iter()
        .map(|group| GroupStatus {
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

/// Re-checks the engineer's fixed groups against a channel count without failing or mutating them.
///
/// Unlike [validate_groups], which rejects an out-of-range group at startup, this is the lenient
/// post-switch check: a channel the new device does not have marks the whole group invalid but keeps
/// its definition, so switching back restores it. Definitions are read only.
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
            name: definition.name.clone(),
            channels: channels.clone(),
            invalid,
        });
        // Invalid groups keep their slot in the list so group indices line up with `group_levels`,
        // but map no channel; the mixer then cannot apply their level to anything.
        mixer_groups.push(Group {
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
    pub(crate) groups: Vec<GroupStatus>,
    pub(crate) mixer_layout: GroupLayout,
    pub(crate) resend_config: bool,
}

/// Plans the state change for a capture device that now has `new_channels` channels.
///
/// Pure and non-mutating: the previous group definitions are read, the groups are re-validated, and
/// `resend_config` is set only when the channel count actually changed, which is when connected
/// clients must rebuild.
pub(crate) fn plan_device_switch(
    current_channels: u8,
    definitions: &[GroupDefinition],
    new_channels: u8,
) -> DeviceSwitchPlan {
    let revalidation = revalidate_groups(definitions, usize::from(new_channels));
    DeviceSwitchPlan {
        source_channels: new_channels,
        groups: revalidation.statuses,
        mixer_layout: revalidation.mixer_layout,
        resend_config: new_channels != current_channels,
    }
}

/// Turns a fallible capture rebuild into the plan to commit, or leaves the previous state alone.
///
/// The rebuild touches real hardware and can fail; keeping the conversion to a plan in a pure
/// function is what makes the "a failed switch preserves the previous state" rule testable without
/// an audio device.
pub(crate) fn plan_switch_from_attempt(
    current_channels: u8,
    definitions: &[GroupDefinition],
    attempt: Result<u16, Box<dyn Error>>,
) -> Result<DeviceSwitchPlan, Box<dyn Error>> {
    let new_channels = attempt?.min(u16::from(u8::MAX)) as u8;
    Ok(plan_device_switch(
        current_channels,
        definitions,
        new_channels,
    ))
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
    groups: Vec<GroupStatus>,
    levels: Arc<LevelMeter>,
    /// The live routing table, shared with the network and control threads so targets can be added
    /// and removed while both keep running.
    targets: Arc<TargetRegistry>,
    /// The group layout the last committed device switch produced, used to give a newly added
    /// target the same channel-to-group membership the existing mix states carry.
    current_layout: GroupLayout,
    stopped: Arc<AtomicBool>,
    samples_seen: Arc<AtomicU64>,
    /// The packet sequence shared with whichever capture stream is currently running, so a switch
    /// continues the numbering instead of restarting it.
    sequence: Arc<AtomicU64>,
    packet_stats: Arc<PacketStats>,
    peers: ControlPeers,
    config_state: Arc<ConfigState>,
    events: EventBus,
    stream: Option<cpal::Stream>,
    packet_sender: Option<SyncSender<AudioPacket>>,
    network_thread: Option<JoinHandle<()>>,
    control_thread: Option<JoinHandle<()>>,
    discovery_thread: Option<JoinHandle<()>>,
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
        EngineStatus {
            device_name: self.device_name.clone(),
            capture_format: self.capture_format.clone(),
            control_port: self.control_port,
            sample_rate: TARGET_SAMPLE_RATE,
            source_channels: self.source_channels,
            groups: self.groups.clone(),
            channel_levels: self.levels.read(usize::from(self.source_channels)),
            stopped: self.is_stopped(),
            samples_received: self.samples_seen.load(Ordering::Relaxed),
            packets_sent: self.packet_stats.sent.load(Ordering::Relaxed),
            packets_discarded: self.packet_stats.discarded.load(Ordering::Relaxed),
            musicians: join_musicians(&targets, &connected, &counters, &mixes, &announced, &links),
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
    /// Only the capture stream is rebuilt: the UDP network thread, the control WebSocket server and
    /// the discovery beacon are untouched, and the packet sequence continues from where it was. The
    /// new stream is built and started before the old one is dropped, so a device that has vanished
    /// or a format that cannot be opened leaves the previous capture exactly as it was and returns
    /// the error. On success, groups are re-validated (never rewritten) and connected clients are
    /// told to rebuild when the channel count changed.
    pub fn switch_device(&mut self, filter: Option<&str>) -> Result<CaptureFormat, Box<dyn Error>> {
        if self.is_stopped() {
            return Err("the engine is stopping".into());
        }
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
        let packet_sender = self.packet_sender.clone().ok_or("the engine is stopping")?;

        // Build and start the replacement before touching the current capture, so any failure below
        // is a no-op for the running session.
        let (new_stream, effective_buffer) = build_input_stream(
            &device,
            &supported_config,
            self.buffer_frames,
            packet_sender,
            Arc::clone(&self.samples_seen),
            Arc::clone(&self.packet_stats),
            Arc::clone(&self.levels),
            Arc::clone(&self.sequence),
        )?;
        new_stream
            .play()
            .map_err(|error| format!("could not start input capture: {error}"))?;

        let plan = plan_switch_from_attempt(
            self.source_channels,
            &self.group_definitions,
            Ok(supported_config.channels()),
        )?;

        // Commit: the new capture is confirmed working.
        self.stream = Some(new_stream);
        self.device_name = device.to_string();
        self.capture_format = CaptureFormat {
            channels: supported_config.channels(),
            sample_rate: supported_config.sample_rate(),
            sample_format: format!("{:?}", supported_config.sample_format()),
            buffer_size: describe_buffer(effective_buffer),
        };
        self.source_channels = plan.source_channels;
        self.groups = plan.groups;
        for entry in self.targets.read().iter() {
            entry.mix_state.set_group_layout(&plan.mixer_layout);
        }
        self.current_layout = plan.mixer_layout;
        if plan.resend_config {
            self.config_state.set_channels(plan.source_channels);
        }
        // The old device's levels describe a stream that is gone, and channels the new device lacks
        // would keep theirs frozen until a switch back; the new stream raises its own at once.
        self.levels.clear();
        Ok(self.capture_format.clone())
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
            .add(value, &self.current_layout)
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
    /// to the musician's phone as `mix_state` so its screen follows. Returns the mix now stored.
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

    /// Stops capture and joins the engine threads, the same clean shutdown Ctrl+C triggers.
    pub fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        self.stopped.store(true, Ordering::SeqCst);
        // Stop producing before draining: drop the capture stream first, then the sender, so the
        // network thread sees the channel close and exits.
        self.stream = None;
        self.packet_sender = None;
        if let Some(thread) = self.network_thread.take() {
            thread.join().map_err(|_| "UDP network thread panicked")?;
        }
        // Capture and the send loop have ended, so nothing would ever lower the last levels.
        clear_meters(&self.levels, &self.targets.read());
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
            "Input is {} Hz; applying integer decimation to {TARGET_SAMPLE_RATE} Hz before packaging.",
            supported_config.sample_rate()
        );
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let source_channels = supported_config.channels().min(u16::from(u8::MAX)) as u8;
    let group_layout = Arc::new(validate_groups(
        &config.groups,
        usize::from(source_channels),
    )?);
    let target_registry = Arc::new(TargetRegistry::new(&targets, &group_layout)?);
    let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
    let config_state = Arc::new(ConfigState::new(source_channels));
    let events = EventBus::default();

    let control_thread = spawn_control_thread(
        config.control_port,
        Arc::clone(&target_registry),
        Arc::clone(&stopped),
        Arc::clone(&config_state),
        Arc::clone(&group_layout),
        Arc::clone(&peers),
        events.clone(),
    );
    let discovery_thread = spawn_discovery_thread(
        Arc::clone(&target_registry),
        config.control_port,
        TARGET_SAMPLE_RATE,
        Arc::clone(&stopped),
    );
    let (packet_sender, packet_receiver) = sync_channel(CHANNEL_CAPACITY);
    let packet_stats = Arc::new(PacketStats {
        sent: AtomicU64::new(0),
        discarded: AtomicU64::new(0),
    });
    let levels = Arc::new(LevelMeter::new());
    let network_thread = spawn_network_thread(
        packet_receiver,
        Arc::clone(&target_registry),
        Arc::clone(&packet_stats),
        Arc::clone(&levels),
    )?;
    let samples_seen = Arc::new(AtomicU64::new(0));
    let sequence = Arc::new(AtomicU64::new(0));
    let (stream, effective_buffer) = build_input_stream(
        &device,
        &supported_config,
        buffer_frames,
        packet_sender.clone(),
        Arc::clone(&samples_seen),
        Arc::clone(&packet_stats),
        Arc::clone(&levels),
        Arc::clone(&sequence),
    )?;
    stream
        .play()
        .map_err(|error| format!("could not start input capture: {error}"))?;

    let capture_format = CaptureFormat {
        channels: supported_config.channels(),
        sample_rate: supported_config.sample_rate(),
        sample_format: format!("{:?}", supported_config.sample_format()),
        buffer_size: describe_buffer(effective_buffer),
    };

    Ok(Some(EngineHandle {
        device_name,
        capture_format,
        control_port: config.control_port,
        buffer_frames,
        host: config.host,
        source_channels,
        group_definitions: config.groups,
        groups: group_statuses(&group_layout),
        levels,
        targets: target_registry,
        current_layout: (*group_layout).clone(),
        stopped,
        samples_seen,
        sequence,
        packet_stats,
        peers,
        config_state,
        events,
        stream: Some(stream),
        packet_sender: Some(packet_sender),
        network_thread: Some(network_thread),
        control_thread: Some(control_thread),
        discovery_thread: Some(discovery_thread),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GroupDefinition;

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("test IP should parse")
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

        let musicians = join_musicians(&targets, &connected, &counters, &mixes, &announced, &links);

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
        meter.observe(&[16_384, 32_767, -16_384, -1], 2);

        assert_eq!(meter.read(2), vec![50, 100]);
    }

    /// Readers only look: the meter stream, the status events and any poller can read as often as
    /// they like without changing what the others see.
    #[test]
    fn reading_a_meter_never_lowers_it() {
        let inputs = LevelMeter::new();
        inputs.observe(&[16_384, 32_767], 2);
        let output = OutputMeter::default();
        output.observe(&[32_767, 16_384]);

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
        inputs.observe(&[32_767], 1);
        output.observe(&[32_767, 32_767]);

        inputs.observe(&vec![0; 7_200], 1);
        output.observe(&vec![0; 2 * 7_200]);
        assert_eq!(inputs.read(1), vec![90], "0.15 s after a full-scale peak");
        assert_eq!(output.read(), [90, 90]);

        inputs.observe(&vec![0; 64_080], 1);
        output.observe(&vec![0; 2 * 64_080]);
        assert_eq!(inputs.read(1), vec![1], "one step short of 1.5 s");
        assert_eq!(output.read(), [1, 1]);

        inputs.observe(&vec![0; 720], 1);
        output.observe(&vec![0; 2 * 720]);
        assert_eq!(inputs.read(1), vec![0], "1.5 s after the peak");
        assert_eq!(output.read(), [0, 0]);
    }

    /// A louder block shows at once; a quieter one does not pull the level below its fall.
    #[test]
    fn a_new_peak_above_the_fallen_level_shows_at_once() {
        let inputs = LevelMeter::new();
        let output = OutputMeter::default();
        inputs.observe(&[32_767, 32_767], 2);
        output.observe(&[32_767, 32_767]);
        inputs.observe(&vec![0; 2 * 36_000], 2);
        output.observe(&vec![0; 2 * 36_000]);
        assert_eq!(inputs.read(2), vec![50, 50], "half way down after 0.75 s");
        assert_eq!(output.read(), [50, 50]);

        // One frame: 80 on the first side, 30 on the second.
        inputs.observe(&[26_214, 9_830], 2);
        output.observe(&[26_214, 9_830]);

        assert_eq!(inputs.read(2), vec![80, 50]);
        assert_eq!(output.read(), [80, 50]);
    }

    #[test]
    fn a_stereo_peak_reads_each_side_of_an_interleaved_buffer() {
        // Left peaks at half scale (on its negative swing), right at full scale.
        assert_eq!(stereo_peak(&[8_000, 100, -16_384, 32_767]), [50, 100]);
        assert_eq!(stereo_peak(&[]), [0, 0]);
        assert_eq!(stereo_peak(&[0, i16::MIN]), [0, 100]);
    }

    #[test]
    fn an_output_meter_holds_each_sides_peak() {
        let meter = OutputMeter::default();

        meter.observe(&[16_384, 32_767]);
        // A quieter block does not pull the held peak below its fall.
        meter.observe(&[100, 100]);
        assert_eq!(meter.read(), [50, 100]);
        // A buffer that is not whole stereo frames is ignored.
        meter.observe(&[32_767, 32_767, 32_767]);
        assert_eq!(meter.read(), [50, 100]);
    }

    /// A stopped stream delivers no more audio, so no audio time passes for the meters to fall by:
    /// the engine clears them when it stops capturing.
    #[test]
    fn clearing_the_meters_returns_every_level_to_zero() {
        let address: SocketAddr = "192.168.1.3:50000".parse().unwrap();
        let registry = TargetRegistry::new(&[address], &GroupLayout::default())
            .expect("registry should build");
        registry.read()[0].output_meter.observe(&[32_767, 16_384]);
        let inputs = LevelMeter::new();
        inputs.observe(&[32_767, 16_384], 2);

        clear_meters(&inputs, &registry.read());

        assert_eq!(
            read_meters(&inputs, 2, &registry.read()),
            EngineMeters {
                channel_levels: vec![0, 0],
                musicians: vec![MusicianMeters {
                    address,
                    output_levels: [0, 0],
                }],
            }
        );
    }

    #[test]
    fn the_meter_read_carries_input_peaks_and_each_musicians_output_peaks() {
        let first: SocketAddr = "192.168.1.3:50000".parse().unwrap();
        let second: SocketAddr = "192.168.1.4:50000".parse().unwrap();
        let registry = TargetRegistry::new(&[first, second], &GroupLayout::default())
            .expect("registry should build");
        registry.read()[0].output_meter.observe(&[32_767, -16_384]);
        let inputs = LevelMeter::new();
        inputs.observe(&[16_384, 0, -32_767, 0], 2);

        let meters = read_meters(&inputs, 2, &registry.read());

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
            }
        );
        assert_eq!(
            read_meters(&inputs, 2, &registry.read()),
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

        meter.observe(&[1, 2, 3], 2);
        meter.observe(&[], 0);

        assert_eq!(meter.read(2), vec![0, 0]);
    }

    #[test]
    fn group_statuses_carry_names_and_zero_based_channels() {
        use crate::mix::Group;

        let layout = GroupLayout::new(vec![Group {
            name: "Drums".to_owned(),
            channels: vec![0, 1],
        }]);

        assert_eq!(
            group_statuses(&layout),
            vec![GroupStatus {
                name: "Drums".to_owned(),
                channels: vec![0, 1],
                invalid: false,
            }]
        );
    }

    #[test]
    fn revalidating_groups_reports_out_of_range_groups_without_changing_them() {
        let definitions = vec![
            GroupDefinition {
                name: "Drums".to_owned(),
                channels: vec![1, 2],
            },
            GroupDefinition {
                name: "Vocals".to_owned(),
                channels: vec![5],
            },
        ];

        let revalidation = revalidate_groups(&definitions, 2);

        assert_eq!(revalidation.statuses.len(), 2);
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
            GroupDefinition {
                name: "Drums".to_owned(),
                channels: vec![1, 2],
            },
            GroupDefinition {
                name: "Vocals".to_owned(),
                channels: vec![3, 4],
            },
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
        let definitions = vec![GroupDefinition {
            name: "Drums".to_owned(),
            channels: vec![1, 2, 3, 4],
        }];

        let smaller = plan_device_switch(4, &definitions, 2);
        assert_eq!(smaller.source_channels, 2);
        assert!(smaller.resend_config);
        assert!(smaller.groups[0].invalid);
        assert_eq!(smaller.mixer_layout.channel_group()[0], None);

        let restored = plan_device_switch(2, &definitions, 4);
        assert_eq!(restored.source_channels, 4);
        assert!(restored.resend_config);
        assert!(!restored.groups[0].invalid);
        assert_eq!(restored.mixer_layout.channel_group()[0], Some(0));
        assert_eq!(restored.mixer_layout.channel_group()[3], Some(0));
    }

    #[test]
    fn a_switch_to_the_same_channel_count_does_not_ask_for_a_config_resend() {
        let definitions = vec![GroupDefinition {
            name: "Drums".to_owned(),
            channels: vec![1, 2],
        }];

        let plan = plan_device_switch(2, &definitions, 2);

        assert!(!plan.resend_config);
    }

    #[test]
    fn a_failed_capture_rebuild_yields_no_plan_and_leaves_the_previous_state() {
        let definitions = vec![GroupDefinition {
            name: "Drums".to_owned(),
            channels: vec![1, 2, 3, 4],
        }];
        let error: Box<dyn Error> = "input device \"Gone\" is no longer available".into();

        let result = plan_switch_from_attempt(4, &definitions, Err(error));

        assert!(result.is_err());
        // Nothing was planned or mutated: the previous configuration stands.
        assert_eq!(definitions[0].channels, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_successful_capture_rebuild_plans_the_new_channels_and_group_validity() {
        let definitions = vec![GroupDefinition {
            name: "Drums".to_owned(),
            channels: vec![1, 2, 3, 4],
        }];

        let plan = plan_switch_from_attempt(4, &definitions, Ok(2)).expect("plan should build");

        assert_eq!(plan.source_channels, 2);
        assert!(plan.resend_config);
        assert!(plan.groups[0].invalid);
    }
}
