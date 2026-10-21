use std::sync::atomic::{
    AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::{Arc, Mutex};

use crate::dsp::dynamics::{Limiter, LimiterDesign, LimiterSettings};
use crate::effects::{add_return, Effect, EffectLevels, Wet};

pub(crate) const MAX_MIX_CHANNELS: usize = 32;
pub(crate) const MAX_GROUPS: usize = 16;
pub(crate) const OUTPUT_CHANNELS: u8 = 2;

/// The next mix revision, shared by every mix in the process: a revision is never handed out
/// twice, and a mix created later (a musician added again) starts above every earlier revision.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

/// The sentinel stored for a channel that belongs to no group, or to an invalid one.
const NO_GROUP: u8 = u8::MAX;

/// A named group of source channels, with 0-based indices as the protocol and the mixer use them.
///
/// The id is the group's identity across edits (see [crate::GroupDefinition]); its position in the
/// layout is the index of its level in every mix's `group_levels`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) channels: Vec<usize>,
}

/// The level a new group, and a slot no group uses, starts at: neutral, so the group changes
/// nothing until the musician moves it.
pub(crate) const NEUTRAL_GROUP_LEVEL: u8 = 100;

/// Where each group's level comes from when the groups are edited: for every position of the new
/// layout, the position the same group (by id) had in the old one, or `None` for a new group and
/// for every slot past the new list.
pub(crate) type GroupSources = [Option<usize>; MAX_GROUPS];

/// Matches the groups of `new` to those of `old` by id (see [GroupSources]).
pub(crate) fn group_sources(old: &GroupLayout, new: &GroupLayout) -> GroupSources {
    let mut sources = [None; MAX_GROUPS];
    for (slot, group) in sources.iter_mut().zip(new.groups()) {
        *slot = old
            .groups()
            .iter()
            .position(|previous| previous.id == group.id);
    }
    sources
}

/// A mix's group levels after an edit of the groups: each kept group's level moves to its new
/// position, a new group starts at [NEUTRAL_GROUP_LEVEL], and a deleted group's level is dropped
/// (its slot, like every slot past the new list, is neutral again). Renaming a group or giving it
/// other channels keeps its level, because its id is unchanged.
pub(crate) fn regrouped_levels(
    levels: &[u8; MAX_GROUPS],
    sources: &GroupSources,
) -> [u8; MAX_GROUPS] {
    std::array::from_fn(|position| {
        sources[position].map_or(NEUTRAL_GROUP_LEVEL, |previous| levels[previous])
    })
}

/// The groups plus the derived channel-to-group lookup.
///
/// A layout is immutable: a change of groups (or of the device, which can invalidate one) builds a
/// new layout, and each mix copies its lookup into the atomics the network thread reads. A channel
/// belongs to at most one group, which validation enforces; the lookup is therefore a single owner
/// per channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GroupLayout {
    groups: Vec<Group>,
    channel_group: [Option<usize>; MAX_MIX_CHANNELS],
}

impl GroupLayout {
    pub(crate) fn new(groups: Vec<Group>) -> Self {
        let mut channel_group = [None; MAX_MIX_CHANNELS];
        for (index, group) in groups.iter().enumerate() {
            for &channel in &group.channels {
                if channel < MAX_MIX_CHANNELS {
                    channel_group[channel] = Some(index);
                }
            }
        }
        Self {
            groups,
            channel_group,
        }
    }

    pub(crate) fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub(crate) fn channel_group(&self) -> [Option<usize>; MAX_MIX_CHANNELS] {
        self.channel_group
    }

    /// Whether every group of `other` sits where the group with its id sits here, with none
    /// missing or added: then every group level is already in its place and an edit only renames
    /// groups or changes their channels.
    pub(crate) fn same_positions(&self, other: &GroupLayout) -> bool {
        self.groups.len() == other.groups.len()
            && self
                .groups
                .iter()
                .zip(&other.groups)
                .all(|(mine, theirs)| mine.id == theirs.id)
    }
}

/// Where a new musician's mix takes its group membership from.
///
/// A fixed [GroupLayout] answers with itself; the engine's live configuration answers with the
/// membership in force at the moment the mix is created. The target registry creates a mix under
/// its own write lock, so a mix created while the groups change gets either the old membership
/// (and is then moved with every other mix) or the new one, never a stale one left behind.
pub(crate) trait Membership {
    fn new_mix(&self) -> MixState;
}

impl Membership for GroupLayout {
    fn new_mix(&self) -> MixState {
        MixState::new(self)
    }
}

/// The engineer's channel flags the mixer and every mix writer act on, one bit per source channel
/// (bit `k` is channel `k`, 0-based), so each set is one atomic word in every mix.
///
/// `hidden` channels are never heard. `linked` marks the first channel of each stereo pair: a set
/// bit `k` links channels `k` and `k + 1`, which the channel details guarantee are both within the
/// mixer's 32 channels and in no other pair (see [crate::engine::check_channels]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChannelFlags {
    pub(crate) hidden: u32,
    pub(crate) linked: u32,
}

/// The first channel of every pair `linked` marks, in channel order.
fn pairs(linked: u32) -> impl Iterator<Item = usize> {
    (0..MAX_MIX_CHANNELS - 1).filter(move |&first| linked & (1 << first) != 0)
}

/// What the engineer sets for one musician beyond the mix itself: a cap on what they hear and the
/// channels their phone cannot change.
///
/// It is engineer policy, never client input: only the engine's own path changes it
/// ([crate::EngineHandle::set_engineer_controls]), a phone's `mix` has no field for it, and every
/// report of the mix carries it so the phone can show it.
///
/// - `limit_percent` (0-100, 100 by default) caps the musician's output as the phone's own level
///   cap (`max_level_percent`) does; the mixer applies the lower of the two.
/// - `locked` holds one bit per source channel (bit `k` is channel `k`, 0-based): the phone cannot
///   change a locked channel's level, pan, mute or solo, while the desk still can. Both sides of a
///   stereo pair are locked or unlocked together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineerControls {
    pub limit_percent: u8,
    pub locked: u32,
}

impl Default for EngineerControls {
    fn default() -> Self {
        Self {
            limit_percent: 100,
            locked: 0,
        }
    }
}

impl EngineerControls {
    /// Whether the phone is kept from changing `channel` (0-based).
    pub fn is_locked(&self, channel: usize) -> bool {
        channel < MAX_MIX_CHANNELS && self.locked & (1 << channel) != 0
    }
}

/// A partial change to one musician's [EngineerControls]: an absent field keeps its value.
///
/// `locked` replaces the lock of each channel from the first on (entry 0 is CH 1) and leaves the
/// rest alone, as the lists of a [MixUpdate] do, and a list longer than the mix is rejected. The
/// limit is clamped to 0-100. Changing one side of a stereo pair changes both; when one change
/// sets the two sides differently, the first channel's lock wins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineerUpdate {
    pub limit_percent: Option<i32>,
    pub locked: Option<Vec<bool>>,
}

impl EngineerUpdate {
    /// Merges this change into `current`, with both sides of each pair in `linked` kept alike, or
    /// names the list that cannot be applied.
    fn merge_into(
        &self,
        current: EngineerControls,
        linked: u32,
    ) -> Result<EngineerControls, String> {
        let before = lock_flags(current.locked);
        let mut merged = merge_flags("locked", self.locked.as_ref(), &before)?;
        for first in pairs(linked) {
            follow(&before, &mut merged, first);
        }
        Ok(EngineerControls {
            limit_percent: self
                .limit_percent
                .map_or(current.limit_percent, clamp_percent),
            locked: lock_mask(&merged),
        })
    }
}

/// One lock per channel, from a mask (bit `k` is channel `k`).
fn lock_flags(mask: u32) -> [bool; MAX_MIX_CHANNELS] {
    std::array::from_fn(|channel| mask & (1 << channel) != 0)
}

/// The mask of the channels whose lock is set.
fn lock_mask(flags: &[bool; MAX_MIX_CHANNELS]) -> u32 {
    flags
        .iter()
        .enumerate()
        .filter(|(_, &locked)| locked)
        .fold(0, |mask, (channel, _)| mask | 1 << channel)
}

/// `locked` with both sides of every pair in `linked` locked when either side is: a pair is one
/// strip, so a lock that reaches it covers the whole strip.
fn lock_pairs(locked: u32, linked: u32) -> u32 {
    pairs(linked).fold(locked, |locked, first| {
        let both = 0b11 << first;
        if locked & both != 0 {
            locked | both
        } else {
            locked
        }
    })
}

/// Who writes a mix: the desk may change any value, the musician's phone none the engineer locked
/// and, while their mix follows another musician's, no channel and no group level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Writer {
    Desk,
    Phone,
}

/// One channel of a mix as a preset keeps it and a copy carries it: its level, pan, mute and solo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetChannel {
    pub gain: u8,
    pub pan: u8,
    pub muted: bool,
    pub solo: bool,
}

/// What of a musician's mix the engineer can hand to other musicians: every channel's level, pan,
/// mute and solo and the group levels, as a preset keeps it. It never holds what is the musician's
/// own (the master volume, the phone's level cap and the master mute) nor the engineer's controls.
///
/// `channels` is in channel order, the first entry CH 1; entries past the mixer's 32 channels are
/// kept but never applied. `groups` names each group level by the group's stable id (see
/// [crate::GroupDefinition]), so a preset still fits after the groups were renamed, reordered or
/// given other channels: applied, a level goes to the live group with its id, a live group it does
/// not name keeps the musician's level, and an id no live group has is ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MixPreset {
    pub channels: Vec<PresetChannel>,
    pub groups: Vec<(String, u8)>,
}

impl MixPreset {
    /// The first `channels` channels of `values` (at most the mixer's 32) and its group levels by
    /// the ids of `layout`, the groups the mix is placed for.
    pub(crate) fn of(values: &MixValues, layout: &GroupLayout, channels: usize) -> Self {
        Self {
            channels: (0..channels.min(MAX_MIX_CHANNELS))
                .map(|channel| PresetChannel {
                    gain: values.channel_gains[channel],
                    pan: values.pans[channel],
                    muted: values.channel_muted[channel],
                    solo: values.channel_solo[channel],
                })
                .collect(),
            groups: layout
                .groups()
                .iter()
                .zip(values.group_levels)
                .map(|(group, level)| (group.id.clone(), level))
                .collect(),
        }
    }

    /// The desk's update that puts this preset on `current`, a mix placed for `layout` with the
    /// pairs `linked`: each listed channel up to the mixer's 32, and the level of every live group
    /// the preset names; every other value keeps its own. A pair's second side takes the first
    /// side's level, mute and solo, so the first side wins where the preset holds them apart.
    fn update_for(&self, current: &MixValues, layout: &GroupLayout, linked: u32) -> MixUpdate {
        let mut channels: Vec<PresetChannel> = self
            .channels
            .iter()
            .take(MAX_MIX_CHANNELS)
            .copied()
            .collect();
        for first in pairs(linked) {
            if let Some(&side) = channels.get(first) {
                if let Some(second) = channels.get_mut(first + 1) {
                    *second = PresetChannel {
                        pan: second.pan,
                        ..side
                    };
                }
            }
        }
        let mut group_levels = current.group_levels;
        for (slot, group) in group_levels.iter_mut().zip(layout.groups()) {
            if let Some((_, level)) = self.groups.iter().find(|(id, _)| *id == group.id) {
                *slot = *level;
            }
        }
        MixUpdate {
            channel_gains: Some(channels.iter().map(|side| i32::from(side.gain)).collect()),
            pans: Some(channels.iter().map(|side| i32::from(side.pan)).collect()),
            channel_muted: Some(channels.iter().map(|side| side.muted).collect()),
            channel_solo: Some(channels.iter().map(|side| side.solo).collect()),
            group_levels: Some(group_levels.iter().copied().map(i32::from).collect()),
            ..MixUpdate::default()
        }
    }
}

/// The desk's update that gives a mix every channel value and group level of `values`, in place:
/// what a copy and a mirror carry from one musician's mix to another's, both placed for the same
/// groups.
fn shared_update(values: &MixValues) -> MixUpdate {
    MixUpdate {
        channel_gains: Some(
            values
                .channel_gains
                .iter()
                .copied()
                .map(i32::from)
                .collect(),
        ),
        pans: Some(values.pans.iter().copied().map(i32::from).collect()),
        channel_muted: Some(values.channel_muted.to_vec()),
        channel_solo: Some(values.channel_solo.to_vec()),
        group_levels: Some(values.group_levels.iter().copied().map(i32::from).collect()),
        ..MixUpdate::default()
    }
}

/// Whether two mixes hold the same channel values and group levels: what a copy and a mirror carry.
fn same_shared(one: &MixValues, other: &MixValues) -> bool {
    one.channel_gains == other.channel_gains
        && one.pans == other.pans
        && one.channel_muted == other.channel_muted
        && one.channel_solo == other.channel_solo
        && one.group_levels == other.group_levels
}

pub(crate) struct MixState {
    channel_gains: [AtomicU8; MAX_MIX_CHANNELS],
    pans: [AtomicU8; MAX_MIX_CHANNELS],
    channel_muted: [AtomicBool; MAX_MIX_CHANNELS],
    channel_solo: [AtomicBool; MAX_MIX_CHANNELS],
    group_levels: [AtomicU8; MAX_GROUPS],
    /// Channel-to-group membership, one atomic slot per channel so a device switch can replace the
    /// mapping while the network thread keeps reading it lock-free. `NO_GROUP` means no group.
    channel_group: [AtomicU8; MAX_MIX_CHANNELS],
    /// The hidden channels ([ChannelFlags::hidden]), one word the network thread loads with the
    /// rest of the mix, so hiding an input silences it without a lock on the audio path.
    hidden: AtomicU32,
    /// The stereo pairs ([ChannelFlags::linked]). Only writers read it, under the writer lock.
    linked: AtomicU32,
    /// The engineer's limit ([EngineerControls::limit_percent]), loaded by the network thread with
    /// the rest of the mix, so the output ceiling needs no lock on the audio path.
    engineer_limit: AtomicU8,
    /// The engineer's locked channels ([EngineerControls::locked]); writers read it under the
    /// writer lock.
    locked: AtomicU32,
    volume_percent: AtomicU8,
    max_level_percent: AtomicU8,
    muted: AtomicBool,
    /// Serializes every writer ([MixState::apply] and [MixState::update]), so two writers (the
    /// musician's phone and the desktop) never interleave a read-merge-store and lose each other's
    /// fields. The network thread only loads the atomics and never takes it, so the audio path stays
    /// lock-free.
    ///
    /// It guards the mix's revision: every stored change takes a new, higher one, so a reader can
    /// tell which of two copies of the mix is newer whatever order they reach it in.
    writer: Mutex<u64>,
    /// The revision for the lock-free [MixState::snapshot], set after each store. A snapshot taken
    /// during a store may pair it with values of the change before; [MixState::current] never does.
    published: AtomicU64,
    /// The name of the musician whose mix this one follows (see [MixState::set_following]), or
    /// `None`. Changed and read by writers under the writer lock; never read by the audio path.
    following: Mutex<Option<Arc<str>>>,
    /// The master limiter the engineer set for this mix (see [LimiterControl]): engineer-only, and
    /// apart from the mix's values, so no copy, mirror or `mix` of the values touches it.
    limiter: LimiterControl,
    /// The return level of each shared effect in this mix, 0-100% (see [crate::effects]):
    /// engineer-only and apart from the mix's values, like the limiter. Both in one atomic (the
    /// reverb in the low byte, the delay in the high one), so a change to one effect's return is a
    /// single compare-and-swap that never loses a change made meanwhile to the other's, and the
    /// engine thread reads both from one moment.
    fx_returns: AtomicU16,
}

/// A partial change to one client's mix: every absent field keeps its current value.
///
/// It is the engine's own form of a `mix` control message, so a change from the phone and a change
/// through [crate::EngineHandle::set_mix] are merged by the same code. A list replaces the values
/// from the first channel (or group) on and leaves the rest alone; levels are clamped to 0-100 and
/// a list longer than the table is rejected, exactly as for a phone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MixUpdate {
    pub channel_gains: Option<Vec<i32>>,
    pub pans: Option<Vec<i32>>,
    pub channel_muted: Option<Vec<bool>>,
    pub channel_solo: Option<Vec<bool>>,
    pub group_levels: Option<Vec<i32>>,
    pub volume_percent: Option<i32>,
    pub max_level_percent: Option<i32>,
    pub muted: Option<bool>,
}

impl MixUpdate {
    /// Merges this update into `current`, or names the list that cannot be applied.
    ///
    /// The list labels are the `mix` message's field names, so a phone's error text is unchanged.
    pub(crate) fn merge_into(&self, current: MixValues) -> Result<MixValues, String> {
        Ok(MixValues {
            channel_gains: merge_levels(
                "channels",
                self.channel_gains.as_ref(),
                &current.channel_gains,
            )?,
            pans: merge_levels("pans", self.pans.as_ref(), &current.pans)?,
            channel_muted: merge_flags(
                "mutes",
                self.channel_muted.as_ref(),
                &current.channel_muted,
            )?,
            channel_solo: merge_flags("solos", self.channel_solo.as_ref(), &current.channel_solo)?,
            group_levels: merge_levels(
                "group_levels",
                self.group_levels.as_ref(),
                &current.group_levels,
            )?,
            // Membership, hidden channels and the engineer's controls are server configuration,
            // never client input: preserve whatever the source snapshot carried.
            channel_group: current.channel_group,
            hidden_channels: current.hidden_channels,
            engineer: current.engineer,
            // The store that keeps the merge gives it its revision.
            revision: current.revision,
            volume_percent: self
                .volume_percent
                .map_or(current.volume_percent, clamp_percent),
            max_level_percent: self
                .max_level_percent
                .map_or(current.max_level_percent, clamp_percent),
            muted: self.muted.unwrap_or(current.muted),
        })
    }
}

fn clamp_percent(value: i32) -> u8 {
    value.clamp(0, 100) as u8
}

/// A plain, copyable snapshot of one client's mix.
///
/// This is the engine's public view of the mix: atomics become plain values, so a UI can hold,
/// compare and render it without touching the live state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixValues {
    pub channel_gains: [u8; MAX_MIX_CHANNELS],
    pub pans: [u8; MAX_MIX_CHANNELS],
    pub channel_muted: [bool; MAX_MIX_CHANNELS],
    pub channel_solo: [bool; MAX_MIX_CHANNELS],
    pub group_levels: [u8; MAX_GROUPS],
    pub channel_group: [Option<usize>; MAX_MIX_CHANNELS],
    /// The source channels the engineer hid, one bit per channel (bit `k` is channel `k`): a hidden
    /// channel is never heard and its solo counts for nothing. Server configuration, like
    /// `channel_group`: a change of it is not a change of the stored mix and takes no revision.
    pub hidden_channels: u32,
    /// What the engineer set for this musician: their limit and their locked channels. Engineer
    /// policy, which no `mix` can change; setting it is a change of the mix and takes a revision,
    /// so the phone is told.
    pub engineer: EngineerControls,
    pub volume_percent: u8,
    /// The phone's own level cap. The mixer's ceiling is the lower of it and the engineer's limit.
    pub max_level_percent: u8,
    pub muted: bool,
    /// Which stored version of the mix this is: every change stores a higher revision, so of two
    /// copies of a musician's mix the one with the higher revision is newer. 0 is no stored mix.
    pub revision: u64,
}

/// Default pan keeps the captured interleaved layout: even sources hard left, odd sources hard
/// right. With two sources and neutral gains this reproduces the pre-pan output exactly.
fn default_pan(index: usize) -> u8 {
    if index % 2 == 0 {
        0
    } else {
        100
    }
}

impl Default for MixState {
    fn default() -> Self {
        let revision = next_revision();
        Self {
            channel_gains: std::array::from_fn(|_| AtomicU8::new(100)),
            pans: std::array::from_fn(|index| AtomicU8::new(default_pan(index))),
            channel_muted: std::array::from_fn(|_| AtomicBool::new(false)),
            channel_solo: std::array::from_fn(|_| AtomicBool::new(false)),
            group_levels: std::array::from_fn(|_| AtomicU8::new(NEUTRAL_GROUP_LEVEL)),
            channel_group: std::array::from_fn(|_| AtomicU8::new(NO_GROUP)),
            hidden: AtomicU32::new(0),
            linked: AtomicU32::new(0),
            engineer_limit: AtomicU8::new(EngineerControls::default().limit_percent),
            locked: AtomicU32::new(EngineerControls::default().locked),
            volume_percent: AtomicU8::new(100),
            max_level_percent: AtomicU8::new(100),
            muted: AtomicBool::new(false),
            writer: Mutex::new(revision),
            published: AtomicU64::new(revision),
            following: Mutex::new(None),
            limiter: LimiterControl::default(),
            fx_returns: AtomicU16::new(0),
        }
    }
}

impl Default for MixValues {
    fn default() -> Self {
        Self {
            channel_gains: [100; MAX_MIX_CHANNELS],
            pans: std::array::from_fn(default_pan),
            channel_muted: [false; MAX_MIX_CHANNELS],
            channel_solo: [false; MAX_MIX_CHANNELS],
            group_levels: [NEUTRAL_GROUP_LEVEL; MAX_GROUPS],
            channel_group: [None; MAX_MIX_CHANNELS],
            hidden_channels: 0,
            engineer: EngineerControls::default(),
            volume_percent: 100,
            max_level_percent: 100,
            muted: false,
            revision: 0,
        }
    }
}

fn store_all<const N: usize>(slots: &[AtomicU8; N], values: &[u8; N]) {
    for (slot, value) in slots.iter().zip(values.iter().copied()) {
        slot.store(value, Ordering::Relaxed);
    }
}

fn store_all_flags(slots: &[AtomicBool; MAX_MIX_CHANNELS], values: &[bool; MAX_MIX_CHANNELS]) {
    for (slot, value) in slots.iter().zip(values.iter().copied()) {
        slot.store(value, Ordering::Relaxed);
    }
}

/// Both effect returns in the bits of one atomic: the reverb low, the delay high.
fn pack_fx_returns(levels: EffectLevels) -> u16 {
    u16::from(levels.reverb) | u16::from(levels.delay) << 8
}

/// The effect returns [pack_fx_returns] packed.
fn unpack_fx_returns(bits: u16) -> EffectLevels {
    let [reverb, delay] = bits.to_le_bytes();
    EffectLevels { reverb, delay }
}

impl MixState {
    /// The master limiter of this mix, for the engine thread's [MixPlan::mix_master].
    pub(crate) fn limiter(&self) -> &LimiterControl {
        &self.limiter
    }

    /// Sets this mix's master limiter (see [LimiterControl]) from the next block; answers the
    /// settings kept, held in range. Engineer-only: never from a phone.
    pub(crate) fn set_limiter(&self, settings: LimiterSettings) -> LimiterSettings {
        self.limiter.set(settings)
    }

    /// The master limiter's settings in force.
    pub(crate) fn limiter_settings(&self) -> LimiterSettings {
        self.limiter.settings()
    }

    /// This mix's effect return levels, for the engine thread and the status.
    pub(crate) fn fx_returns(&self) -> EffectLevels {
        unpack_fx_returns(self.fx_returns.load(Ordering::Relaxed))
    }

    /// Sets this mix's effect return levels (each held to 100%) from the next block, where they
    /// glide; answers what is kept. Engineer-only: never from a phone.
    pub(crate) fn set_fx_returns(&self, levels: EffectLevels) -> EffectLevels {
        let levels = levels.clamped();
        self.fx_returns
            .store(pack_fx_returns(levels), Ordering::Relaxed);
        levels
    }

    /// Sets the return of `effect` alone to `level` percent (held to 100), keeping the other's
    /// as it is at that moment, as one atomic step: two changes at once, one to each effect, both
    /// land. Answers both returns now kept.
    pub(crate) fn set_fx_return(&self, effect: Effect, level: u8) -> EffectLevels {
        let update = |bits| Some(pack_fx_returns(unpack_fx_returns(bits).with(effect, level)));
        let before = self
            .fx_returns
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, update)
            .unwrap_or_else(|bits| bits);
        unpack_fx_returns(before).with(effect, level)
    }

    /// Builds a state that carries the given group membership. Each client's group levels start
    /// neutral, so a client that never sends `group_levels` hears every channel ungrouped.
    pub(crate) fn new(layout: &GroupLayout) -> Self {
        let state = Self::default();
        state.set_group_layout(layout);
        state
    }

    /// Builds a state for a musician who joins with `flags` in force: their hidden channels are
    /// silent and each linked pair starts panned apart (the first channel hard left, the second
    /// hard right), as every mix already there was panned when the pair was linked. Every other
    /// value starts neutral, so both sides of a pair start equal.
    pub(crate) fn configured(layout: &GroupLayout, flags: ChannelFlags) -> Self {
        let state = Self::new(layout);
        state.hidden.store(flags.hidden, Ordering::Relaxed);
        state.linked.store(flags.linked, Ordering::Relaxed);
        for first in pairs(flags.linked) {
            state.pans[first].store(0, Ordering::Relaxed);
            state.pans[first + 1].store(100, Ordering::Relaxed);
        }
        state
    }

    /// Takes the engineer's channel flags: the hidden channels go silent at once and every writer
    /// from now on keeps both sides of each linked pair equal. Returns the mix now stored when it
    /// changed, `None` when no stored value moved.
    ///
    /// Hiding or showing a channel and unlinking a pair change no stored value. Linking a pair is a
    /// change of the mix, made under the writer lock like [MixState::apply] and with a new
    /// revision, so the musician's phone is sent it: the pair is panned apart (first channel 0,
    /// second 100) and its second side takes the first side's level, mute and solo.
    ///
    /// A new pair with either side locked by the engineer is locked whole (see
    /// [EngineerControls::locked]), so the phone cannot move the side that was free.
    pub(crate) fn set_channel_flags(&self, flags: ChannelFlags) -> Option<MixValues> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let newly_linked = flags.linked & !self.linked.load(Ordering::Relaxed);
        self.hidden.store(flags.hidden, Ordering::Relaxed);
        self.linked.store(flags.linked, Ordering::Relaxed);
        if newly_linked == 0 {
            return None;
        }
        self.locked.store(
            lock_pairs(self.locked.load(Ordering::Relaxed), newly_linked),
            Ordering::Relaxed,
        );
        let mut pans = self.snapshot().pans;
        for first in pairs(newly_linked) {
            pans[first] = 0;
            pans[first + 1] = 100;
        }
        // Through the writers' own merge, which also gives each new pair's second side the first
        // side's level, mute and solo (see [keep_pairs_equal]).
        let update = MixUpdate {
            pans: Some(pans.iter().copied().map(i32::from).collect()),
            ..MixUpdate::default()
        };
        Some(
            self.apply_locked(&update, &mut revision, Writer::Desk)
                .expect("a whole table of pans always fits the mix"),
        )
    }

    /// Replaces the channel-to-group membership the mixer reads, leaving the client's stored group
    /// levels untouched.
    ///
    /// A device switch changes which channels exist, so a group can become invalid. An invalid group
    /// maps no channel and therefore cannot affect audio, while its level is preserved for the
    /// switch back. The update is lock-free: the network thread only ever loads complete slots.
    pub(crate) fn set_group_layout(&self, layout: &GroupLayout) {
        for (slot, group) in self.channel_group.iter().zip(layout.channel_group().iter()) {
            slot.store(
                match group {
                    Some(index) => *index as u8,
                    None => NO_GROUP,
                },
                Ordering::Relaxed,
            );
        }
    }

    /// Replaces the whole stored mix. Tests use it to set a mix up in one step; the server's
    /// writers (the phone's `mix` and the desktop's `set_mix`) all go through [MixState::apply].
    ///
    /// It takes the writer lock like [MixState::apply], so a whole-mix store can never land between
    /// a partial update's read and its store and undo part of that update.
    #[cfg(test)]
    pub(crate) fn update(&self, values: MixValues) {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        self.store(values, &mut revision);
    }

    /// Stores every client-settable field under a new revision, which it returns. The caller holds
    /// the writer lock and passes its revision; the group membership is server configuration and
    /// is left alone.
    fn store(&self, values: MixValues, revision: &mut u64) -> u64 {
        store_all(&self.channel_gains, &values.channel_gains);
        store_all(&self.pans, &values.pans);
        store_all_flags(&self.channel_muted, &values.channel_muted);
        store_all_flags(&self.channel_solo, &values.channel_solo);
        store_all(&self.group_levels, &values.group_levels);
        self.volume_percent
            .store(values.volume_percent, Ordering::Relaxed);
        self.max_level_percent
            .store(values.max_level_percent, Ordering::Relaxed);
        self.muted.store(values.muted, Ordering::Relaxed);
        self.publish_revision(revision)
    }

    /// Gives what was just stored a new revision, which it returns. The caller holds the writer
    /// lock and passes its revision.
    fn publish_revision(&self, revision: &mut u64) -> u64 {
        *revision = next_revision();
        self.published.store(*revision, Ordering::Release);
        *revision
    }

    /// Applies a partial update from the desk and returns the mix that is now stored, with its new
    /// revision. The desk may change every value, locked channels included; a phone's `mix` goes
    /// through [MixState::apply_from_phone].
    ///
    /// The read, merge and store happen under the writer lock, so the phone and the desktop can
    /// both change different parts of the same mix without one undoing the other: the last writer
    /// wins for the fields it carries, and nothing else. An update that cannot be applied leaves
    /// the mix exactly as it was, revision included.
    pub(crate) fn apply(&self, update: &MixUpdate) -> Result<MixValues, String> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        self.apply_locked(update, &mut revision, Writer::Desk)
    }

    /// Applies a `mix` from the musician's phone as [MixState::apply] does, except that the level,
    /// pan, mute and solo of every channel the engineer locked keep their stored values: the phone
    /// sends its whole mix, and everything else in it applies. While the mix follows another
    /// musician's ([MixState::set_following]) every channel value and group level keeps its stored
    /// value too, and only the master volume, the phone's level cap and the master mute apply. The
    /// mix returned, which the phone is acknowledged with, carries the kept values.
    ///
    /// The connection uses [MixState::apply_from_phone_report], which also answers the hold; the
    /// tests use this when the hold does not matter.
    #[cfg(test)]
    pub(crate) fn apply_from_phone(&self, update: &MixUpdate) -> Result<MixValues, String> {
        self.apply_from_phone_report(update)
            .map(|(values, _)| values)
    }

    /// Applies a `mix` from the musician's phone (see [MixState::apply_from_phone]), answered with
    /// the name of the musician the mix followed while it was applied, read under the same writer
    /// lock: what the phone's `mix_ack` carries, so it never names a hold other than the one its
    /// values were kept under.
    pub(crate) fn apply_from_phone_report(
        &self,
        update: &MixUpdate,
    ) -> Result<(MixValues, Option<Arc<str>>), String> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let values = self.apply_locked(update, &mut revision, Writer::Phone)?;
        Ok((values, self.following()))
    }

    /// Puts `preset` on this mix from the desk, as [MixState::apply] does: the preset's channels
    /// and the levels of the live groups it names (by id, see [MixPreset]), locked channels
    /// included, with both sides of a pair kept equal (the first side wins when the preset holds
    /// them apart). The master volume, the phone's level cap, the master mute and the engineer's
    /// controls keep their values. Returns the mix now stored, under a new revision.
    pub(crate) fn apply_preset(&self, preset: &MixPreset, layout: &GroupLayout) -> MixValues {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let update = preset.update_for(
            &self.snapshot(),
            layout,
            self.linked.load(Ordering::Relaxed),
        );
        self.apply_locked(&update, &mut revision, Writer::Desk)
            .expect("a preset never holds more than the mix")
    }

    /// Makes this mix sound like `leader`'s, from the desk, as a copy and a mirror do: every channel
    /// value and group level in place (both mixes are placed for the same groups), locked channels
    /// included. The master volume, the phone's level cap, the master mute and the engineer's
    /// controls keep their values. Returns the mix now stored, under a new revision, or `None` when
    /// it already held the leader's values, and then nothing changes.
    pub(crate) fn follow(&self, leader: &MixValues) -> Option<MixValues> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        if same_shared(&self.snapshot(), leader) {
            return None;
        }
        Some(
            self.apply_locked(&shared_update(leader), &mut revision, Writer::Desk)
                .expect("whole tables always fit the mix"),
        )
    }

    /// Starts or stops this mix following another musician's: `name` is the leader the musician's
    /// phone is told it follows, `None` to stop. While it follows, the phone's `mix` changes no
    /// channel and no group level (see [MixState::apply_from_phone]). No value of the mix moves:
    /// starting takes the leader's values through [MixState::follow], and stopping leaves the mix
    /// as it is. Returns the mix now stored, under a new revision so the phone is told, or `None`
    /// when the mix already followed under that name.
    pub(crate) fn set_following(&self, name: Option<Arc<str>>) -> Option<MixValues> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        {
            let mut following = self.following.lock().expect("mix following lock poisoned");
            if *following == name {
                return None;
            }
            *following = name;
        }
        self.publish_revision(&mut revision);
        Some(MixValues {
            revision: *revision,
            ..self.snapshot()
        })
    }

    /// The name of the musician whose mix this one follows, or `None`.
    pub(crate) fn following(&self) -> Option<Arc<str>> {
        self.following
            .lock()
            .expect("mix following lock poisoned")
            .clone()
    }

    /// The stored mix with exactly its revision and the name of the musician it follows, read
    /// together under the writer lock: what a report to the phone carries.
    pub(crate) fn report(&self) -> (MixValues, Option<Arc<str>>) {
        let revision = self.writer.lock().expect("mix writer lock poisoned");
        (
            MixValues {
                revision: *revision,
                ..self.snapshot()
            },
            self.following(),
        )
    }

    /// [MixState::apply] for a caller that holds the writer lock and passes its revision.
    ///
    /// A phone's change to a locked channel is undone first (see [keep_locked]), and while the mix
    /// follows another musician's every channel and group change of the phone is (see
    /// [keep_followed]); then every linked pair is left with both sides equal (see
    /// [keep_pairs_equal]). A pair is always locked whole, so the rules never pull a pair apart.
    /// The pairs and the locks are read under the lock, so a writer never merges against pairs or
    /// locks an edit is replacing.
    fn apply_locked(
        &self,
        update: &MixUpdate,
        revision: &mut u64,
        writer: Writer,
    ) -> Result<MixValues, String> {
        let current = self.snapshot();
        let mut merged = update.merge_into(current)?;
        if writer == Writer::Phone {
            if self.following().is_some() {
                keep_followed(&current, &mut merged);
            }
            keep_locked(&current, &mut merged);
        }
        keep_pairs_equal(&current, &mut merged, self.linked.load(Ordering::Relaxed));
        merged.revision = self.store(merged, revision);
        Ok(merged)
    }

    /// The engineer's controls in force.
    fn engineer(&self) -> EngineerControls {
        EngineerControls {
            limit_percent: self.engineer_limit.load(Ordering::Relaxed),
            locked: self.locked.load(Ordering::Relaxed),
        }
    }

    /// Stores the engineer's controls; the caller holds the writer lock and then publishes a
    /// revision.
    fn store_engineer(&self, controls: EngineerControls) {
        self.engineer_limit
            .store(controls.limit_percent, Ordering::Relaxed);
        self.locked.store(controls.locked, Ordering::Relaxed);
    }

    /// Changes the engineer's controls for this musician, as the desk does, and returns the mix now
    /// stored. It is a change of the mix under the writer lock with a new revision, like
    /// [MixState::apply], so the phone is sent the new limit and locks; no value of the mix moves.
    ///
    /// Both sides of a pair are locked or unlocked together (see [EngineerUpdate]). A list longer
    /// than the mix is an error naming it, and nothing changes.
    pub(crate) fn apply_engineer(&self, update: &EngineerUpdate) -> Result<MixValues, String> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let controls = update.merge_into(self.engineer(), self.linked.load(Ordering::Relaxed))?;
        self.store_engineer(controls);
        self.publish_revision(&mut revision);
        Ok(MixValues {
            revision: *revision,
            ..self.snapshot()
        })
    }

    /// Puts the engineer's controls kept for this musician in force, as a registration or the
    /// desk's saved session does: a pair with either side locked is locked whole, and a limit above
    /// 100 reads as 100. Returns the mix now stored when they changed, under a new revision like
    /// [MixState::apply_engineer]; `None` when they were in force already.
    pub(crate) fn restore_engineer(&self, controls: EngineerControls) -> Option<MixValues> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let controls = EngineerControls {
            limit_percent: controls.limit_percent.min(100),
            locked: lock_pairs(controls.locked, self.linked.load(Ordering::Relaxed)),
        };
        if controls == self.engineer() {
            return None;
        }
        self.store_engineer(controls);
        self.publish_revision(&mut revision);
        Some(MixValues {
            revision: *revision,
            ..self.snapshot()
        })
    }

    /// Moves this mix onto an edited set of groups: switches the membership the mixer reads to
    /// `layout` and moves the group levels by `sources` (see [regrouped_levels]). Returns the mix
    /// now stored.
    ///
    /// It is one change under the writer lock, like [MixState::apply]: it takes a new revision, so
    /// the musician's phone is sent the moved levels, and a change from the phone or the desktop
    /// cannot land between the read of the old levels and the store of the moved ones.
    pub(crate) fn regroup(&self, layout: &GroupLayout, sources: &GroupSources) -> MixValues {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        self.set_group_layout(layout);
        let mut values = self.snapshot();
        values.group_levels = regrouped_levels(&values.group_levels, sources);
        values.revision = self.store(values, &mut revision);
        values
    }

    /// The stored mix with exactly its revision, read under the writer lock. For a reader that
    /// compares revisions (a window, a status); the audio path uses the lock-free
    /// [MixState::snapshot].
    pub(crate) fn current(&self) -> MixValues {
        let revision = self.writer.lock().expect("mix writer lock poisoned");
        MixValues {
            revision: *revision,
            ..self.snapshot()
        }
    }

    /// The stored mix, read without a lock for the audio path. Its revision may lag the values
    /// while a store is in progress; use [MixState::current] when the revision must match.
    pub(crate) fn snapshot(&self) -> MixValues {
        MixValues {
            channel_gains: std::array::from_fn(|index| {
                self.channel_gains[index].load(Ordering::Relaxed)
            }),
            pans: std::array::from_fn(|index| self.pans[index].load(Ordering::Relaxed)),
            channel_muted: std::array::from_fn(|index| {
                self.channel_muted[index].load(Ordering::Relaxed)
            }),
            channel_solo: std::array::from_fn(|index| {
                self.channel_solo[index].load(Ordering::Relaxed)
            }),
            group_levels: std::array::from_fn(|index| {
                self.group_levels[index].load(Ordering::Relaxed)
            }),
            channel_group: std::array::from_fn(|index| {
                match self.channel_group[index].load(Ordering::Relaxed) {
                    NO_GROUP => None,
                    group => Some(usize::from(group)),
                }
            }),
            hidden_channels: self.hidden.load(Ordering::Relaxed),
            engineer: self.engineer(),
            volume_percent: self.volume_percent.load(Ordering::Relaxed),
            max_level_percent: self.max_level_percent.load(Ordering::Relaxed),
            muted: self.muted.load(Ordering::Relaxed),
            revision: self.published.load(Ordering::Acquire),
        }
    }
}

/// Merges an optional list of 0-100 values into an existing table.
///
/// An absent list preserves the current values, so a client that does not send the field keeps
/// working. Values are clamped; an oversized list is rejected rather than silently truncated.
pub(crate) fn merge_levels<const N: usize>(
    label: &str,
    incoming: Option<&Vec<i32>>,
    current: &[u8; N],
) -> Result<[u8; N], String> {
    let Some(values) = incoming else {
        return Ok(*current);
    };
    if values.len() > N {
        return Err(format!(
            "{label} accepts at most {N} values, received {}",
            values.len()
        ));
    }

    let mut merged = *current;
    for (slot, value) in merged.iter_mut().zip(values.iter()) {
        *slot = (*value).clamp(0, 100) as u8;
    }
    Ok(merged)
}

/// Merges an optional list of booleans into an existing table.
///
/// Separate from `merge_levels` on purpose: the 0-100 clamp that is right for gains and pans would
/// be meaningless here, and silently reusing it would hide a protocol mistake.
pub(crate) fn merge_flags(
    label: &str,
    incoming: Option<&Vec<bool>>,
    current: &[bool; MAX_MIX_CHANNELS],
) -> Result<[bool; MAX_MIX_CHANNELS], String> {
    let Some(values) = incoming else {
        return Ok(*current);
    };
    if values.len() > MAX_MIX_CHANNELS {
        return Err(format!(
            "{label} accepts at most {MAX_MIX_CHANNELS} flags, received {}",
            values.len()
        ));
    }

    let mut merged = *current;
    for (slot, value) in merged.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    Ok(merged)
}

/// Keeps both sides of every pair in `linked` (see [ChannelFlags::linked]) at the same level, mute
/// and solo after a merge from `before` to `merged`.
///
/// A side the update changed carries the other with it: a phone that shows the pair as one strip
/// writes both sides, and one that shows them apart moves one. When the update changed both sides
/// to different values, or the second side alone was not what changed, the first channel's value
/// wins, which also evens out a pair that was just linked. Pans are left alone: a pair's balance is
/// the client's, which writes both pans.
pub(crate) fn keep_pairs_equal(before: &MixValues, merged: &mut MixValues, linked: u32) {
    for first in pairs(linked) {
        follow(&before.channel_gains, &mut merged.channel_gains, first);
        follow(&before.channel_muted, &mut merged.channel_muted, first);
        follow(&before.channel_solo, &mut merged.channel_solo, first);
    }
}

/// Puts back the stored level, pan, mute and solo of every channel `before` has locked
/// ([EngineerControls::locked]) after a phone's merge from `before` to `merged`: the phone's change
/// to them is ignored, and everything else it sent stands.
pub(crate) fn keep_locked(before: &MixValues, merged: &mut MixValues) {
    for channel in (0..MAX_MIX_CHANNELS).filter(|&channel| before.engineer.is_locked(channel)) {
        merged.channel_gains[channel] = before.channel_gains[channel];
        merged.pans[channel] = before.pans[channel];
        merged.channel_muted[channel] = before.channel_muted[channel];
        merged.channel_solo[channel] = before.channel_solo[channel];
    }
}

/// Puts back every channel's level, pan, mute and solo and every group level after a phone's merge
/// from `before` to `merged`, for a musician whose mix follows another musician's: only the master
/// volume, the phone's level cap and the master mute are theirs to change.
pub(crate) fn keep_followed(before: &MixValues, merged: &mut MixValues) {
    merged.channel_gains = before.channel_gains;
    merged.pans = before.pans;
    merged.channel_muted = before.channel_muted;
    merged.channel_solo = before.channel_solo;
    merged.group_levels = before.group_levels;
}

/// Makes the pair at `first` equal in one table: the second side's new value when only the second
/// side changed, the first side's otherwise.
fn follow<T: Copy + PartialEq>(before: &[T], merged: &mut [T], first: usize) {
    let second = first + 1;
    if merged[second] != before[second] && merged[first] == before[first] {
        merged[first] = merged[second];
    } else {
        merged[second] = merged[first];
    }
}

/// Equal-power pan law, the console standard: `theta` runs from 0 at hard left to `PI/2` at
/// hard right, so a centred source contributes about `0.707` to each output.
fn pan_gains(pan: u8) -> (f32, f32) {
    let theta = (f32::from(pan) / 100.0) * std::f32::consts::FRAC_PI_2;
    (theta.cos(), theta.sin())
}

/// One heard source channel of a [MixPlan]: where it is in the frame, its gain and its pan.
#[derive(Clone, Copy, Default)]
struct Voice {
    channel: usize,
    gain: f32,
    left: f32,
    right: f32,
}

/// One client's mix of one block, worked out once from a snapshot of their [MixValues]: the
/// channels they hear, each with its gain and pan weights, and the output ceiling. Lives on the
/// stack, so building one per musician per block costs no allocation.
///
/// Every source channel contributes to both outputs weighted by its pan, so source channel `k`
/// is no longer bound to output slot `k`. The default pan table keeps the captured layout (even
/// sources hard left, odd sources hard right), which with neutral gains on a two-channel source
/// reproduces the captured output exactly, apart from the full-scale ceiling clamp. Master volume
/// and mute are folded into each gain; the ceiling is applied after the sum. The ceiling is the
/// lower of the phone's level cap (`max_level_percent`) and the engineer's limit
/// ([EngineerControls::limit_percent]), both read from `values`.
///
/// A channel contributes only when it is not hidden, not muted and either nothing is soloed or it
/// is one of the soloed channels. Solo is applied here rather than by zeroing gains, so coming out
/// of solo restores exactly the levels the musician had set. A hidden channel's solo counts for
/// nothing: a spare input the engineer hid cannot leave a musician hearing nothing. Like the rest of
/// the mix, the hidden channels are one value read from `values`, so this stays free of locks.
///
/// The arithmetic is the 16-bit mixer's, operation for operation (the same gain expression, the
/// same `sample * gain * weight` order, the same channel order), on samples scaled by 2^-15. A
/// power-of-two scale is exact in floating point, so for 16-bit capture every output is exactly
/// the 16-bit mixer's divided by 32768, and the phones receive the same packets.
///
/// That holds for every block whose mix is the one of the block before. A block where something
/// moved is mixed by [MixPlan::mix_ramped] instead, which glides from the previous block's weights
/// to these across the block so that a fader move does not click: see [MixRamp].
pub(crate) struct MixPlan {
    voices: [Voice; MAX_MIX_CHANNELS],
    count: usize,
    channels: usize,
    ceiling: f32,
    /// Every source channel's weights in this mix, heard or not (a channel not heard has a gain
    /// of 0), for [MixPlan::mix_ramped] to compare with the previous block's.
    weights: [Weights; MAX_MIX_CHANNELS],
    active: usize,
    /// What the effects' returns are scaled by: the master volume, 0 under the master mute.
    returns_gain: f32,
}

/// What one source channel is weighted with in a mix: its gain, 0 when it is not heard (muted,
/// hidden, soloed out, at 0 or under the master mute), and its pan weights.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Weights {
    gain: f32,
    left: f32,
    right: f32,
}

/// One channel of a block being ramped: its weights in the previous block and how far they move
/// to this block's.
#[derive(Clone, Copy, Default)]
struct Moving {
    channel: usize,
    from: Weights,
    step: Weights,
}

/// What one mix weighted its channels with in the previous block, so that the next block can
/// glide from there: one per mix that is mixed every block (each musician's, the local output's
/// and the recorder's local mix each have their own, since each is mixed on its own).
///
/// When a channel's gain or pan, the master volume, a mute, a solo or a hidden channel changed
/// since the previous block, [MixPlan::mix_ramped] moves each changed channel's weights linearly
/// across the block, from the previous block's at the start to the new ones at its last frame
/// (frame `i` of `n` is at `(i + 1) / n` of the way). A channel that falls silent fades to 0 on its
/// last pan; one that comes in fades up from 0 on its new pan. When nothing changed, the block is
/// mixed by [MixPlan::mix_into], bit for bit as without ramps, so the golden contract with the
/// 16-bit mixer holds for every steady block and only a block where a gain moved differs, on
/// purpose. The ceiling clamp is never ramped: it applies to every frame as before.
///
/// Before the first block, and when the source's channel count changes, there is nothing to glide
/// from: that block is mixed as it is, unramped.
///
/// It also keeps the running state of the mix's master limiter ([MixPlan::mix_master]): how far
/// it turns the mix down after the previous block, and the gain reduction to report. The limiter's
/// settings belong to the mix ([LimiterControl], on its [MixState]); its state belongs to whoever
/// mixes it, so the local output and the recorder's local mix, which mix the same local mix, each
/// limit with their own and never share it.
///
/// Only the engine thread uses it once it is made; its atomics are never contended. They let a
/// target entry share it with that thread without a lock, as [crate::redundancy::HeldCopies] is
/// shared, and let the meters read the gain reduction. Made off the engine thread, sized for
/// [MAX_MIX_CHANNELS]; mixing never allocates.
pub(crate) struct MixRamp {
    /// The channel count of the previous block's source; 0 before the first block.
    channels: AtomicUsize,
    /// Each channel's gain, left and right weights in the previous block, as `f32` bits.
    weights: [[AtomicU32; 3]; MAX_MIX_CHANNELS],
    /// The master limiter's attenuation after the previous block (`f32` bits; see
    /// [crate::dsp::dynamics::Limiter::attenuation]).
    limiter_attenuation: AtomicU32,
    /// Whether the master limiter still touches the samples (on, or releasing after being turned
    /// off).
    limiter_active: AtomicBool,
    /// How far the master limiter turned the last block down, in dB as `f32` bits (0: not at all).
    limiter_reduction_db: AtomicU32,
    /// Each effect's return gain in the previous block (`f32` bits; NaN before the first, so a
    /// first block takes its gain as it is), for [MixPlan::mix_master] to glide from.
    returns: [AtomicU32; 2],
}

impl Default for MixRamp {
    fn default() -> Self {
        Self {
            channels: AtomicUsize::new(0),
            weights: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            limiter_attenuation: AtomicU32::new(0),
            limiter_active: AtomicBool::new(false),
            limiter_reduction_db: AtomicU32::new(0),
            returns: std::array::from_fn(|_| AtomicU32::new(f32::NAN.to_bits())),
        }
    }
}

impl MixRamp {
    /// Forgets the previous block: the next one is mixed unramped, as a first block is, and the
    /// master limiter starts again from no reduction.
    pub(crate) fn reset(&self) {
        self.channels.store(0, Ordering::Relaxed);
        self.keep_limiter(0.0, false, 0.0);
        for value in &self.returns {
            value.store(f32::NAN.to_bits(), Ordering::Relaxed);
        }
    }

    /// How far the master limiter turned the last block of this mix down, in dB (0: not at all,
    /// or off). A read for the meters, from any thread.
    pub(crate) fn limiter_reduction_db(&self) -> f32 {
        f32::from_bits(self.limiter_reduction_db.load(Ordering::Relaxed))
    }

    /// The returns' gains of the previous block, keeping `to` for the next; a gain not known yet
    /// is taken as `to`.
    fn take_returns(&self, to: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|effect| {
            let from =
                f32::from_bits(self.returns[effect].swap(to[effect].to_bits(), Ordering::Relaxed));
            if from.is_nan() {
                to[effect]
            } else {
                from
            }
        })
    }

    fn keep_limiter(&self, attenuation: f32, active: bool, reduction_db: f32) {
        self.limiter_attenuation
            .store(attenuation.to_bits(), Ordering::Relaxed);
        self.limiter_active.store(active, Ordering::Relaxed);
        self.limiter_reduction_db
            .store(reduction_db.to_bits(), Ordering::Relaxed);
    }

    fn load(&self, channel: usize) -> Weights {
        let [gain, left, right] = &self.weights[channel];
        let read = |value: &AtomicU32| f32::from_bits(value.load(Ordering::Relaxed));
        Weights {
            gain: read(gain),
            left: read(left),
            right: read(right),
        }
    }

    fn store(&self, channel: usize, weights: Weights) {
        let [gain, left, right] = &self.weights[channel];
        gain.store(weights.gain.to_bits(), Ordering::Relaxed);
        left.store(weights.left.to_bits(), Ordering::Relaxed);
        right.store(weights.right.to_bits(), Ordering::Relaxed);
    }
}

/// A mix's master limiter, as the engineer set it: engineer-only, like [EngineerControls], so no
/// phone's `mix` can change it and it is not part of [MixValues] or any `mix` message.
///
/// **Where it sits.** After the mix's sum and before the clamp at the mix's ceiling (the lower of
/// the phone's level cap and the engineer's limit, see [MixPlan]), which stays as the last safety
/// net. Peak, with an instant attack and no lookahead: it adds no latency.
///
/// **Its ceiling.** The lower of its own ceiling (-24 to 0 dBFS) and the mix's ceiling. At its
/// default of 0 dBFS ([LimiterSettings::master]) it limits right at the mix's ceiling, so the clamp
/// never cuts a peak while it is on; a lower ceiling limits below it. Turning the engineer's limit
/// or the phone's cap down lowers it too.
///
/// **Off.** Off is the default and leaves the mix bit for bit as without a limiter; turned off
/// while it works, it releases what it still turns down (a 20 ms time constant) and then stops
/// touching the mix.
///
/// The settings are written off the engine thread with the design worked out there (the linear
/// ceiling and the release coefficient), and read by the engine thread with plain atomic loads:
/// no lock, no allocation and no logarithm on the audio path. A change that lands while a block
/// reads it may pair the new switch with the old numbers for that one block, which is harmless.
pub(crate) struct LimiterControl {
    enabled: AtomicBool,
    ceiling_db: AtomicU32,
    release_ms: AtomicU32,
    /// The design the engine thread runs: limiting, linear ceiling, release coefficient.
    design_enabled: AtomicBool,
    design_ceiling: AtomicU32,
    design_release: AtomicU32,
}

impl Default for LimiterControl {
    fn default() -> Self {
        let control = Self {
            enabled: AtomicBool::new(false),
            ceiling_db: AtomicU32::new(0),
            release_ms: AtomicU32::new(0),
            design_enabled: AtomicBool::new(false),
            design_ceiling: AtomicU32::new(0),
            design_release: AtomicU32::new(0),
        };
        control.set(LimiterSettings::master());
        control
    }
}

impl LimiterControl {
    /// Puts `settings`, held in range, in force from the next block; answers what was kept. Works
    /// out the design here, so call it off the engine thread.
    pub(crate) fn set(&self, settings: LimiterSettings) -> LimiterSettings {
        let kept = settings.clamped();
        let (enabled, ceiling, release) = kept.design().parts();
        self.ceiling_db
            .store(kept.ceiling_db.to_bits(), Ordering::Relaxed);
        self.release_ms
            .store(kept.release_ms.to_bits(), Ordering::Relaxed);
        self.enabled.store(kept.enabled, Ordering::Relaxed);
        self.design_ceiling
            .store(ceiling.to_bits(), Ordering::Relaxed);
        self.design_release
            .store(release.to_bits(), Ordering::Relaxed);
        self.design_enabled.store(enabled, Ordering::Release);
        kept
    }

    /// The settings in force.
    pub(crate) fn settings(&self) -> LimiterSettings {
        LimiterSettings {
            enabled: self.enabled.load(Ordering::Relaxed),
            ceiling_db: f32::from_bits(self.ceiling_db.load(Ordering::Relaxed)),
            release_ms: f32::from_bits(self.release_ms.load(Ordering::Relaxed)),
        }
    }

    /// The design the engine thread runs this block.
    fn design(&self) -> LimiterDesign {
        let enabled = self.design_enabled.load(Ordering::Acquire);
        LimiterDesign::from_parts(
            enabled,
            f32::from_bits(self.design_ceiling.load(Ordering::Relaxed)),
            f32::from_bits(self.design_release.load(Ordering::Relaxed)),
        )
    }
}

impl MixPlan {
    pub(crate) fn new(values: &MixValues, channels: usize) -> Self {
        let active = channels.min(MAX_MIX_CHANNELS);
        let audible = |channel: usize| values.hidden_channels & (1 << channel) == 0;
        let any_solo = (0..active).any(|channel| values.channel_solo[channel] && audible(channel));
        let ceiling_percent = values.max_level_percent.min(values.engineer.limit_percent);
        let ceiling = (i32::from(i16::MAX) * i32::from(ceiling_percent) / 100) as f32 / 32_768.0;
        let master = f32::from(values.volume_percent) / 100.0;
        let mut plan = Self {
            voices: [Voice::default(); MAX_MIX_CHANNELS],
            count: 0,
            channels: channels.max(1),
            ceiling,
            weights: [Weights::default(); MAX_MIX_CHANNELS],
            active,
            returns_gain: if values.muted { 0.0 } else { master },
        };

        for channel in 0..active {
            let (left, right) = pan_gains(values.pans[channel]);
            plan.weights[channel] = Weights {
                gain: 0.0,
                left,
                right,
            };
            if !audible(channel) || values.channel_muted[channel] {
                continue;
            }
            if any_solo && !values.channel_solo[channel] {
                continue;
            }
            // A grouped channel is scaled by its group level; an ungrouped channel uses 1.0, and
            // `x * 1.0 == x` for every f32, so the no-group case stays byte-identical to the
            // pre-group arithmetic.
            let group_gain = match values.channel_group[channel] {
                Some(group) => f32::from(values.group_levels[group]) / 100.0,
                None => 1.0,
            };
            let gain = f32::from(values.channel_gains[channel]) / 100.0 * group_gain * master;
            if gain == 0.0 || values.muted {
                continue;
            }
            plan.weights[channel].gain = gain;
            plan.voices[plan.count] = Voice {
                channel,
                gain,
                left,
                right,
            };
            plan.count += 1;
        }
        plan
    }

    /// Mixes the whole frames of `source` into `stereo` (interleaved left and right, at most as
    /// many frames as it holds) and returns the frames written. Allocation-free. The sinks mix
    /// through [Self::mix_master], which is this for a steady block with the limiter off; the
    /// tests compare against it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn mix_into(&self, source: &[f32], stereo: &mut [f32]) -> usize {
        self.mix_into_at(self.ceiling, source, stereo)
    }

    /// [Self::mix_into], clamped at `ceiling` (infinite: not clamped, for the master limiter).
    fn mix_into_at(&self, ceiling: f32, source: &[f32], stereo: &mut [f32]) -> usize {
        let voices = &self.voices[..self.count];
        let mut frames = 0;
        for (frame, output) in source
            .chunks_exact(self.channels)
            .zip(stereo.chunks_exact_mut(usize::from(OUTPUT_CHANNELS)))
        {
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in voices {
                let sample = frame[voice.channel];
                left += sample * voice.gain * voice.left;
                right += sample * voice.gain * voice.right;
            }
            output[0] = left.clamp(-ceiling, ceiling);
            output[1] = right.clamp(-ceiling, ceiling);
            frames += 1;
        }
        frames
    }

    /// Mixes like [Self::mix_into], gliding from the weights `ramp` kept of the previous block to
    /// this plan's wherever they differ (see [MixRamp]), and keeps this plan's in `ramp` for the
    /// next block. Allocation-free.
    pub(crate) fn mix_ramped(&self, ramp: &MixRamp, source: &[f32], stereo: &mut [f32]) -> usize {
        self.mix_ramped_at(self.ceiling, ramp, source, stereo)
    }

    /// [Self::mix_ramped], clamped at `ceiling` (infinite: not clamped, for the master limiter).
    fn mix_ramped_at(
        &self,
        ceiling: f32,
        ramp: &MixRamp,
        source: &[f32],
        stereo: &mut [f32],
    ) -> usize {
        let continued = ramp.channels.swap(self.channels, Ordering::Relaxed) == self.channels;
        let mut moving = [Moving::default(); MAX_MIX_CHANNELS];
        let mut count = 0;
        let mut changed = false;
        for channel in 0..self.active {
            let mut to = self.weights[channel];
            let mut from = if continued { ramp.load(channel) } else { to };
            ramp.store(channel, to);
            if from.gain == 0.0 && to.gain == 0.0 {
                continue;
            }
            // Only the level of a channel coming in or going out moves: it comes in on its new
            // pan and goes out on its last one.
            if from.gain == 0.0 {
                (from.left, from.right) = (to.left, to.right);
            }
            if to.gain == 0.0 {
                (to.left, to.right) = (from.left, from.right);
            }
            changed |= from != to;
            moving[count] = Moving {
                channel,
                from,
                step: Weights {
                    gain: to.gain - from.gain,
                    left: to.left - from.left,
                    right: to.right - from.right,
                },
            };
            count += 1;
        }
        if !changed {
            // The same voices with the same weights: the steady mix, bit for bit.
            return self.mix_into_at(ceiling, source, stereo);
        }

        let moving = &moving[..count];
        let frames =
            (source.len() / self.channels).min(stereo.len() / usize::from(OUTPUT_CHANNELS));
        for (index, (frame, output)) in source
            .chunks_exact(self.channels)
            .zip(stereo.chunks_exact_mut(usize::from(OUTPUT_CHANNELS)))
            .enumerate()
        {
            // At the last frame the way is covered: the block ends on the new weights.
            let way = (index + 1) as f32 / frames as f32;
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for voice in moving {
                let sample = frame[voice.channel];
                let gain = voice.from.gain + voice.step.gain * way;
                left += sample * gain * (voice.from.left + voice.step.left * way);
                right += sample * gain * (voice.from.right + voice.step.right * way);
            }
            output[0] = left.clamp(-ceiling, ceiling);
            output[1] = right.clamp(-ceiling, ceiling);
        }
        frames
    }

    /// Mixes like [Self::mix_ramped], through the mix's master limiter `limiter` when it is on or
    /// still releasing (see [LimiterControl]): the sum, unclamped, goes through the limiter, its
    /// ceiling held to this mix's, and then through the clamp as before. The limiter carries on
    /// from the state `ramp` kept and keeps its state and its gain reduction there. While the
    /// limiter is off and done, this is [Self::mix_ramped], bit for bit. Allocation-free.
    ///
    /// Before the limiter, each effect that ran this block (`wet`, see [crate::effects]) is added
    /// at the mix's return level `returns`, scaled by the master volume (nothing under the master
    /// mute); a return that changed glides across the block. An effect that did not run, or a
    /// return at 0 in this block and the one before, adds nothing, so with no effect returned and
    /// the limiter off the mix is [Self::mix_ramped], bit for bit.
    pub(crate) fn mix_master(
        &self,
        ramp: &MixRamp,
        limiter: &LimiterControl,
        returns: EffectLevels,
        wet: &Wet<'_>,
        source: &[f32],
        stereo: &mut [f32],
    ) -> usize {
        let design = limiter.design().capped(self.ceiling);
        let mut running = Limiter::resumed(
            usize::from(OUTPUT_CHANNELS),
            &design,
            f32::from_bits(ramp.limiter_attenuation.load(Ordering::Relaxed)),
            ramp.limiter_active.load(Ordering::Relaxed),
        );
        let to = Effect::ALL
            .map(|effect| f32::from(returns.get(effect).min(100)) / 100.0 * self.returns_gain);
        let from = ramp.take_returns(to);
        let returned = Effect::ALL.map(|effect| {
            let index = effect as usize;
            wet.get(effect)
                .filter(|_| from[index] != 0.0 || to[index] != 0.0)
        });
        let returning = returned.iter().any(Option::is_some);
        if !running.is_active() && !returning {
            ramp.keep_limiter(0.0, false, 0.0);
            return self.mix_ramped(ramp, source, stereo);
        }
        let frames = self.mix_ramped_at(f32::INFINITY, ramp, source, stereo);
        let mixed = &mut stereo[..frames * usize::from(OUTPUT_CHANNELS)];
        for (index, wet) in returned.iter().enumerate() {
            if let Some(wet) = wet {
                add_return(mixed, wet, from[index], to[index]);
            }
        }
        if !running.is_active() {
            for sample in mixed.iter_mut() {
                *sample = sample.clamp(-self.ceiling, self.ceiling);
            }
            ramp.keep_limiter(0.0, false, 0.0);
            return frames;
        }
        running.process(mixed, usize::from(OUTPUT_CHANNELS), 0);
        // The clamp stays the last word, though the limiter already held every sample under it.
        for sample in mixed.iter_mut() {
            *sample = sample.clamp(-self.ceiling, self.ceiling);
        }
        ramp.keep_limiter(
            running.attenuation(),
            running.is_active(),
            running.gain_reduction_db(),
        );
        frames
    }
}

/// Mixes a 16-bit buffer through [MixPlan] and back to 16 bits, as the phones receive it: the
/// shape the mixer's tests are written in.
#[cfg(test)]
pub(crate) fn mix_channels(source: &[i16], channels: usize, values: MixValues) -> Vec<i16> {
    use crate::pipeline::CaptureSample;

    if channels == 0 || source.len() < channels {
        return source.to_vec();
    }
    let floats: Vec<f32> = source.iter().map(|sample| sample.to_f32()).collect();
    let mut stereo = vec![0.0; source.len() / channels * usize::from(OUTPUT_CHANNELS)];
    MixPlan::new(&values, channels).mix_into(&floats, &mut stereo);
    stereo
        .iter()
        .map(|sample| crate::protocol::to_pcm16(*sample))
        .collect()
}

/// The 16-bit mixer the phones heard before the float path, kept verbatim as the reference the
/// float path must reproduce, and a deterministic source of inputs and mixes to compare them on.
#[cfg(test)]
pub(crate) mod golden {
    use super::*;

    fn clamp_sample(value: f32, ceiling: f32) -> i16 {
        value
            .clamp(-ceiling, ceiling)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
    }

    /// The previous `mix_channels`, unchanged: 16-bit in, 16-bit out, pan gains per frame.
    pub(crate) fn legacy_mix_channels(
        source: &[i16],
        channels: usize,
        values: MixValues,
    ) -> Vec<i16> {
        if channels == 0 || source.len() < channels {
            return source.to_vec();
        }

        let frames = source.len() / channels;
        let active = channels.min(MAX_MIX_CHANNELS);
        let audible = |channel: usize| values.hidden_channels & (1 << channel) == 0;
        let any_solo = (0..active).any(|channel| values.channel_solo[channel] && audible(channel));
        let ceiling_percent = values.max_level_percent.min(values.engineer.limit_percent);
        let ceiling = (i32::from(i16::MAX) * i32::from(ceiling_percent) / 100) as f32;
        let master = f32::from(values.volume_percent) / 100.0;
        let mut output = vec![0i16; frames * usize::from(OUTPUT_CHANNELS)];

        for frame in 0..frames {
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for channel in 0..active {
                if !audible(channel) || values.channel_muted[channel] {
                    continue;
                }
                if any_solo && !values.channel_solo[channel] {
                    continue;
                }
                let group_gain = match values.channel_group[channel] {
                    Some(group) => f32::from(values.group_levels[group]) / 100.0,
                    None => 1.0,
                };
                let gain = f32::from(values.channel_gains[channel]) / 100.0 * group_gain * master;
                if gain == 0.0 || values.muted {
                    continue;
                }
                let sample = f32::from(source[frame * channels + channel]);
                let (left_weight, right_weight) = pan_gains(values.pans[channel]);
                left += sample * gain * left_weight;
                right += sample * gain * right_weight;
            }
            output[frame * 2] = clamp_sample(left, ceiling);
            output[frame * 2 + 1] = clamp_sample(right, ceiling);
        }

        output
    }

    /// A xorshift stream: the same inputs and mixes on every run, with no dependency.
    pub(crate) struct Noise(pub(crate) u64);

    impl Noise {
        pub(crate) fn next(&mut self) -> u64 {
            let mut value = self.0;
            value ^= value << 13;
            value ^= value >> 7;
            value ^= value << 17;
            self.0 = value;
            value
        }

        pub(crate) fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }

        /// A sample: full-range noise, quiet noise or a full-scale extreme.
        pub(crate) fn sample(&mut self) -> i16 {
            match self.below(8) {
                0 => i16::MIN,
                1 => i16::MAX,
                2..=4 => (self.below(2_001) as i16) - 1_000,
                _ => self.next() as i16,
            }
        }

        /// A mix with every field drawn: gains, pans, mutes, solos, groups and their levels,
        /// hidden channels, master, both ceilings and the master mute.
        pub(crate) fn mix(&mut self) -> MixValues {
            let mut values = MixValues::default();
            for channel in 0..MAX_MIX_CHANNELS {
                values.channel_gains[channel] = self.below(101) as u8;
                values.pans[channel] = self.below(101) as u8;
                values.channel_muted[channel] = self.below(8) == 0;
                values.channel_solo[channel] = self.below(24) == 0;
                values.channel_group[channel] =
                    (self.below(3) == 0).then(|| self.below(MAX_GROUPS as u64) as usize);
            }
            for level in &mut values.group_levels {
                *level = self.below(101) as u8;
            }
            values.hidden_channels = (self.next() & self.next() & self.next()) as u32;
            values.volume_percent = self.below(101) as u8;
            values.max_level_percent = 40 + self.below(61) as u8;
            values.engineer.limit_percent = 40 + self.below(61) as u8;
            values.muted = self.below(32) == 0;
            values
        }
    }
}

#[cfg(test)]
mod tests {
    use super::golden::{legacy_mix_channels, Noise};
    use super::*;
    use crate::protocol::parse_mix_command;

    /// The float mixer reproduces the 16-bit mixer exactly on 16-bit input: thousands of drawn
    /// mixes (every field: groups, hidden channels, solos, mutes, both ceilings, the master mute)
    /// over drawn inputs with full-scale extremes, from 1 to 34 channels.
    #[test]
    fn the_float_mixer_matches_the_16_bit_mixer_on_16_bit_input() {
        let mut noise = Noise(0x2545_f491_4f6c_dd1d);
        for case in 0..3_000 {
            let channels = 1 + noise.below(34) as usize;
            let frames = 1 + noise.below(32) as usize;
            let values = noise.mix();
            let input: Vec<i16> = (0..channels * frames).map(|_| noise.sample()).collect();

            assert_eq!(
                mix_channels(&input, channels, values),
                legacy_mix_channels(&input, channels, values),
                "case {case}: {channels} channels"
            );
        }
    }

    /// Below the 16-bit step the float mix keeps what the 16-bit one rounded away: one 24-bit step
    /// on the left input comes out on the left output unchanged.
    #[test]
    fn the_float_mixer_keeps_24_bit_detail() {
        let step = 1.0 / 8_388_608.0;
        let mut stereo = [0.0; 2];

        let frames = MixPlan::new(&MixValues::default(), 2).mix_into(&[step, 0.0], &mut stereo);

        assert_eq!(frames, 1);
        assert_eq!(stereo[0], step);
        assert!(stereo[1].abs() < step);
    }

    /// One block of `source` (`channels` inputs) mixed with `values` after the blocks `ramp` saw.
    fn ramped(ramp: &MixRamp, values: &MixValues, source: &[f32], channels: usize) -> Vec<f32> {
        let mut stereo = vec![0.0; source.len() / channels * usize::from(OUTPUT_CHANNELS)];
        MixPlan::new(values, channels).mix_ramped(ramp, source, &mut stereo);
        stereo
    }

    /// The same block mixed with no ramp, as every steady block must be.
    fn unramped(values: &MixValues, source: &[f32], channels: usize) -> Vec<f32> {
        let mut stereo = vec![0.0; source.len() / channels * usize::from(OUTPUT_CHANNELS)];
        MixPlan::new(values, channels).mix_into(source, &mut stereo);
        stereo
    }

    fn side(stereo: &[f32], index: usize) -> Vec<f32> {
        stereo.iter().skip(index).step_by(2).copied().collect()
    }

    fn falls(samples: &[f32]) -> bool {
        samples.windows(2).all(|pair| pair[1] < pair[0])
    }

    fn rises(samples: &[f32]) -> bool {
        samples.windows(2).all(|pair| pair[1] > pair[0])
    }

    /// Two inputs at 0.5, the first hard left and the second hard right (the default pans).
    fn two_inputs(frames: usize) -> Vec<f32> {
        vec![0.5; frames * 2]
    }

    /// A fader pulled from 100 to 50 between two blocks glides across the next one: its first
    /// frame is a 64th of the way from the old gain, its last at the new gain, falling steadily.
    /// The channel nobody touched is mixed exactly as without a ramp, and the block after is
    /// steady again, bit for bit the unramped mix.
    #[test]
    fn a_fader_move_glides_across_the_next_block_then_holds() {
        let ramp = MixRamp::default();
        let source = two_inputs(64);
        let mut values = MixValues::default();
        assert_eq!(
            ramped(&ramp, &values, &source, 2),
            unramped(&values, &source, 2),
            "the first block has nothing to glide from"
        );

        values.channel_gains[0] = 50;
        let moved = ramped(&ramp, &values, &source, 2);

        let left = side(&moved, 0);
        assert!(
            (left[0] - 0.5 * (1.0 - 0.5 / 64.0)).abs() < 1e-6,
            "{}",
            left[0]
        );
        assert!((left[63] - 0.25).abs() < 1e-6, "{}", left[63]);
        assert!(falls(&left), "{left:?}");
        assert_eq!(side(&moved, 1), side(&unramped(&values, &source, 2), 1));
        assert_eq!(
            ramped(&ramp, &values, &source, 2),
            unramped(&values, &source, 2)
        );
    }

    /// A mute fades the channel out over one block, ending silent, and stays silent; unmuted, it
    /// fades back in from silence.
    #[test]
    fn a_mute_fades_out_and_back_in_instead_of_clicking() {
        let ramp = MixRamp::default();
        let source = two_inputs(32);
        let mut values = MixValues::default();
        ramped(&ramp, &values, &source, 2);

        values.channel_muted[0] = true;
        let out = side(&ramped(&ramp, &values, &source, 2), 0);
        assert!(
            out[0] > 0.45 && out[31].abs() < 1e-6 && falls(&out),
            "{out:?}"
        );
        let silent = ramped(&ramp, &values, &source, 2);
        assert_eq!(silent, unramped(&values, &source, 2));
        assert!(side(&silent, 0).iter().all(|sample| sample.abs() < 1e-6));

        values.channel_muted[0] = false;
        let back = side(&ramped(&ramp, &values, &source, 2), 0);
        assert!(
            back[0] < 0.05 && (back[31] - 0.5).abs() < 1e-6 && rises(&back),
            "{back:?}"
        );
    }

    /// Soloing another channel, hiding this one, the master volume and the master mute all take
    /// the channel out over a block, as a mute does.
    #[test]
    fn a_solo_a_hidden_channel_and_the_master_glide_too() {
        let changes: [fn(&mut MixValues); 4] = [
            |values| values.channel_solo[1] = true,
            |values| values.hidden_channels = 1,
            |values| values.volume_percent = 0,
            |values| values.muted = true,
        ];
        for (index, change) in changes.iter().enumerate() {
            let ramp = MixRamp::default();
            let source = two_inputs(32);
            let mut values = MixValues::default();
            ramped(&ramp, &values, &source, 2);

            change(&mut values);
            let out = side(&ramped(&ramp, &values, &source, 2), 0);

            assert!(
                out[0] > 0.45 && out[31].abs() < 1e-6 && falls(&out),
                "change {index}: {out:?}"
            );
        }
    }

    /// A pan move glides from one side to the other: the left falls as the right rises.
    #[test]
    fn a_pan_move_glides_between_the_sides() {
        let ramp = MixRamp::default();
        let source = vec![0.5, 0.0].repeat(48);
        let mut values = MixValues::default();
        ramped(&ramp, &values, &source, 2);

        values.pans[0] = 100;
        let moved = ramped(&ramp, &values, &source, 2);

        let (left, right) = (side(&moved, 0), side(&moved, 1));
        assert!(
            left[0] > 0.48 && left[47].abs() < 1e-6 && falls(&left),
            "{left:?}"
        );
        assert!(right[0] < 0.02 && (right[47] - 0.5).abs() < 1e-6 && rises(&right));
        assert_eq!(
            ramped(&ramp, &values, &source, 2),
            unramped(&values, &source, 2)
        );
    }

    /// The golden contract holds for every steady block: thousands of drawn mixes, each mixed
    /// over three blocks of drawn input, give exactly the unramped mixer's output (which is the
    /// 16-bit mixer's, see above) on every block once the mix stops moving; a mix that moved is
    /// exact again from the block after.
    #[test]
    fn a_steady_mix_is_the_unramped_mix_bit_for_bit() {
        let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
        for case in 0..1_000 {
            let channels = 1 + noise.below(34) as usize;
            let frames = 1 + noise.below(32) as usize;
            let ramp = MixRamp::default();
            let draw = |noise: &mut Noise| -> Vec<f32> {
                (0..channels * frames)
                    .map(|_| f32::from(noise.sample()) / 32_768.0)
                    .collect()
            };
            let values = noise.mix();
            for block in 0..3 {
                let input = draw(&mut noise);
                assert_eq!(
                    ramped(&ramp, &values, &input, channels),
                    unramped(&values, &input, channels),
                    "case {case}, block {block}"
                );
            }
            let moved = noise.mix();
            ramped(&ramp, &moved, &draw(&mut noise), channels);
            let input = draw(&mut noise);
            assert_eq!(
                ramped(&ramp, &moved, &input, channels),
                unramped(&moved, &input, channels),
                "case {case}, after the move"
            );
        }
    }

    /// With another channel count there is nothing to glide from, and after a reset neither:
    /// those blocks are mixed as they are.
    #[test]
    fn a_new_layout_or_a_reset_starts_unramped() {
        let ramp = MixRamp::default();
        let mut values = MixValues::default();
        ramped(&ramp, &values, &two_inputs(16), 2);

        values.channel_gains[0] = 40;
        let four = vec![0.5; 4 * 16];
        assert_eq!(
            ramped(&ramp, &values, &four, 4),
            unramped(&values, &four, 4)
        );

        ramp.reset();
        values.channel_gains[0] = 90;
        assert_eq!(
            ramped(&ramp, &values, &four, 4),
            unramped(&values, &four, 4)
        );
    }

    /// Ramping every block, on 32 inputs, allocates and frees nothing.
    #[test]
    fn ramping_never_allocates() {
        let ramp = MixRamp::default();
        let source = vec![0.25f32; 256 * 32];
        let mut stereo = vec![0.0f32; 256 * 2];
        let mut values = MixValues::default();

        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for block in 0..300 {
                values.channel_gains[block % 32] = (block % 101) as u8;
                values.pans[(block + 7) % 32] = (block * 3 % 101) as u8;
                values.channel_muted[(block + 3) % 32] = block % 2 == 0;
                MixPlan::new(&values, 32).mix_ramped(&ramp, &source, &mut stereo);
            }
        });

        assert_eq!((allocations, frees), (0, 0));
    }

    /// The effects' returns: nothing returned (no effect ran, or every return at 0) is the
    /// ramped mix bit for bit; a return adds the wet at its level times the master volume, before
    /// the clamp; the master mute silences it; a return that moves glides across the block.
    #[test]
    fn a_mix_returns_the_effects_at_its_level_and_the_master_and_nothing_else() {
        use golden::Noise;

        let limiter = LimiterControl::default();
        let mut noise = Noise(0x5eed_f00d_1234_5678);
        let (plain, unreturned, unran) =
            (MixRamp::default(), MixRamp::default(), MixRamp::default());
        let wet: Vec<f32> = (0..2 * 64)
            .map(|n| ((n * 37 % 101) as f32 - 50.0) / 400.0)
            .collect();
        for block in 0..300 {
            let values = noise.mix();
            let source: Vec<f32> = (0..4 * 64)
                .map(|_| f32::from(noise.sample()) / 32_768.0)
                .collect();
            let plan = MixPlan::new(&values, 4);
            let mut expected = vec![0.0; 128];
            plan.mix_ramped(&plain, &source, &mut expected);
            // The effects ran, but this mix returns nothing of them.
            let mut stereo = vec![0.0; 128];
            let ran = Wet::of(Some(&wet), Some(&wet));
            plan.mix_master(
                &unreturned,
                &limiter,
                EffectLevels::default(),
                &ran,
                &source,
                &mut stereo,
            );
            assert!(
                crate::dsp::tests::same_bits(&stereo, &expected),
                "returned, block {block}"
            );
            // This mix returns both, but neither effect ran.
            let levels = EffectLevels {
                reverb: 70,
                delay: 30,
            };
            plan.mix_master(&unran, &limiter, levels, &Wet::NONE, &source, &mut stereo);
            assert!(
                crate::dsp::tests::same_bits(&stereo, &expected),
                "ran, block {block}"
            );
        }

        let mut values = MixValues {
            volume_percent: 80,
            ..MixValues::default()
        };
        values.channel_gains = [0; MAX_MIX_CHANNELS];
        let silent = vec![0.0f32; 2 * 64];
        let levels = EffectLevels {
            reverb: 50,
            delay: 25,
        };
        let flat = vec![0.5f32; 2 * 64];
        let ramp = MixRamp::default();
        let mut stereo = vec![0.0; 128];
        MixPlan::new(&values, 2).mix_master(
            &ramp,
            &limiter,
            levels,
            &Wet::of(Some(&flat), Some(&flat)),
            &silent,
            &mut stereo,
        );
        // 0.5 × (50% + 25%) × 80% of the master, from the first block (nothing to glide from).
        assert!(
            stereo.iter().all(|s| (s - 0.3).abs() < 1e-6),
            "{:?}",
            &stereo[..4]
        );

        // The reverb's return moves to 100%: it glides there across the block.
        MixPlan::new(&values, 2).mix_master(
            &ramp,
            &limiter,
            EffectLevels {
                reverb: 100,
                delay: 25,
            },
            &Wet::of(Some(&flat), Some(&flat)),
            &silent,
            &mut stereo,
        );
        assert!(
            (stereo[0] - (0.3 + 0.2 / 64.0)).abs() < 1e-6,
            "{}",
            stereo[0]
        );
        assert!((stereo[126] - 0.5).abs() < 1e-6, "{}", stereo[126]);

        // Held at the mix's ceiling, as the dry mix is.
        values.max_level_percent = 20;
        let loud = vec![4.0f32; 2 * 64];
        MixPlan::new(&values, 2).mix_master(
            &ramp,
            &limiter,
            EffectLevels {
                reverb: 100,
                delay: 25,
            },
            &Wet::of(Some(&loud), None),
            &silent,
            &mut stereo,
        );
        let ceiling = (i32::from(i16::MAX) * 20 / 100) as f32 / 32_768.0;
        assert!(stereo.iter().all(|s| *s == ceiling), "{}", stereo[0]);

        // Under the master mute, the returns go: they glide out, then add nothing.
        values.muted = true;
        for _ in 0..2 {
            MixPlan::new(&values, 2).mix_master(
                &ramp,
                &limiter,
                levels,
                &Wet::of(Some(&flat), Some(&flat)),
                &silent,
                &mut stereo,
            );
        }
        assert!(stereo.iter().all(|s| *s == 0.0));
    }

    fn limiter_on(ceiling_db: f32, release_ms: f32) -> LimiterControl {
        let control = LimiterControl::default();
        control.set(LimiterSettings {
            enabled: true,
            ceiling_db,
            release_ms,
        });
        control
    }

    fn mastered(
        plan: &MixPlan,
        ramp: &MixRamp,
        limiter: &LimiterControl,
        source: &[f32],
        channels: usize,
    ) -> Vec<f32> {
        let mut stereo = vec![0.0; source.len() / channels * 2];
        plan.mix_master(
            ramp,
            limiter,
            EffectLevels::default(),
            &Wet::NONE,
            source,
            &mut stereo,
        );
        stereo
    }

    fn peak(samples: &[f32]) -> f32 {
        samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    /// Off (the default), the master limiter is not on the path at all: every block, steady or
    /// gliding, is the ramped mix bit for bit, over hundreds of drawn mixes.
    #[test]
    fn with_the_limiter_off_the_master_is_the_ramped_mix_bit_for_bit() {
        use golden::Noise;

        let limiter = LimiterControl::default();
        assert_eq!(limiter.settings(), LimiterSettings::master());
        let (plain, master) = (MixRamp::default(), MixRamp::default());
        let mut noise = Noise(0x0bad_cafe_f00d_d00d);
        for block in 0..600 {
            let channels = [2, 4, 8, 32][noise.below(4) as usize];
            let frames = 1 + noise.below(64) as usize;
            // A mix moves every third block; the others hold it.
            let values = if block % 3 == 0 {
                noise.mix()
            } else {
                MixValues::default()
            };
            let source: Vec<f32> = (0..channels * frames)
                .map(|_| f32::from(noise.sample()) / 16_384.0)
                .collect();
            let plan = MixPlan::new(&values, channels);
            let mut expected = vec![0.0; frames * 2];
            plan.mix_ramped(&plain, &source, &mut expected);
            let got = mastered(&plan, &master, &limiter, &source, channels);
            assert!(
                got.iter()
                    .zip(&expected)
                    .all(|(got, want)| got.to_bits() == want.to_bits()),
                "block {block} differs"
            );
            assert_eq!(master.limiter_reduction_db(), 0.0);
        }
    }

    /// A mix far over full scale comes out under the limiter's ceiling, its reduction is reported,
    /// and the clamp is not what held it: the limited block is the mix turned down, not cut flat.
    #[test]
    fn a_hot_mix_is_held_under_the_ceiling_and_its_reduction_reported() {
        // Four inputs, two on each side, each a 0.9 peak sine: up to 1.8 on each side.
        let source: Vec<f32> = (0..480)
            .flat_map(|frame| {
                let value = 0.9 * (frame as f32 * 0.05).sin();
                [value; 4]
            })
            .collect();
        let plan = MixPlan::new(&MixValues::default(), 4);
        let ramp = MixRamp::default();
        let limiter = limiter_on(-6.0, 100.0);
        let ceiling = 10f32.powf(-6.0 / 20.0);

        let limited = mastered(&plan, &ramp, &limiter, &source, 4);
        assert!(peak(&limited) <= ceiling, "{}", peak(&limited));
        assert!(peak(&limited) > ceiling * 0.99, "it limits, not mutes");
        let reduction = ramp.limiter_reduction_db();
        assert!(reduction > 9.0 && reduction < 12.0, "{reduction}");
        // Past the first crest the whole mix is turned down, the quiet parts too: a clip at the
        // ceiling would have left every sample under it as it was.
        let mut unlimited = vec![0.0; limited.len()];
        plan.mix_into(&source, &mut unlimited);
        let quiet: Vec<(f32, f32)> = limited
            .iter()
            .zip(&unlimited)
            .skip(2 * 40)
            .filter(|(_, plain)| plain.abs() > 0.05 && plain.abs() < ceiling)
            .map(|(limited, plain)| (*limited, *plain))
            .collect();
        assert!(quiet.len() > 60, "{}", quiet.len());
        assert!(
            quiet
                .iter()
                .all(|(limited, plain)| limited.abs() < plain.abs() * 0.5),
            "{quiet:?}"
        );

        // The engineer's limit lowers the ceiling below the limiter's: 25% is about -12 dBFS.
        let mut values = MixValues::default();
        values.engineer.limit_percent = 25;
        let capped = MixPlan::new(&values, 4);
        let limited = mastered(&capped, &MixRamp::default(), &limiter, &source, 4);
        assert!(peak(&limited) <= capped.ceiling, "{}", peak(&limited));
        assert!(peak(&limited) > capped.ceiling * 0.99);
    }

    /// The limiter's settings belong to the mix, its state to whoever mixes it: two mixes of the
    /// same settings (the local output and the recorder's local mix, or two musicians) never share
    /// what one of them is turning down.
    #[test]
    fn each_mix_limits_with_its_own_state() {
        let hot = vec![1.5f32; 2 * 64];
        let quiet = vec![0.25f32; 2 * 64];
        let plan = MixPlan::new(&MixValues::default(), 2);
        let limiter = limiter_on(-3.0, 500.0);
        let (output, recorder) = (MixRamp::default(), MixRamp::default());

        mastered(&plan, &output, &limiter, &hot, 2);
        assert!(output.limiter_reduction_db() > 6.0);
        assert_eq!(recorder.limiter_reduction_db(), 0.0, "untouched");

        let still_down = mastered(&plan, &output, &limiter, &quiet, 2);
        let fresh = mastered(&plan, &recorder, &limiter, &quiet, 2);
        let mut plain = vec![0.0; quiet.len()];
        plan.mix_into(&quiet, &mut plain);
        assert!(
            still_down[0] < 0.25 * 0.6,
            "still releasing: {}",
            still_down[0]
        );
        assert_eq!(fresh, plain, "the other mix starts from no reduction");
        assert_eq!(recorder.limiter_reduction_db(), 0.0);

        // A reset (a new take) starts the state again.
        output.reset();
        assert_eq!(output.limiter_reduction_db(), 0.0);
        assert_eq!(mastered(&plan, &output, &limiter, &quiet, 2), plain);
    }

    /// The effects' returns are limited with the dry mix: a quiet mix whose reverb and delay
    /// returns push it over the ceiling comes out under it, turned down by the limiter (its
    /// reduction reported), not cut flat by the clamp.
    #[test]
    fn the_effect_returns_are_limited_with_the_dry_mix() {
        let source: Vec<f32> = (0..2 * 480)
            .map(|index| 0.2 * ((index / 2) as f32 * 0.05).sin())
            .collect();
        let plan = MixPlan::new(&MixValues::default(), 2);
        let ceiling = 10f32.powf(-6.0 / 20.0);
        let mut dry = vec![0.0; source.len()];
        plan.mix_into(&source, &mut dry);
        assert!(peak(&dry) < ceiling * 0.9, "the dry mix alone is under it");

        let wet: Vec<f32> = (0..2 * 480)
            .map(|index| 0.9 * ((index / 2) as f32 * 0.031).cos())
            .collect();
        let levels = EffectLevels {
            reverb: 100,
            delay: 100,
        };
        let (limited, plain) = (MixRamp::default(), MixRamp::default());
        let mut returned = vec![0.0; source.len()];
        plan.mix_master(
            &limited,
            &limiter_on(-6.0, 100.0),
            levels,
            &Wet::of(Some(&wet), Some(&wet)),
            &source,
            &mut returned,
        );
        assert!(peak(&returned) <= ceiling, "{}", peak(&returned));
        assert!(peak(&returned) > ceiling * 0.99, "it limits, not mutes");
        assert!(
            limited.limiter_reduction_db() > 6.0,
            "{}",
            limited.limiter_reduction_db()
        );
        // The same returns with the limiter off reach far past the ceiling's -6 dB (the clamp at
        // the mix's full scale holds them there), so it was the limiter that held them.
        let mut unlimited = vec![0.0; source.len()];
        plan.mix_master(
            &plain,
            &LimiterControl::default(),
            levels,
            &Wet::of(Some(&wet), Some(&wet)),
            &source,
            &mut unlimited,
        );
        assert!(peak(&unlimited) > ceiling * 1.5, "{}", peak(&unlimited));
        assert_eq!(plain.limiter_reduction_db(), 0.0);
    }

    /// Two changes at once, one to each effect's return (the desk's reverb and delay sliders on
    /// two threads): every change lands, none undoes the other.
    #[test]
    fn changes_to_both_returns_at_once_never_undo_each_other() {
        let state = Arc::new(MixState::new(&GroupLayout::new(Vec::new())));
        for round in 0..50u8 {
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = Effect::ALL
                .into_iter()
                .map(|effect| {
                    let (state, barrier) = (Arc::clone(&state), Arc::clone(&barrier));
                    std::thread::spawn(move || {
                        barrier.wait();
                        for level in 0..=100 {
                            state.set_fx_return(effect, level);
                        }
                        state.set_fx_return(effect, round + 1)
                    })
                })
                .collect();
            for (effect, thread) in Effect::ALL.into_iter().zip(threads) {
                assert_eq!(thread.join().unwrap().get(effect), round + 1);
            }
            assert_eq!(
                state.fx_returns(),
                EffectLevels {
                    reverb: round + 1,
                    delay: round + 1
                },
                "round {round}"
            );
        }
        assert_eq!(state.set_fx_return(Effect::Delay, 250).delay, 100);
    }

    /// Turned off while working, the limiter lets go over its switch-off time and then the mix is
    /// as without it again, bit for bit; the settings are held in range.
    #[test]
    fn a_limiter_turned_off_lets_go_and_then_leaves_the_mix_alone() {
        let plan = MixPlan::new(&MixValues::default(), 2);
        let ramp = MixRamp::default();
        let limiter = limiter_on(-12.0, 1_000.0);
        mastered(&plan, &ramp, &limiter, &[1.0f32; 2 * 64], 2);
        assert!(ramp.limiter_reduction_db() > 11.0);

        let kept = limiter.set(LimiterSettings {
            enabled: false,
            ceiling_db: -40.0,
            release_ms: f32::NAN,
        });
        assert_eq!(
            kept,
            LimiterSettings {
                enabled: false,
                ceiling_db: -24.0,
                release_ms: 50.0,
            },
            "held in range"
        );
        let quiet = vec![0.1f32; 2 * 480];
        let letting_go = mastered(&plan, &ramp, &limiter, &quiet, 2);
        assert!(letting_go[0] < 0.1 && letting_go[0] > 0.0);
        // The switch-off release is a 20 ms time constant: give it 400 ms.
        for _ in 0..40 {
            mastered(&plan, &ramp, &limiter, &quiet, 2);
        }
        let plain = {
            let mut stereo = vec![0.0; quiet.len()];
            plan.mix_ramped(&MixRamp::default(), &quiet, &mut stereo);
            stereo
        };
        assert_eq!(mastered(&plan, &ramp, &limiter, &quiet, 2), plain);
        assert_eq!(ramp.limiter_reduction_db(), 0.0);
    }

    #[test]
    fn mixing_through_a_working_limiter_never_allocates() {
        let ramp = MixRamp::default();
        let limiter = limiter_on(-6.0, 50.0);
        let source = vec![0.75f32; 256 * 32];
        let mut stereo = vec![0.0f32; 256 * 2];
        let mut values = MixValues::default();

        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for block in 0..300 {
                values.channel_gains[block % 32] = (block % 101) as u8;
                if block % 50 == 0 {
                    limiter.set(LimiterSettings {
                        enabled: block % 100 == 0,
                        ceiling_db: -(block as f32 % 24.0),
                        release_ms: 20.0,
                    });
                }
                MixPlan::new(&values, 32).mix_master(
                    &ramp,
                    &limiter,
                    EffectLevels::default(),
                    &Wet::NONE,
                    &source,
                    &mut stereo,
                );
            }
        });

        assert_eq!((allocations, frees), (0, 0));
    }
    use crate::targets::TargetRegistry;
    use std::net::IpAddr;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    fn group(id: &str, name: &str, channels: &[usize]) -> Group {
        Group {
            id: id.to_owned(),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    /// Drums, Bass and Keys, in that order, one channel each.
    fn trio() -> GroupLayout {
        GroupLayout::new(vec![
            group("drums", "Drums", &[0]),
            group("bass", "Bass", &[1]),
            group("keys", "Keys", &[2]),
        ])
    }

    /// Group levels with Drums at 10, Bass at 20 and Keys at 30, the rest neutral.
    fn trio_levels() -> [u8; MAX_GROUPS] {
        let mut levels = [NEUTRAL_GROUP_LEVEL; MAX_GROUPS];
        levels[..3].copy_from_slice(&[10, 20, 30]);
        levels
    }

    fn regrouped(new: &GroupLayout) -> [u8; MAX_GROUPS] {
        regrouped_levels(&trio_levels(), &group_sources(&trio(), new))
    }

    #[test]
    fn renaming_a_group_or_changing_its_channels_keeps_its_level() {
        let edited = GroupLayout::new(vec![
            group("drums", "Kit", &[0, 3]),
            group("bass", "Bass", &[1]),
            group("keys", "Keys", &[2]),
        ]);

        assert_eq!(regrouped(&edited), trio_levels());
        assert!(trio().same_positions(&edited));
    }

    #[test]
    fn reordering_groups_moves_each_level_with_its_group() {
        let reordered = GroupLayout::new(vec![
            group("keys", "Keys", &[2]),
            group("drums", "Drums", &[0]),
            group("bass", "Bass", &[1]),
        ]);

        assert_eq!(&regrouped(&reordered)[..4], &[30, 10, 20, 100]);
        assert!(!trio().same_positions(&reordered));
    }

    #[test]
    fn deleting_a_group_drops_its_level() {
        let without_bass = GroupLayout::new(vec![
            group("drums", "Drums", &[0]),
            group("keys", "Keys", &[2]),
        ]);

        let levels = regrouped(&without_bass);

        assert_eq!(
            &levels[..2],
            &[10, 30],
            "Keys keeps its level in its new place"
        );
        assert!(
            levels[2..]
                .iter()
                .all(|&level| level == NEUTRAL_GROUP_LEVEL),
            "nothing is left of the deleted group: {levels:?}"
        );
    }

    #[test]
    fn a_new_group_starts_at_the_neutral_level() {
        let with_vocals = GroupLayout::new(vec![
            group("vocals", "Vocals", &[3]),
            group("drums", "Drums", &[0]),
            group("bass", "Bass", &[1]),
            group("keys", "Keys", &[2]),
        ]);

        assert_eq!(&regrouped(&with_vocals)[..4], &[100, 10, 20, 30]);
    }

    #[test]
    fn a_new_group_never_inherits_a_deleted_groups_level_by_taking_its_place() {
        let bass_replaced = GroupLayout::new(vec![
            group("drums", "Drums", &[0]),
            group("synth", "Bass", &[1]),
            group("keys", "Keys", &[2]),
        ]);

        assert_eq!(&regrouped(&bass_replaced)[..3], &[10, 100, 30]);
    }

    #[test]
    fn regrouping_a_mix_moves_its_levels_and_membership_under_a_new_revision() {
        let state = MixState::new(&trio());
        state.update(MixValues {
            group_levels: trio_levels(),
            max_level_percent: 90,
            ..MixValues::default()
        });
        let before = state.current();
        let reordered = GroupLayout::new(vec![
            group("keys", "Keys", &[2]),
            group("drums", "Drums", &[0]),
        ]);

        let moved = state.regroup(&reordered, &group_sources(&trio(), &reordered));

        assert_eq!(moved, state.current(), "the moved mix is what is stored");
        assert!(
            moved.revision > before.revision,
            "the phone is sent the move"
        );
        assert_eq!(&moved.group_levels[..3], &[30, 10, 100]);
        assert_eq!(
            moved.channel_group[2],
            Some(0),
            "Keys is now the first group"
        );
        assert_eq!(moved.channel_group[0], Some(1));
        assert_eq!(moved.channel_group[1], None, "Bass is gone");
        assert_eq!(moved.max_level_percent, 90, "nothing else changes");
        // Keys' channel is still scaled by Keys' level, now read from its new position.
        assert_eq!(mix_channels(&[0, 0, 10_000], 3, moved), [3_000, 0]);
    }

    #[test]
    fn independent_mix_states_transform_the_same_input_differently() {
        let targets = vec![
            "192.168.1.3:50000".parse().unwrap(),
            "192.168.1.4:50000".parse().unwrap(),
        ];
        let registry = TargetRegistry::new(&targets, &GroupLayout::default()).unwrap();
        registry
            .mix_state("192.168.1.3".parse().unwrap())
            .unwrap()
            .update(MixValues {
                volume_percent: 50,
                ..MixValues::default()
            });
        registry
            .mix_state("192.168.1.4".parse().unwrap())
            .unwrap()
            .update(MixValues::default());

        let input = [20_000, -20_000];
        let first_output = mix_channels(
            &input,
            2,
            registry
                .mix_state("192.168.1.3".parse().unwrap())
                .unwrap()
                .snapshot(),
        );
        let second_output = mix_channels(
            &input,
            2,
            registry
                .mix_state("192.168.1.4".parse().unwrap())
                .unwrap()
                .snapshot(),
        );

        assert_eq!(first_output, [10_000, -10_000]);
        assert_eq!(second_output, input);
    }

    #[test]
    fn updating_one_mix_state_does_not_change_another() {
        let targets = vec![
            "192.168.1.3:50000".parse().unwrap(),
            "192.168.1.4:50000".parse().unwrap(),
        ];
        let registry = TargetRegistry::new(&targets, &GroupLayout::default()).unwrap();
        let first_ip: IpAddr = "192.168.1.3".parse().unwrap();
        let second_ip: IpAddr = "192.168.1.4".parse().unwrap();
        let second_state = registry.mix_state(second_ip).unwrap();
        let initial_second = second_state.snapshot();

        registry.mix_state(first_ip).unwrap().update(MixValues {
            volume_percent: 0,
            max_level_percent: 10,
            muted: true,
            ..MixValues::default()
        });

        assert_eq!(second_state.snapshot(), initial_second);
    }

    #[test]
    fn clamps_mix_command_and_applies_gain_ceiling() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":150,"max_level_percent":25,"muted":false}"#,
            MixValues::default(),
        )
        .unwrap();
        assert_eq!(values.volume_percent, 100);
        assert_eq!(values.max_level_percent, 25);

        let samples = mix_channels(&[i16::MIN, -16_000, 16_000, i16::MAX], 2, values);
        assert_eq!(samples, [-8191, -8191, 8191, 8191]);
    }

    #[test]
    fn mute_zeroes_samples_and_rejects_other_message_types() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":true}"#,
            MixValues::default(),
        )
        .unwrap();
        let samples = mix_channels(&[i16::MIN, 0, i16::MAX, 100], 2, values);
        assert_eq!(samples, [0, 0, 0, 0]);
        assert!(parse_mix_command(
            r#"{"type":"status","volume_percent":80,"max_level_percent":90,"muted":false}"#,
            MixValues::default(),
        )
        .is_err());
    }

    #[test]
    fn neutral_channel_gains_and_default_pans_leave_the_buffer_unchanged() {
        let original = [-16_000, -8_000, 8_000, 16_000];

        let samples = mix_channels(&original, 2, MixValues::default());

        assert_eq!(samples, original);
    }

    #[test]
    fn gain_table_holds_one_entry_per_supported_channel() {
        assert_eq!(MixValues::default().channel_gains.len(), MAX_MIX_CHANNELS);
    }

    #[test]
    fn per_channel_gains_transform_only_their_own_channel() {
        let values = MixValues {
            channel_gains: std::array::from_fn(|index| if index == 1 { 50 } else { 100 }),
            ..MixValues::default()
        };

        let samples = mix_channels(&[10_000, 10_000, -10_000, -10_000], 2, values);

        assert_eq!(samples, [10_000, 5_000, -10_000, -5_000]);
    }

    #[test]
    fn absent_channels_field_preserves_existing_gains() {
        let current = MixValues {
            channel_gains: std::array::from_fn(|index| if index == 0 { 40 } else { 70 }),
            ..MixValues::default()
        };

        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#,
            current,
        )
        .unwrap();

        assert_eq!(values.channel_gains, current.channel_gains);
        assert_eq!(values.volume_percent, 80);
    }

    #[test]
    fn channel_gains_are_clamped_and_oversized_lists_are_rejected() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"channels":[150,-20,100]}"#,
            MixValues::default(),
        )
        .unwrap();

        assert_eq!(&values.channel_gains[..3], &[100, 0, 100]);
        assert_eq!(values.channel_gains[3], 100);

        let oversized = format!(
            r#"{{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"channels":[{}]}}"#,
            vec!["100"; MAX_MIX_CHANNELS + 1].join(",")
        );
        assert!(parse_mix_command(&oversized, MixValues::default()).is_err());
    }

    #[test]
    fn centring_a_source_places_it_in_both_outputs() {
        let values = MixValues {
            pans: std::array::from_fn(|_| 50),
            ..MixValues::default()
        };

        let samples = mix_channels(&[10_000, 0], 2, values);

        // cos(PI/4) == sin(PI/4), so both outputs get the same share.
        assert_eq!(samples, [7_071, 7_071]);
    }

    #[test]
    fn panning_everything_left_empties_the_right_output() {
        let values = MixValues {
            pans: std::array::from_fn(|_| 0),
            ..MixValues::default()
        };

        let samples = mix_channels(&[10_000, -4_000], 2, values);

        assert_eq!(samples, [6_000, 0]);
    }

    #[test]
    fn a_multichannel_source_is_summed_into_a_stereo_output() {
        let samples = mix_channels(&[1_000, 2_000, 3_000, 4_000], 4, MixValues::default());

        assert_eq!(samples.len(), 2);
        assert_eq!(samples, [4_000, 6_000]);
    }

    #[test]
    fn absent_pans_field_preserves_existing_pans_and_oversized_lists_are_rejected() {
        let current = MixValues {
            pans: std::array::from_fn(|index| if index == 0 { 25 } else { 75 }),
            ..MixValues::default()
        };

        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#,
            current,
        )
        .unwrap();
        assert_eq!(values.pans, current.pans);

        let clamped = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"pans":[150,-5,50]}"#,
            current,
        )
        .unwrap();
        assert_eq!(&clamped.pans[..3], &[100, 0, 50]);

        let oversized = format!(
            r#"{{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"pans":[{}]}}"#,
            vec!["50"; MAX_MIX_CHANNELS + 1].join(",")
        );
        assert!(parse_mix_command(&oversized, current).is_err());
    }

    #[test]
    fn default_pan_reproduces_the_captured_interleaved_layout() {
        let values = MixValues::default();

        assert_eq!(values.pans[0], 0);
        assert_eq!(values.pans[1], 100);
        assert_eq!(values.pans[2], 0);
        assert_eq!(values.pans[3], 100);
    }

    #[test]
    fn solo_silences_every_other_channel() {
        let source = [10_000, 20_000];
        let values = MixValues {
            channel_solo: std::array::from_fn(|index| index == 0),
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, values), [10_000, 0]);
    }

    #[test]
    fn clearing_the_last_solo_restores_the_previous_levels() {
        let source = [10_000, 20_000];
        let soloed = MixValues {
            channel_solo: std::array::from_fn(|index| index == 0),
            ..MixValues::default()
        };
        assert_eq!(mix_channels(&source, 2, soloed), [10_000, 0]);

        let cleared = MixValues::default();
        assert_eq!(mix_channels(&source, 2, cleared), source);
    }

    #[test]
    fn muting_a_channel_silences_only_that_channel() {
        let source = [10_000, 20_000];
        let values = MixValues {
            channel_muted: std::array::from_fn(|index| index == 1),
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, values), [10_000, 0]);
    }

    #[test]
    fn a_muted_soloed_channel_stays_silent() {
        let source = [10_000, 20_000];
        let values = MixValues {
            channel_muted: std::array::from_fn(|index| index == 0),
            channel_solo: std::array::from_fn(|index| index == 0),
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, values), [0, 0]);
    }

    #[test]
    fn solo_applies_on_top_of_the_stored_gains_without_changing_them() {
        let source = [10_000, 20_000];
        let values = MixValues {
            channel_gains: std::array::from_fn(|index| if index == 1 { 50 } else { 100 }),
            channel_solo: std::array::from_fn(|index| index == 1),
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, values), [0, 10_000]);
    }

    #[test]
    fn absent_mutes_and_solos_preserve_the_stored_flags() {
        let current = MixValues {
            channel_muted: std::array::from_fn(|index| index == 1),
            channel_solo: std::array::from_fn(|index| index == 0),
            ..MixValues::default()
        };

        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#,
            current,
        )
        .unwrap();

        assert_eq!(values.channel_muted, current.channel_muted);
        assert_eq!(values.channel_solo, current.channel_solo);
    }

    #[test]
    fn mutes_and_solos_are_applied_and_oversized_lists_are_rejected() {
        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"mutes":[true,false],"solos":[false,true]}"#,
            MixValues::default(),
        )
        .unwrap();

        assert!(values.channel_muted[0]);
        assert!(!values.channel_muted[1]);
        assert!(!values.channel_solo[0]);
        assert!(values.channel_solo[1]);

        let oversized = format!(
            r#"{{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"mutes":[{}]}}"#,
            vec!["false"; MAX_MIX_CHANNELS + 1].join(",")
        );
        assert!(parse_mix_command(&oversized, MixValues::default()).is_err());
    }

    #[test]
    fn a_group_level_scales_every_member_and_keeps_their_balance() {
        let layout = GroupLayout::new(vec![group("kit", "Drums", &[0, 1])]);
        let mut group_levels = [100u8; MAX_GROUPS];
        group_levels[0] = 50;
        let values = MixValues {
            channel_group: layout.channel_group(),
            group_levels,
            ..MixValues::default()
        };

        let samples = mix_channels(&[20_000, 20_000], 2, values);

        // Both members are halved, and because their pans are opposite the sum lands in each
        // output at the same reduced level: the relative balance survives.
        assert_eq!(samples, [10_000, 10_000]);
    }

    #[test]
    fn a_channel_in_no_group_ignores_every_group_level() {
        let layout = GroupLayout::new(vec![group("kit", "Drums", &[0])]);
        let mut group_levels = [100u8; MAX_GROUPS];
        group_levels[0] = 0;
        let values = MixValues {
            channel_group: layout.channel_group(),
            group_levels,
            ..MixValues::default()
        };

        let samples = mix_channels(&[10_000, 10_000], 2, values);

        assert_eq!(samples, [0, 10_000]);
    }

    #[test]
    fn no_groups_reproduces_the_current_mix_byte_for_byte() {
        let source = [-16_000, -8_000, 8_000, 16_000];
        let layout = GroupLayout::default();
        let without_groups = MixValues {
            channel_group: layout.channel_group(),
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, without_groups), source);
        assert_eq!(
            mix_channels(&source, 2, without_groups),
            mix_channels(&source, 2, MixValues::default())
        );
    }

    #[test]
    fn absent_group_levels_preserve_and_oversized_lists_are_rejected() {
        let mut current = MixValues::default();
        current.group_levels[0] = 40;

        let preserved = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#,
            current,
        )
        .unwrap();
        assert_eq!(preserved.group_levels, current.group_levels);

        let clamped = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"group_levels":[150,-5,50]}"#,
            current,
        )
        .unwrap();
        assert_eq!(&clamped.group_levels[..3], &[100, 0, 50]);

        let oversized = format!(
            r#"{{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"group_levels":[{}]}}"#,
            vec!["50"; MAX_GROUPS + 1].join(",")
        );
        assert!(parse_mix_command(&oversized, current).is_err());
    }

    #[test]
    fn a_mix_update_preserves_the_servers_group_membership() {
        let layout = GroupLayout::new(vec![group("kit", "Drums", &[0])]);

        let values = parse_mix_command(
            r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"group_levels":[50]}"#,
            MixValues {
                channel_group: layout.channel_group(),
                ..MixValues::default()
            },
        )
        .unwrap();

        assert_eq!(values.channel_group[0], Some(0));
        assert_eq!(values.group_levels[0], 50);
    }

    #[test]
    fn a_partial_update_changes_only_the_fields_it_carries() {
        let state = MixState::new(&GroupLayout::default());
        state.update(MixValues {
            channel_gains: std::array::from_fn(|index| if index == 0 { 40 } else { 70 }),
            channel_muted: std::array::from_fn(|index| index == 1),
            volume_percent: 60,
            max_level_percent: 80,
            muted: true,
            ..MixValues::default()
        });
        let before = state.snapshot();

        let applied = state
            .apply(&MixUpdate {
                pans: Some(vec![50]),
                volume_percent: Some(150),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        assert_eq!(
            applied,
            state.snapshot(),
            "the applied mix is what is stored"
        );
        assert_eq!(applied.pans[0], 50);
        assert_eq!(&applied.pans[1..], &before.pans[1..]);
        assert_eq!(applied.volume_percent, 100, "clamped like a phone's value");
        assert_eq!(applied.channel_gains, before.channel_gains);
        assert_eq!(applied.channel_muted, before.channel_muted);
        assert_eq!(applied.channel_solo, before.channel_solo);
        assert_eq!(applied.group_levels, before.group_levels);
        assert_eq!(applied.max_level_percent, 80);
        assert!(applied.muted);
    }

    #[test]
    fn an_invalid_partial_update_changes_nothing() {
        let state = MixState::new(&GroupLayout::default());
        let before = state.snapshot();

        let error = state
            .apply(&MixUpdate {
                volume_percent: Some(10),
                channel_gains: Some(vec![50; MAX_MIX_CHANNELS + 1]),
                ..MixUpdate::default()
            })
            .expect_err("an oversized list is rejected");

        assert!(
            error.contains("channels"),
            "the error names the list: {error}"
        );
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn replacing_the_group_layout_stops_an_invalid_group_from_affecting_audio() {
        let layout = GroupLayout::new(vec![group("kit", "Drums", &[0])]);
        let state = MixState::new(&layout);
        let mut group_levels = [100u8; MAX_GROUPS];
        group_levels[0] = 50;
        state.update(MixValues {
            group_levels,
            ..MixValues::default()
        });

        // Valid mapping: the group level scales the channel.
        assert_eq!(state.snapshot().channel_group[0], Some(0));
        assert_eq!(mix_channels(&[10_000], 1, state.snapshot()), [5_000, 0]);

        // The device no longer has that channel, so the group is mapped to nothing and the channel
        // mixes at its own gain. The stored group level is preserved for when the channel returns.
        let invalid = GroupLayout::new(vec![group("kit", "Drums", &[])]);
        state.set_group_layout(&invalid);

        assert_eq!(state.snapshot().channel_group[0], None);
        assert_eq!(state.snapshot().group_levels[0], 50);
        assert_eq!(mix_channels(&[10_000], 1, state.snapshot()), [10_000, 0]);
    }

    /// A whole-mix store waits for a writer in progress instead of slipping in between that
    /// writer's read and its store.
    #[test]
    fn replacing_the_whole_mix_waits_for_the_writer_lock() {
        let state = Arc::new(MixState::new(&GroupLayout::default()));
        let held = state.writer.lock().expect("mix writer lock");
        let writer = {
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                state.update(MixValues {
                    volume_percent: 10,
                    ..MixValues::default()
                })
            })
        };

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            state.snapshot().volume_percent,
            100,
            "the store must wait while another writer holds the lock"
        );

        drop(held);
        writer
            .join()
            .expect("the store finishes once the lock is free");
        assert_eq!(state.snapshot().volume_percent, 10);
    }

    #[test]
    fn every_applied_change_takes_a_higher_revision() {
        let state = MixState::new(&GroupLayout::default());
        let start = state.current().revision;

        let first = state
            .apply(&MixUpdate {
                volume_percent: Some(50),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let second = state
            .apply(&MixUpdate {
                muted: Some(true),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        assert!(first.revision > start);
        assert!(second.revision > first.revision);
        assert_eq!(
            state.current(),
            second,
            "the stored mix carries its revision"
        );
        assert_eq!(state.snapshot().revision, second.revision);
    }

    #[test]
    fn a_refused_change_keeps_the_revision() {
        let state = MixState::new(&GroupLayout::default());
        let before = state.current();

        state
            .apply(&MixUpdate {
                group_levels: Some(vec![1; MAX_GROUPS + 1]),
                ..MixUpdate::default()
            })
            .expect_err("a table longer than the mix is refused");

        assert_eq!(state.current(), before);
    }

    #[test]
    fn a_new_mix_starts_above_every_revision_handed_out_before() {
        let old = MixState::new(&GroupLayout::default());
        let changed = old
            .apply(&MixUpdate {
                volume_percent: Some(5),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        let fresh = MixState::new(&GroupLayout::default());

        assert!(
            fresh.current().revision > changed.revision,
            "a musician removed and added again never looks older than before"
        );
    }

    #[test]
    fn concurrent_changes_take_distinct_increasing_revisions() {
        const WRITERS: usize = 4;
        const CHANGES: usize = 500;
        let state = Arc::new(MixState::new(&GroupLayout::default()));
        let writers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    (0..CHANGES)
                        .map(|change| {
                            state
                                .apply(&MixUpdate {
                                    volume_percent: Some(((writer + change) % 101) as i32),
                                    ..MixUpdate::default()
                                })
                                .expect("a valid update applies")
                                .revision
                        })
                        .collect::<Vec<u64>>()
                })
            })
            .collect();

        let mut all = Vec::new();
        for writer in writers {
            let revisions = writer.join().expect("the writer finishes");
            assert!(
                revisions.windows(2).all(|pair| pair[0] < pair[1]),
                "each writer sees its own revisions rise"
            );
            all.extend(revisions);
        }
        let count = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), count, "no two changes share a revision");
        assert_eq!(
            Some(state.current().revision),
            all.last().copied(),
            "the stored mix carries the newest revision"
        );
    }

    /// The phone and the desktop change different parts of one mix at the same moment, round after
    /// round: neither change is ever lost.
    #[test]
    fn concurrent_writers_of_disjoint_fields_never_lose_a_change() {
        const ROUNDS: usize = 2_000;
        let state = Arc::new(MixState::new(&GroupLayout::default()));
        let start = Arc::new(Barrier::new(3));
        let done = Arc::new(Barrier::new(3));
        let spawn_writer = |update: fn(usize) -> MixUpdate| {
            let (state, start, done) = (Arc::clone(&state), Arc::clone(&start), Arc::clone(&done));
            std::thread::spawn(move || {
                for round in 0..ROUNDS {
                    start.wait();
                    state.apply(&update(round)).expect("a valid update applies");
                    done.wait();
                }
            })
        };
        let phone = spawn_writer(|round| MixUpdate {
            volume_percent: Some((round % 101) as i32),
            ..MixUpdate::default()
        });
        let desktop = spawn_writer(|round| MixUpdate {
            channel_gains: Some(vec![(round * 7 % 101) as i32]),
            ..MixUpdate::default()
        });

        for round in 0..ROUNDS {
            start.wait();
            done.wait();
            let mix = state.snapshot();
            assert_eq!(
                usize::from(mix.volume_percent),
                round % 101,
                "the phone's change in round {round} was lost"
            );
            assert_eq!(
                usize::from(mix.channel_gains[0]),
                round * 7 % 101,
                "the desktop's change in round {round} was lost"
            );
        }
        phone.join().expect("the phone's writer finishes");
        desktop.join().expect("the desktop's writer finishes");
    }

    /// The flags for stereo pairs starting at the channels `linked` and the hidden channels
    /// `hidden`, all 0-based.
    fn flags(linked: &[usize], hidden: &[usize]) -> ChannelFlags {
        let mask = |channels: &[usize]| {
            channels
                .iter()
                .fold(0, |mask, &channel| mask | 1 << channel)
        };
        ChannelFlags {
            hidden: mask(hidden),
            linked: mask(linked),
        }
    }

    fn gains(values: &[i32]) -> MixUpdate {
        MixUpdate {
            channel_gains: Some(values.to_vec()),
            ..MixUpdate::default()
        }
    }

    #[test]
    fn a_hidden_channel_is_silent_and_showing_it_again_restores_it() {
        let source = [10_000, 20_000];
        let hidden = MixValues {
            hidden_channels: 0b10,
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&source, 2, hidden), [10_000, 0]);
        assert_eq!(mix_channels(&source, 2, MixValues::default()), source);
    }

    /// A spare input the engineer hid cannot leave a musician hearing nothing because the phone
    /// soloed it before it was hidden: its solo counts for nothing.
    #[test]
    fn a_hidden_channels_solo_silences_no_other_channel() {
        let values = MixValues {
            channel_solo: std::array::from_fn(|index| index == 1),
            hidden_channels: 0b10,
            ..MixValues::default()
        };

        assert_eq!(mix_channels(&[10_000, 20_000], 2, values), [10_000, 0]);
    }

    #[test]
    fn a_change_to_one_side_of_a_linked_pair_moves_the_other() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[0], &[]));

        let first = state.apply(&gains(&[80])).expect("a valid update applies");
        assert_eq!(&first.channel_gains[..3], &[80, 80, 100]);

        // An older phone shows the two sides apart and moves the second one only.
        let second = state
            .apply(&gains(&[80, 40]))
            .expect("a valid update applies");
        assert_eq!(&second.channel_gains[..2], &[40, 40]);

        let muted = state
            .apply(&MixUpdate {
                channel_muted: Some(vec![false, true]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        assert_eq!(&muted.channel_muted[..2], &[true, true]);
        let soloed = state
            .apply(&MixUpdate {
                channel_solo: Some(vec![true]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        assert_eq!(&soloed.channel_solo[..2], &[true, true]);
        assert_eq!(soloed, state.current(), "what it answers is what is stored");
    }

    #[test]
    fn when_both_sides_of_a_pair_change_and_disagree_the_first_channel_wins() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[2], &[]));

        // Only a level can change on both sides to different values: a flag of an equal pair that
        // changes on both sides changes to the same value.
        let applied = state
            .apply(&gains(&[100, 100, 30, 60]))
            .expect("a valid update applies");

        assert_eq!(&applied.channel_gains[2..4], &[30, 30]);
    }

    /// A pair's balance is the phone's: it writes both pans, and the server keeps each as sent.
    #[test]
    fn the_pans_of_a_pair_and_the_channels_outside_it_move_on_their_own() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[0], &[]));

        let applied = state
            .apply(&MixUpdate {
                pans: Some(vec![20, 70]),
                channel_gains: Some(vec![100, 100, 50]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        assert_eq!(&applied.pans[..2], &[20, 70]);
        assert_eq!(
            &applied.channel_gains[2..4],
            &[50, 100],
            "CH 3 and CH 4 are no pair"
        );
    }

    #[test]
    fn linking_a_pair_pans_it_apart_and_evens_it_out_under_a_new_revision() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply(&MixUpdate {
                channel_gains: Some(vec![100, 70, 40]),
                channel_muted: Some(vec![false, false, true]),
                pans: Some(vec![0, 30, 60]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let before = state.current();

        let linked = state
            .set_channel_flags(flags(&[1], &[]))
            .expect("linking moves the pair");

        assert_eq!(linked, state.current(), "what it answers is what is stored");
        assert!(
            linked.revision > before.revision,
            "the phone is sent the change"
        );
        assert_eq!(
            &linked.pans[..3],
            &[0, 0, 100],
            "CH 2 hard left, CH 3 hard right"
        );
        assert_eq!(
            &linked.channel_gains[..3],
            &[100, 70, 70],
            "the first side's level"
        );
        assert_eq!(&linked.channel_muted[..3], &[false, false, false]);
        assert_eq!(
            state.set_channel_flags(flags(&[1], &[])),
            None,
            "a pair already linked is not moved again"
        );
    }

    #[test]
    fn hiding_a_channel_or_unlinking_a_pair_changes_no_stored_value() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[0], &[]));
        let before = state.current();

        assert_eq!(state.set_channel_flags(flags(&[], &[3])), None);

        let after = state.current();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.hidden_channels, 0b1000);
        let apart = state.apply(&gains(&[50])).expect("a valid update applies");
        assert_eq!(
            &apart.channel_gains[..2],
            &[50, 100],
            "the sides move apart again"
        );
    }

    #[test]
    fn a_new_mix_starts_with_its_pairs_panned_apart_and_its_hidden_channels_silent() {
        let mix = MixState::configured(&GroupLayout::default(), flags(&[1], &[4])).current();

        assert_eq!(&mix.pans[..4], &[0, 0, 100, 100]);
        assert_eq!(mix.hidden_channels, 0b1_0000);
    }

    fn locks(channels: &[bool]) -> EngineerUpdate {
        EngineerUpdate {
            locked: Some(channels.to_vec()),
            ..EngineerUpdate::default()
        }
    }

    /// A phone's whole mix: every table and the master values, as a phone sends it.
    fn whole_phone_mix(gains: &[i32], pans: &[i32], mutes: &[bool], solos: &[bool]) -> MixUpdate {
        MixUpdate {
            channel_gains: Some(gains.to_vec()),
            pans: Some(pans.to_vec()),
            channel_muted: Some(mutes.to_vec()),
            channel_solo: Some(solos.to_vec()),
            group_levels: Some(vec![100; 2]),
            volume_percent: Some(70),
            max_level_percent: Some(90),
            muted: Some(false),
        }
    }

    #[test]
    fn the_engineers_controls_start_open_and_change_under_a_new_revision() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply(&gains(&[40, 50]))
            .expect("a valid update applies");
        let before = state.current();
        assert_eq!(before.engineer, EngineerControls::default());
        assert_eq!(before.engineer.limit_percent, 100);

        let set = state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(60),
                locked: Some(vec![true]),
            })
            .expect("valid controls apply");

        assert_eq!(set, state.current(), "what it answers is what is stored");
        assert!(
            set.revision > before.revision,
            "the phone is sent the change"
        );
        assert_eq!(
            set.engineer,
            EngineerControls {
                limit_percent: 60,
                locked: 0b1,
            }
        );
        assert_eq!(
            MixValues {
                engineer: before.engineer,
                revision: before.revision,
                ..set
            },
            before,
            "no value of the mix moves"
        );

        let clamped = state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(150),
                ..EngineerUpdate::default()
            })
            .expect("valid controls apply");
        assert_eq!(clamped.engineer.limit_percent, 100);
        assert_eq!(
            clamped.engineer.locked, 0b1,
            "an absent list keeps the locks"
        );
        let clamped = state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(-5),
                ..EngineerUpdate::default()
            })
            .expect("valid controls apply");
        assert_eq!(clamped.engineer.limit_percent, 0);
    }

    #[test]
    fn an_oversized_lock_list_changes_nothing() {
        let state = MixState::new(&GroupLayout::default());
        let before = state.current();

        let error = state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(10),
                locked: Some(vec![true; MAX_MIX_CHANNELS + 1]),
            })
            .expect_err("a list longer than the mix is refused");

        assert!(
            error.contains("locked"),
            "the error names the list: {error}"
        );
        assert_eq!(state.current(), before);
    }

    #[test]
    fn a_phone_cannot_change_a_locked_channel_and_the_rest_of_its_mix_applies() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply(&MixUpdate {
                channel_gains: Some(vec![40, 50]),
                pans: Some(vec![30, 60]),
                channel_muted: Some(vec![true, false]),
                channel_solo: Some(vec![false, false]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        state
            .apply_engineer(&locks(&[true]))
            .expect("valid controls apply");
        let before = state.current();

        let applied = state
            .apply_from_phone(&whole_phone_mix(
                &[90, 80],
                &[50, 50],
                &[false, true],
                &[true, false],
            ))
            .expect("a valid mix applies");

        assert_eq!(applied, state.current(), "the ack carries what is stored");
        assert!(applied.revision > before.revision);
        assert_eq!(applied.channel_gains[0], 40, "CH 1's level is kept");
        assert_eq!(applied.pans[0], 30, "CH 1's pan is kept");
        assert!(applied.channel_muted[0], "CH 1's mute is kept");
        assert!(!applied.channel_solo[0], "CH 1's solo is kept");
        assert_eq!(applied.channel_gains[1], 80, "CH 2 is the phone's");
        assert_eq!(applied.pans[1], 50);
        assert!(applied.channel_muted[1]);
        assert_eq!(
            applied.volume_percent, 70,
            "the master values are the phone's"
        );
        assert_eq!(applied.max_level_percent, 90);
        assert_eq!(applied.engineer, before.engineer);
    }

    #[test]
    fn the_desk_can_still_change_a_locked_channel() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply_engineer(&locks(&[true]))
            .expect("valid controls apply");

        let applied = state
            .apply(&MixUpdate {
                channel_gains: Some(vec![25]),
                channel_muted: Some(vec![true]),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        assert_eq!(applied.channel_gains[0], 25);
        assert!(applied.channel_muted[0]);
        assert!(applied.engineer.is_locked(0), "and the lock stays");
    }

    #[test]
    fn locking_either_side_of_a_pair_locks_both_and_unlocking_either_unlocks_both() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[1], &[]));

        let second = state
            .apply_engineer(&locks(&[false, false, true]))
            .expect("valid controls apply");
        assert_eq!(second.engineer.locked, 0b110, "CH 2 and CH 3 are one strip");

        let first = state
            .apply_engineer(&locks(&[false, false]))
            .expect("valid controls apply");
        assert_eq!(first.engineer.locked, 0, "unlocking CH 2 unlocks CH 3");

        let disagreeing = state
            .apply_engineer(&locks(&[false, true, false]))
            .expect("valid controls apply");
        assert_eq!(
            disagreeing.engineer.locked, 0b110,
            "when both sides are set apart the first channel wins"
        );
    }

    #[test]
    fn a_phone_cannot_move_either_side_of_a_locked_pair() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[0], &[]));
        state.apply(&gains(&[60])).expect("a valid update applies");
        state
            .apply_engineer(&locks(&[true]))
            .expect("valid controls apply");

        // An older phone shows the sides apart and moves the second one only.
        let applied = state
            .apply_from_phone(&gains(&[60, 20]))
            .expect("a valid mix applies");

        assert_eq!(&applied.channel_gains[..2], &[60, 60]);
    }

    #[test]
    fn a_pair_linked_after_one_side_was_locked_is_locked_whole() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply_engineer(&locks(&[false, true]))
            .expect("valid controls apply");

        let linked = state
            .set_channel_flags(flags(&[0], &[]))
            .expect("linking moves the pair");

        assert_eq!(linked.engineer.locked, 0b11);
        let applied = state
            .apply_from_phone(&gains(&[10, 10]))
            .expect("a valid mix applies");
        assert_eq!(
            &applied.channel_gains[..2],
            &[100, 100],
            "neither side moves"
        );
    }

    #[test]
    fn the_output_ceiling_is_the_lower_of_the_phones_cap_and_the_engineers_limit() {
        let source = [i16::MIN, -16_000, 16_000, i16::MAX];
        let limited = |max_level_percent: u8, limit_percent: u8| MixValues {
            max_level_percent,
            engineer: EngineerControls {
                limit_percent,
                ..EngineerControls::default()
            },
            ..MixValues::default()
        };

        assert_eq!(
            mix_channels(&source, 2, limited(90, 25)),
            [-8191, -8191, 8191, 8191],
            "the engineer's limit is lower"
        );
        assert_eq!(
            mix_channels(&source, 2, limited(25, 90)),
            [-8191, -8191, 8191, 8191],
            "the phone's cap is lower"
        );
        let quiet = [-16_000, -8_000, 8_000, 16_000];
        assert_eq!(mix_channels(&quiet, 2, limited(100, 100)), quiet);
    }

    #[test]
    fn the_mixer_reads_the_engineers_limit_from_the_stored_mix() {
        let state = MixState::new(&GroupLayout::default());
        state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(25),
                ..EngineerUpdate::default()
            })
            .expect("valid controls apply");

        assert_eq!(
            mix_channels(&[20_000, -20_000], 2, state.snapshot()),
            [8191, -8191]
        );
    }

    #[test]
    fn kept_controls_apply_once_with_their_pairs_locked_whole() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[2], &[]));
        let before = state.current();
        let kept = EngineerControls {
            limit_percent: 60,
            locked: 0b1000,
        };

        let restored = state
            .restore_engineer(kept)
            .expect("the controls were not in force");

        assert_eq!(restored, state.current());
        assert!(restored.revision > before.revision);
        assert_eq!(
            restored.engineer,
            EngineerControls {
                limit_percent: 60,
                locked: 0b1100,
            },
            "CH 3 and CH 4 are one strip"
        );
        assert_eq!(state.restore_engineer(kept), None, "already in force");
        let capped = state
            .restore_engineer(EngineerControls {
                limit_percent: 250,
                locked: 0,
            })
            .expect("other controls");
        assert_eq!(capped.engineer.limit_percent, 100);
    }

    fn channel(gain: u8, pan: u8, muted: bool, solo: bool) -> PresetChannel {
        PresetChannel {
            gain,
            pan,
            muted,
            solo,
        }
    }

    /// A mix with its own master, cap and engineer's limit, for the copy and preset tests: they
    /// must never move those.
    fn own_master(layout: &GroupLayout) -> MixState {
        let state = MixState::new(layout);
        state
            .apply(&MixUpdate {
                group_levels: Some(vec![70, 80, 90]),
                volume_percent: Some(60),
                max_level_percent: Some(80),
                muted: Some(true),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        state
            .apply_engineer(&EngineerUpdate {
                limit_percent: Some(50),
                locked: Some(vec![true]),
            })
            .expect("valid controls apply");
        state
    }

    fn assert_own_master_kept(values: &MixValues) {
        assert_eq!(
            values.volume_percent, 60,
            "the master volume is the musician's"
        );
        assert_eq!(values.max_level_percent, 80, "so is the phone's cap");
        assert!(values.muted, "and the master mute");
        assert_eq!(
            values.engineer,
            EngineerControls {
                limit_percent: 50,
                locked: 0b1,
            },
            "the engineer's controls are never carried"
        );
    }

    #[test]
    fn a_preset_holds_each_inputs_values_and_the_group_levels_by_id_and_nothing_else() {
        let state = MixState::new(&trio());
        state
            .apply(&MixUpdate {
                channel_gains: Some(vec![40, 50, 60]),
                pans: Some(vec![10, 90, 50]),
                channel_muted: Some(vec![false, true, false]),
                channel_solo: Some(vec![false, false, true]),
                group_levels: Some(vec![10, 20, 30]),
                volume_percent: Some(55),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");

        let preset = MixPreset::of(&state.current(), &trio(), 3);

        assert_eq!(
            preset,
            MixPreset {
                channels: vec![
                    channel(40, 10, false, false),
                    channel(50, 90, true, false),
                    channel(60, 50, false, true),
                ],
                groups: vec![
                    ("drums".to_owned(), 10),
                    ("bass".to_owned(), 20),
                    ("keys".to_owned(), 30),
                ],
            },
            "one entry per input of the device, the groups by id"
        );
        assert_eq!(
            MixPreset::of(&state.current(), &trio(), 40).channels.len(),
            MAX_MIX_CHANNELS,
            "never more than the mixer holds"
        );
    }

    #[test]
    fn a_preset_lands_on_the_live_groups_by_id_after_they_moved_and_keeps_the_musicians_own() {
        // Saved for Drums, Bass and Keys; the groups are now Keys, a new Horns, then Drums.
        let preset = MixPreset {
            channels: vec![channel(40, 10, true, false), channel(50, 90, false, true)],
            groups: vec![
                ("drums".to_owned(), 10),
                ("bass".to_owned(), 20),
                ("keys".to_owned(), 30),
            ],
        };
        let live = GroupLayout::new(vec![
            group("keys", "Keys", &[2]),
            group("horns", "Horns", &[3]),
            group("drums", "Drums", &[0]),
        ]);
        let state = own_master(&live);
        let before = state.current();

        let applied = state.apply_preset(&preset, &live);

        assert_eq!(applied, state.current());
        assert!(applied.revision > before.revision);
        assert_eq!(
            &applied.group_levels[..3],
            &[30, 80, 10],
            "Keys, Horns kept, Drums"
        );
        assert_eq!(&applied.channel_gains[..3], &[40, 50, 100]);
        assert_eq!(&applied.pans[..2], &[10, 90]);
        assert!(
            applied.channel_muted[0],
            "the desk sets a locked channel too"
        );
        assert!(applied.channel_solo[1]);
        assert_eq!(
            applied.pans[2], before.pans[2],
            "an input the preset lacks keeps its own"
        );
        assert_own_master_kept(&applied);
    }

    #[test]
    fn a_preset_keeps_a_pairs_sides_equal() {
        let state = MixState::configured(&GroupLayout::default(), flags(&[0], &[]));
        let preset = MixPreset {
            channels: vec![channel(40, 0, false, false), channel(70, 100, true, false)],
            groups: Vec::new(),
        };

        let applied = state.apply_preset(&preset, &GroupLayout::default());

        assert_eq!(
            &applied.channel_gains[..2],
            &[40, 40],
            "the first side wins"
        );
        assert_eq!(&applied.channel_muted[..2], &[false, false]);
        assert_eq!(&applied.pans[..2], &[0, 100], "the balance is the preset's");
    }

    #[test]
    fn following_a_mix_copies_its_channels_and_groups_never_the_master_or_controls() {
        let leader = MixState::new(&trio());
        leader
            .apply(&MixUpdate {
                channel_gains: Some(vec![30, 40, 50]),
                pans: Some(vec![20, 80]),
                channel_muted: Some(vec![false, false, true]),
                channel_solo: Some(vec![true]),
                group_levels: Some(vec![15, 25, 35]),
                volume_percent: Some(90),
                max_level_percent: Some(95),
                ..MixUpdate::default()
            })
            .expect("a valid update applies");
        let follower = own_master(&trio());
        let before = follower.current();
        let led = leader.current();

        let followed = follower.follow(&led).expect("the mixes differed");

        assert_eq!(followed, follower.current());
        assert!(followed.revision > before.revision);
        assert_eq!(followed.channel_gains, led.channel_gains);
        assert_eq!(followed.pans, led.pans);
        assert_eq!(followed.channel_muted, led.channel_muted);
        assert_eq!(followed.channel_solo, led.channel_solo);
        assert_eq!(followed.group_levels, led.group_levels);
        assert_own_master_kept(&followed);
        assert_eq!(
            follower.follow(&led),
            None,
            "already the leader's: no new revision"
        );
        assert_eq!(follower.current().revision, followed.revision);
    }

    #[test]
    fn a_followers_phone_keeps_its_master_but_cannot_move_a_channel_or_group() {
        let state = MixState::new(&trio());
        let before = state.current();

        let started = state
            .set_following(Some(Arc::from("Bass")))
            .expect("it followed nobody");

        assert!(started.revision > before.revision, "the phone is told");
        assert_eq!(state.following().as_deref(), Some("Bass"));
        assert_eq!(state.report(), (state.current(), Some(Arc::from("Bass"))));
        assert_eq!(state.set_following(Some(Arc::from("Bass"))), None);
        let applied = state
            .apply_from_phone(&MixUpdate {
                group_levels: Some(vec![5, 5, 5]),
                ..whole_phone_mix(&[10, 20], &[30, 40], &[true, true], &[true, false])
            })
            .expect("a valid mix applies");
        assert_eq!(applied.channel_gains, before.channel_gains, "levels kept");
        assert_eq!(applied.pans, before.pans, "pans kept");
        assert_eq!(applied.channel_muted, before.channel_muted, "mutes kept");
        assert_eq!(applied.channel_solo, before.channel_solo, "solos kept");
        assert_eq!(
            applied.group_levels, before.group_levels,
            "group levels kept"
        );
        assert_eq!(applied.volume_percent, 70, "the master is the phone's");
        assert_eq!(applied.max_level_percent, 90, "and its cap");

        let stopped = state.set_following(None).expect("it followed Bass");
        assert_eq!(
            stopped.channel_gains, applied.channel_gains,
            "stopping moves nothing"
        );
        assert_eq!(state.report().1, None);
        let free = state
            .apply_from_phone(&whole_phone_mix(&[10], &[30], &[true], &[false]))
            .expect("a valid mix applies");
        assert_eq!(free.channel_gains[0], 10, "the phone is free again");
    }

    #[test]
    fn a_phones_mix_is_answered_with_the_hold_its_values_were_kept_under() {
        let state = MixState::new(&trio());
        let (_, free) = state
            .apply_from_phone_report(&whole_phone_mix(&[10], &[30], &[false], &[false]))
            .expect("a valid mix applies");
        assert_eq!(free, None);
        state.set_following(Some(Arc::from("Bass")));

        let (values, following) = state
            .apply_from_phone_report(&whole_phone_mix(&[90], &[30], &[false], &[false]))
            .expect("a valid mix applies");

        assert_eq!(following.as_deref(), Some("Bass"));
        assert_eq!(values.channel_gains[0], 10, "kept under that hold");
        assert_eq!(values, state.current());
    }
}
