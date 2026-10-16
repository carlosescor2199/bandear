use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

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
            // Membership and hidden channels are server configuration, never client input:
            // preserve whatever the source snapshot carried.
            channel_group: current.channel_group,
            hidden_channels: current.hidden_channels,
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
    pub volume_percent: u8,
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
            volume_percent: AtomicU8::new(100),
            max_level_percent: AtomicU8::new(100),
            muted: AtomicBool::new(false),
            writer: Mutex::new(revision),
            published: AtomicU64::new(revision),
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

impl MixState {
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
    pub(crate) fn set_channel_flags(&self, flags: ChannelFlags) -> Option<MixValues> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        let newly_linked = flags.linked & !self.linked.load(Ordering::Relaxed);
        self.hidden.store(flags.hidden, Ordering::Relaxed);
        self.linked.store(flags.linked, Ordering::Relaxed);
        if newly_linked == 0 {
            return None;
        }
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
            self.apply_locked(&update, &mut revision)
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
        *revision = next_revision();
        self.published.store(*revision, Ordering::Release);
        *revision
    }

    /// Applies a partial update and returns the mix that is now stored, with its new revision.
    ///
    /// The read, merge and store happen under the writer lock, so the phone and the desktop can
    /// both change different parts of the same mix without one undoing the other: the last writer
    /// wins for the fields it carries, and nothing else. An update that cannot be applied leaves
    /// the mix exactly as it was, revision included.
    pub(crate) fn apply(&self, update: &MixUpdate) -> Result<MixValues, String> {
        let mut revision = self.writer.lock().expect("mix writer lock poisoned");
        self.apply_locked(update, &mut revision)
    }

    /// [MixState::apply] for a caller that holds the writer lock and passes its revision.
    ///
    /// Every linked pair leaves it with both sides equal (see [keep_pairs_equal]); the pairs are
    /// read under the lock, so a writer never merges against pairs an edit is replacing.
    fn apply_locked(&self, update: &MixUpdate, revision: &mut u64) -> Result<MixValues, String> {
        let current = self.snapshot();
        let mut merged = update.merge_into(current)?;
        keep_pairs_equal(&current, &mut merged, self.linked.load(Ordering::Relaxed));
        merged.revision = self.store(merged, revision);
        Ok(merged)
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

fn clamp_sample(value: f32, ceiling: f32) -> i16 {
    value
        .clamp(-ceiling, ceiling)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

/// Mixes one client's interleaved source buffer down to a stereo bus.
///
/// Every source channel contributes to both outputs weighted by its pan, so source channel `k`
/// is no longer bound to output slot `k`. The default pan table keeps the captured layout (even
/// sources hard left, odd sources hard right), which with neutral gains on a two-channel source
/// reproduces the pre-pan output exactly, apart from the full-scale ceiling clamp that also
/// applied before. Master volume, mute and the ceiling are applied after the sum.
///
/// A channel contributes only when it is not hidden, not muted and either nothing is soloed or it
/// is one of the soloed channels. Solo is applied here rather than by zeroing gains, so coming out
/// of solo restores exactly the levels the musician had set. A hidden channel's solo counts for
/// nothing: a spare input the engineer hid cannot leave a musician hearing nothing. Like the rest of
/// the mix, the hidden channels are one value read from `values`, so this stays free of locks.
pub(crate) fn mix_channels(source: &[i16], channels: usize, values: MixValues) -> Vec<i16> {
    if channels == 0 || source.len() < channels {
        return source.to_vec();
    }

    let frames = source.len() / channels;
    let active = channels.min(MAX_MIX_CHANNELS);
    let audible = |channel: usize| values.hidden_channels & (1 << channel) == 0;
    let any_solo = (0..active).any(|channel| values.channel_solo[channel] && audible(channel));
    let ceiling = (i32::from(i16::MAX) * i32::from(values.max_level_percent) / 100) as f32;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_mix_command;
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
}
