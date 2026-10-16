use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use crate::engine::OutputMeter;
use crate::health::LatestLinkReport;
use crate::mix::{EngineerControls, EngineerUpdate, GroupLayout, Membership, MixState, MixValues};
use crate::network::TargetCounters;

/// Whom the engineer's controls for a musician are kept for: the stable client id their phone
/// announced, or, for a configured target and a phone that announced none, their address (joined
/// by IP, like everything else about a musician).
///
/// The desk keeps what it knows about a musician under the same two keys, so the controls it saved
/// follow a phone to a new IP and stay with a fixed rack's address.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MusicianKey {
    Client(String),
    Address(SocketAddr),
}

/// [MusicianKey] as the registry looks controls up: an address by its IP.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum BookKey {
    Client(String),
    Ip(IpAddr),
}

impl From<&MusicianKey> for BookKey {
    fn from(key: &MusicianKey) -> Self {
        match key {
            MusicianKey::Client(id) => BookKey::Client(id.clone()),
            MusicianKey::Address(address) => BookKey::Ip(address.ip()),
        }
    }
}

/// The key a musician's row is known by: their phone's client id when it announced one, else their
/// address.
fn musician_key(entry: &TargetEntry) -> MusicianKey {
    match entry.origin.client_id() {
        Some(id) => MusicianKey::Client(id.to_owned()),
        None => MusicianKey::Address(entry.address),
    }
}

/// Whether `key` names the musician of `entry`: by their phone's client id, or an address by IP.
fn names(key: &MusicianKey, entry: &TargetEntry) -> bool {
    match key {
        MusicianKey::Client(id) => entry.origin.client_id() == Some(id.as_str()),
        MusicianKey::Address(address) => entry.address.ip() == address.ip(),
    }
}

/// A mirror the engine keeps for a follower: the musician whose mix theirs follows, and the name
/// their phone is told it follows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mirror {
    pub(crate) leader: MusicianKey,
    pub(crate) name: Arc<str>,
}

/// Whom a musician's mix follows, as the status reports it (see
/// [crate::EngineHandle::set_mirror]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MirrorStatus {
    /// The leader as the engine keeps the mirror: their phone's client id, else their address.
    pub leader: MusicianKey,
    /// The name the follower's phone is told it follows.
    pub name: String,
    /// Where the leader is now, as the status reports their row; `None` while they are not here,
    /// and then the follower keeps the last mix they took and stays held to it.
    pub leader_address: Option<SocketAddr>,
}

/// What [TargetRegistry::set_mirror] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MirrorSet {
    /// The key the mirror is kept under: the follower's client id, else their address.
    pub(crate) follower: MusicianKey,
    /// The leader the follower now follows, by the key the mirror keeps; `None` once stopped.
    pub(crate) leader: Option<MusicianKey>,
    /// The name the follower's phone is told, when it follows.
    pub(crate) name: Option<Arc<str>>,
    /// The follower's mix now stored.
    pub(crate) mix: MixValues,
    /// Every musician whose stored mix changed, the follower first, then those who follow them.
    pub(crate) changed: Vec<SocketAddr>,
}

/// The mirror kept for the musician with this client id at this IP: the one kept for the phone
/// first, then the one kept for the IP.
fn kept_in(book: &HashMap<BookKey, Mirror>, client_id: Option<&str>, ip: IpAddr) -> Option<Mirror> {
    client_id
        .and_then(|id| book.get(&BookKey::Client(id.to_owned())).cloned())
        .or_else(|| book.get(&BookKey::Ip(ip)).cloned())
}

/// Whether following mirrors from the musician `from`, leader after leader, reaches `target`
/// (named by `target_key`, and by `target_entry` when they are here): then `target` following
/// `from` would close a circle. A chain longer than the book can only go round in a circle already,
/// and counts as one.
fn reaches(
    book: &HashMap<BookKey, Mirror>,
    entries: &[TargetEntry],
    from: &MusicianKey,
    target_key: &MusicianKey,
    target_entry: Option<&TargetEntry>,
) -> bool {
    let mut key = from.clone();
    for _ in 0..=book.len() {
        if target_entry.is_some_and(|target| names(&key, target))
            || BookKey::from(&key) == BookKey::from(target_key)
        {
            return true;
        }
        // A leader who is here is resolved as a registration resolves them (client id, then IP).
        let next = match entries.iter().find(|entry| names(&key, entry)) {
            Some(entry) => kept_in(book, entry.origin.client_id(), entry.address.ip()),
            None => book.get(&BookKey::from(&key)).cloned(),
        };
        match next {
            Some(mirror) => key = mirror.leader,
            None => return false,
        }
    }
    true
}

/// The name a follower's phone is told for `leader`: `name` trimmed, else the name their phone
/// announced, else their IP.
fn mirror_name(leader: &TargetEntry, name: &str) -> Arc<str> {
    let name = name.trim();
    if !name.is_empty() {
        return Arc::from(name);
    }
    match leader.origin.name() {
        Some(announced) => Arc::from(announced),
        None => Arc::from(leader.address.ip().to_string()),
    }
}

/// A musician's key in words, for an error.
fn describe_key(key: &MusicianKey) -> String {
    match key {
        MusicianKey::Client(id) => format!("the phone {id}"),
        MusicianKey::Address(address) => address.to_string(),
    }
}

/// Why `follower` cannot follow `leader`: the leader already follows them.
fn circle_error(leader: &MusicianKey, follower: &MusicianKey) -> String {
    format!(
        "{} already follows {}'s mix, directly or through another musician: a mirror cannot go \
         round in a circle",
        describe_key(leader),
        describe_key(follower)
    )
}

/// The mirror `book` would keep for `follower` following `leader`, shown to the follower's phone as
/// `name` (trimmed; "another musician" when empty), with `entries` the musicians here: never on
/// oneself (one key, or both naming the follower here) and never closing a circle with `book`.
fn admitted(
    book: &HashMap<BookKey, Mirror>,
    entries: &[TargetEntry],
    follower: &MusicianKey,
    leader: &MusicianKey,
    name: &str,
) -> Result<Mirror, String> {
    let follower_entry = entries.iter().find(|entry| names(follower, entry));
    if BookKey::from(follower) == BookKey::from(leader)
        || follower_entry.is_some_and(|entry| names(leader, entry))
    {
        return Err(format!(
            "{} cannot follow their own mix",
            describe_key(follower)
        ));
    }
    if reaches(book, entries, leader, follower, follower_entry) {
        return Err(circle_error(leader, follower));
    }
    let name = name.trim();
    Ok(Mirror {
        leader: leader.clone(),
        name: Arc::from(if name.is_empty() {
            "another musician"
        } else {
            name
        }),
    })
}

/// An address the desk or a phone named, or the error naming it.
fn parse_address(value: &str) -> Result<SocketAddr, String> {
    value
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid target `{value}`: {error}"))
}

/// How long a registered target survives after its last control channel closes.
///
/// A phone that loses the socket for a moment (a Wi-Fi roam, a screen lock) must not lose its
/// audio: the target keeps streaming through the blip and is removed only when the channel stays
/// gone for this long. Fifteen seconds covers the reconnect blips seen in practice while a client
/// that really left stops being a target within a glance.
pub(crate) const REGISTRATION_GRACE_PERIOD: Duration = Duration::from_secs(15);

/// Where a target came from.
///
/// Only a target a client registered is removed when its control channel stays gone: a configured
/// target is the engineer's (a rack or a debugging address) and is never auto-removed.
pub(crate) enum TargetOrigin {
    Configured,
    Registered {
        /// The name the client announced, if it typed one.
        name: Option<String>,
        /// The stable id the client announced, if it sent one the server keeps. It is what lets a
        /// phone that comes back from a new IP keep this entry instead of starting a second one.
        client_id: Option<String>,
        /// When the last control channel for this target closed; `None` while connected or while
        /// the grace period has not started.
        disconnected_at: Option<Instant>,
    },
}

impl TargetOrigin {
    pub(crate) fn name(&self) -> Option<&str> {
        match self {
            TargetOrigin::Configured => None,
            TargetOrigin::Registered { name, .. } => name.as_deref(),
        }
    }

    pub(crate) fn client_id(&self) -> Option<&str> {
        match self {
            TargetOrigin::Configured => None,
            TargetOrigin::Registered { client_id, .. } => client_id.as_deref(),
        }
    }
}

/// What a registration did: the address the target now has, when a known client came back from
/// another IP the address its entry moved from, whether the registration changed the engineer's
/// controls or mirror on the musician's mix, and whether the musician's mix took the mix of the
/// leader they follow, which those who follow them in turn must then take too (see
/// [TargetRegistry::register] and [TargetRegistry::propagate]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Registration {
    pub(crate) address: SocketAddr,
    pub(crate) moved_from: Option<SocketAddr>,
    pub(crate) controls_changed: bool,
    pub(crate) followed: bool,
}

/// One configured musician destination: the UDP address plus the state that belongs to it.
///
/// The three values travel together on purpose. The network thread needs exactly this triple for
/// every packet, and keeping it in one entry removes the per-packet hash lookups (and the panics
/// that guarded them) the previous target list required.
pub(crate) struct TargetEntry {
    pub(crate) address: SocketAddr,
    pub(crate) mix_state: Arc<MixState>,
    pub(crate) counters: Arc<TargetCounters>,
    /// The latest link report the musician's phone sent. Only the control connection writes it and
    /// only the status snapshot reads it; the network thread never touches it.
    pub(crate) link_report: LatestLinkReport,
    /// The peak of the stereo mix this musician is sent. Only the network thread raises it, with
    /// lock-free atomics, and the meter read decays it.
    pub(crate) output_meter: OutputMeter,
    pub(crate) origin: TargetOrigin,
}

/// The live routing table: which UDP targets exist, each with its mix state and send counters.
///
/// This is the piece that makes the target list editable at runtime. The network thread reads the
/// table once per packet, the control server looks a peer's IP up in it per message, and the engine
/// adds and removes entries under a short write lock. Adding or removing one entry therefore cannot
/// disturb any other entry, and no thread has to be restarted: the same capture stream, control
/// server and discovery beacon keep running, exactly as a device switch leaves them running.
///
/// It also keeps the engineer's controls per musician ([EngineerControls], by [MusicianKey]), so a
/// musician whose mix is created again (back after the grace period, or from a new IP) gets them
/// back the moment they register, before their phone's first `mix` is handled. The table's lock is
/// always taken before the controls' lock, and the controls' lock is never held while a mix's
/// writer lock is taken.
///
/// It keeps the engineer's mirrors the same way ([Mirror], by the follower's [MusicianKey]): a
/// follower's mix takes their leader's channels and groups, and their phone is held off them,
/// whenever they register. The mirrors' lock follows the same rules as the controls' and is never
/// held together with it.
///
/// A follower takes its leader's mix under the propagation lock, from the read of the leader's mix
/// to the follower's store, so two changes of one leader carried on at the same moment can never
/// land in the wrong order: each copy reads the leader's mix as it is when it runs, and the last one
/// to run reads the newest. It is taken after the table's lock and before the mirrors' lock and any
/// mix's writer lock.
pub(crate) struct TargetRegistry {
    entries: RwLock<Vec<TargetEntry>>,
    controls: Mutex<HashMap<BookKey, EngineerControls>>,
    mirrors: Mutex<HashMap<BookKey, Mirror>>,
    propagation: Mutex<()>,
    /// Runs once, in the next propagation, between its read of a leader's mix and the followers
    /// taking it: lets a test hold a propagation there while another one runs.
    #[cfg(test)]
    after_leader_read: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl TargetRegistry {
    /// Builds the registry from the startup target list, rejecting duplicate IPs.
    ///
    /// [crate::cli::resolve_targets] already enforces the one-target-per-IP rule for the command
    /// line; this repeats it so a UI-built configuration cannot bypass it, because two targets that
    /// share an IP could not be addressed independently by the mix state or the control join.
    pub(crate) fn new(targets: &[SocketAddr], layout: &GroupLayout) -> Result<Self, String> {
        let mut entries = Vec::with_capacity(targets.len());
        let mut seen: HashMap<IpAddr, SocketAddr> = HashMap::with_capacity(targets.len());
        for target in targets {
            if let Some(previous) = seen.insert(target.ip(), *target) {
                return Err(format!(
                    "duplicate UDP target IP {} is not supported (targets {previous} and {target})",
                    target.ip()
                ));
            }
            entries.push(TargetEntry {
                address: *target,
                mix_state: Arc::new(MixState::new(layout)),
                counters: Arc::new(TargetCounters::default()),
                link_report: LatestLinkReport::default(),
                output_meter: OutputMeter::default(),
                origin: TargetOrigin::Configured,
            });
        }
        Ok(Self {
            entries: RwLock::new(entries),
            controls: Mutex::new(HashMap::new()),
            mirrors: Mutex::new(HashMap::new()),
            propagation: Mutex::new(()),
            #[cfg(test)]
            after_leader_read: Mutex::new(None),
        })
    }

    fn mirror_book(&self) -> std::sync::MutexGuard<'_, HashMap<BookKey, Mirror>> {
        self.mirrors.lock().expect("mirrors lock poisoned")
    }

    /// Held from the read of a leader's mix to the store of every follower's copy of it.
    fn propagation(&self) -> std::sync::MutexGuard<'_, ()> {
        self.propagation.lock().expect("propagation lock poisoned")
    }

    /// Puts the mirror kept for `entry`'s musician in force on its mix, or none when none is kept:
    /// while it follows, its phone is held off every channel and group level and is told whom it
    /// follows, and its mix takes the leader's at once when the leader is among `entries` (an absent
    /// leader leaves it on the last mix it took). `entry` may be a row about to join `entries`.
    /// Returns whether its stored mix changed, and whether it took the leader's values.
    fn restore_mirror(&self, entries: &[TargetEntry], entry: &TargetEntry) -> (bool, bool) {
        let kept = kept_in(
            &self.mirror_book(),
            entry.origin.client_id(),
            entry.address.ip(),
        );
        let named = entry
            .mix_state
            .set_following(kept.as_ref().map(|mirror| Arc::clone(&mirror.name)))
            .is_some();
        let leader = kept.and_then(|mirror| {
            entries
                .iter()
                .find(|other| !std::ptr::eq(*other, entry) && names(&mirror.leader, other))
        });
        let followed = leader.is_some_and(|leader| {
            let _order = self.propagation();
            entry
                .mix_state
                .follow(&leader.mix_state.current())
                .is_some()
        });
        (named || followed, followed)
    }

    /// The positions in `entries` of the musicians whose mirror follows `leader`'s mix.
    fn followers_of(&self, entries: &[TargetEntry], leader: &TargetEntry) -> Vec<usize> {
        let book = self.mirror_book();
        if book.is_empty() {
            return Vec::new();
        }
        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !std::ptr::eq(*entry, leader))
            .filter(|(_, entry)| {
                kept_in(&book, entry.origin.client_id(), entry.address.ip())
                    .is_some_and(|mirror| names(&mirror.leader, leader))
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Makes every musician who follows `entries[start]`'s mix, and those who follow them in turn,
    /// take it now. Returns those whose stored mix changed, in the order they took it. Each mix is
    /// visited once, so even a book that went round in a circle could not loop. It runs under the
    /// propagation lock, so it never stores a leader's mix older than one already carried on.
    fn propagate_from(&self, entries: &[TargetEntry], start: usize) -> Vec<SocketAddr> {
        let _order = self.propagation();
        let mut changed = Vec::new();
        let mut visited = vec![start];
        let mut leaders = VecDeque::from([start]);
        while let Some(leader) = leaders.pop_front() {
            let followers = self.followers_of(entries, &entries[leader]);
            if followers.is_empty() {
                continue;
            }
            let values = entries[leader].mix_state.current();
            #[cfg(test)]
            {
                // Taken first, so the hook's own lock is not held while it runs.
                let hold = self
                    .after_leader_read
                    .lock()
                    .expect("test hook lock poisoned")
                    .take();
                if let Some(hold) = hold {
                    hold();
                }
            }
            for index in followers {
                if visited.contains(&index) {
                    continue;
                }
                visited.push(index);
                if entries[index].mix_state.follow(&values).is_some() {
                    changed.push(entries[index].address);
                }
                leaders.push_back(index);
            }
        }
        changed
    }

    /// Makes every musician who follows the mix of the musician at `ip` take it now, as it changed
    /// (from their phone, the desk, a preset or a copy), and those who follow them in turn. Returns
    /// the musicians whose stored mix changed, for the caller to announce.
    pub(crate) fn propagate(&self, ip: IpAddr) -> Vec<SocketAddr> {
        if self.mirror_book().is_empty() {
            return Vec::new();
        }
        let entries = self.read();
        match entries.iter().position(|entry| entry.address.ip() == ip) {
            Some(start) => self.propagate_from(&entries, start),
            None => Vec::new(),
        }
    }

    /// Starts, changes or stops the mirror of the musician at `follower`'s IP, as
    /// [crate::EngineHandle::set_mirror] does: with `leader` (their address and the name the
    /// follower's phone is told), the follower's mix takes the leader's at once and from then on
    /// follows it, and the follower's phone is held off every channel and group level; with `None`
    /// the follower's mix stays as it is and their phone is free again. The mirror is kept under
    /// the follower's client id when they announced one (dropping any kept for their IP), else
    /// their address, so it is put in force again whenever they register.
    ///
    /// A musician cannot follow their own mix, nor a leader who already follows them, directly or
    /// through others: a mirror never goes round in a circle. An address that does not parse or is
    /// no musician here is an error naming it. Nothing changes on an error.
    pub(crate) fn set_mirror(
        &self,
        follower: &str,
        leader: Option<(&str, &str)>,
    ) -> Result<MirrorSet, String> {
        let requested = parse_address(follower)?;
        let entries = self.read();
        let index = entries
            .iter()
            .position(|entry| entry.address.ip() == requested.ip())
            .ok_or_else(|| format!("target `{follower}` is not configured"))?;
        let entry = &entries[index];
        let follower_key = musician_key(entry);
        let mirror = match leader {
            None => None,
            Some((value, name)) => {
                let wanted = parse_address(value)?;
                let leading = entries
                    .iter()
                    .find(|other| other.address.ip() == wanted.ip())
                    .ok_or_else(|| format!("target `{value}` is not configured"))?;
                if std::ptr::eq(leading, entry) {
                    return Err(format!("{follower} cannot follow their own mix"));
                }
                Some(Mirror {
                    leader: musician_key(leading),
                    name: mirror_name(leading, name),
                })
            }
        };
        {
            let mut book = self.mirror_book();
            if let Some(mirror) = &mirror {
                if reaches(&book, &entries, &mirror.leader, &follower_key, Some(entry)) {
                    return Err(circle_error(&mirror.leader, &follower_key));
                }
            }
            // Under the follower's strongest key, as their controls are: a phone's drops any
            // mirror kept for its IP, so nothing kept there stands in for this one.
            book.remove(&BookKey::Ip(entry.address.ip()));
            book.remove(&BookKey::from(&follower_key));
            if let Some(mirror) = &mirror {
                book.insert(BookKey::from(&follower_key), mirror.clone());
            }
        }
        let (moved, copied) = self.restore_mirror(&entries, entry);
        let mut changed = if moved {
            vec![entry.address]
        } else {
            Vec::new()
        };
        if copied {
            changed.extend(self.propagate_from(&entries, index));
        }
        Ok(MirrorSet {
            follower: follower_key,
            leader: mirror.as_ref().map(|mirror| mirror.leader.clone()),
            name: mirror.map(|mirror| mirror.name),
            mix: entry.mix_state.current(),
            changed,
        })
    }

    /// Keeps the mirror of the musician `follower` names on the leader `leader` names, as the
    /// desk's saved session does at start, and puts it in force for every musician here. Returns
    /// the musicians whose stored mix changed. A mirror on oneself, or one that would go round in a
    /// circle with those kept, is refused and nothing changes.
    pub(crate) fn keep_mirror(
        &self,
        follower: &MusicianKey,
        leader: &MusicianKey,
        name: &str,
    ) -> Result<Vec<SocketAddr>, String> {
        let entries = self.read();
        {
            let mut book = self.mirror_book();
            let mirror = admitted(&book, &entries, follower, leader, name)?;
            book.insert(BookKey::from(follower), mirror);
        }
        Ok(self.restore_mirrors(&entries))
    }

    /// Replaces every mirror the engine keeps with `kept` (the follower's key, the leader's key and
    /// the name the follower's phone is told), as a desk loading a saved session does: a mirror kept
    /// before and not in `kept` is forgotten, also for a musician who is not here, so it never comes
    /// back when they register. Each of `kept` is admitted in order as [Self::keep_mirror] admits one
    /// (never on oneself, never closing a circle with those admitted before it); a refused one is
    /// left out and named. Then every musician here is put on their mirror, or freed from the one
    /// they lost. Returns the musicians whose stored mix changed and the refusals.
    pub(crate) fn replace_mirrors(
        &self,
        kept: &[(MusicianKey, MusicianKey, String)],
    ) -> (Vec<SocketAddr>, Vec<String>) {
        let entries = self.read();
        let mut refused = Vec::new();
        {
            let mut book = self.mirror_book();
            book.clear();
            for (follower, leader, name) in kept {
                match admitted(&book, &entries, follower, leader, name) {
                    Ok(mirror) => {
                        book.insert(BookKey::from(follower), mirror);
                    }
                    Err(error) => refused.push(error),
                }
            }
        }
        (self.restore_mirrors(&entries), refused)
    }

    /// Puts the mirror kept for each musician of `entries` in force, or none (see
    /// [Self::restore_mirror]), resolving each client id first as a registration does, and carries
    /// each leader's mix on. Returns the musicians whose stored mix changed, each once.
    fn restore_mirrors(&self, entries: &[TargetEntry]) -> Vec<SocketAddr> {
        let mut changed: Vec<SocketAddr> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let (moved, copied) = self.restore_mirror(entries, entry);
            let mut moved_now = if moved {
                vec![entry.address]
            } else {
                Vec::new()
            };
            if copied {
                moved_now.extend(self.propagate_from(entries, index));
            }
            for address in moved_now {
                if !changed.contains(&address) {
                    changed.push(address);
                }
            }
        }
        changed
    }

    /// Whom each musician of `entries` follows, by IP, for those who follow anyone.
    pub(crate) fn mirror_statuses(&self, entries: &[TargetEntry]) -> HashMap<IpAddr, MirrorStatus> {
        let book = self.mirror_book();
        if book.is_empty() {
            return HashMap::new();
        }
        entries
            .iter()
            .filter_map(|entry| {
                let mirror = kept_in(&book, entry.origin.client_id(), entry.address.ip())?;
                let leader_address = entries
                    .iter()
                    .find(|other| !std::ptr::eq(*other, entry) && names(&mirror.leader, other))
                    .map(|leader| leader.address);
                Some((
                    entry.address.ip(),
                    MirrorStatus {
                        leader: mirror.leader,
                        name: mirror.name.to_string(),
                        leader_address,
                    },
                ))
            })
            .collect()
    }

    /// The controls kept for a musician: those kept for their phone's client id first, then those
    /// kept for their IP, as the desk resolves a musician's label.
    fn kept_controls(&self, client_id: Option<&str>, ip: IpAddr) -> Option<EngineerControls> {
        let book = self
            .controls
            .lock()
            .expect("engineer controls lock poisoned");
        client_id
            .and_then(|id| book.get(&BookKey::Client(id.to_owned())).copied())
            .or_else(|| book.get(&BookKey::Ip(ip)).copied())
    }

    /// Puts the controls kept for `entry`'s musician in force on its mix, if any are kept. Returns
    /// whether its stored mix changed.
    fn restore_kept_controls(&self, entry: &TargetEntry) -> bool {
        self.kept_controls(entry.origin.client_id(), entry.address.ip())
            .and_then(|controls| entry.mix_state.restore_engineer(controls))
            .is_some()
    }

    /// Puts the controls kept for `entry`'s musician in force on its mix, or the defaults (no
    /// limit, nothing locked) when none are kept: for a row another musician has just taken over.
    /// Returns whether its stored mix changed.
    fn replace_controls(&self, entry: &TargetEntry) -> bool {
        let controls = self
            .kept_controls(entry.origin.client_id(), entry.address.ip())
            .unwrap_or_default();
        entry.mix_state.restore_engineer(controls).is_some()
    }

    /// Changes the engineer's controls for the musician whose IP `value` names (see
    /// [crate::EngineHandle::set_engineer_controls]) and keeps them for that musician: under their
    /// phone's client id when it announced one, which also drops any kept for their IP, else under
    /// their address. Returns the key they were kept under, the musician's address and the mix now
    /// stored.
    ///
    /// The table is held for reading from the change through the keeping, so a registration cannot
    /// come between them and leave the kept controls behind the mix.
    pub(crate) fn set_controls(
        &self,
        value: &str,
        update: &EngineerUpdate,
    ) -> Result<(MusicianKey, SocketAddr, MixValues), String> {
        let requested = value
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid target `{value}`: {error}"))?;
        let entries = self.read();
        let entry = entries
            .iter()
            .find(|entry| entry.address.ip() == requested.ip())
            .ok_or_else(|| format!("target `{value}` is not configured"))?;
        let applied = entry.mix_state.apply_engineer(update)?;
        let key = match entry.origin.client_id() {
            Some(id) => MusicianKey::Client(id.to_owned()),
            None => MusicianKey::Address(entry.address),
        };
        let mut book = self
            .controls
            .lock()
            .expect("engineer controls lock poisoned");
        if matches!(key, MusicianKey::Client(_)) {
            book.remove(&BookKey::Ip(entry.address.ip()));
        }
        book.insert(BookKey::from(&key), applied.engineer);
        Ok((key, entry.address, applied))
    }

    /// Keeps `controls` for the musician `key` names, as the desk's saved session does at start,
    /// and puts them in force for that musician if they are here. Returns the addresses of the
    /// musicians whose stored mix changed.
    pub(crate) fn keep_controls(
        &self,
        key: &MusicianKey,
        controls: EngineerControls,
    ) -> Vec<SocketAddr> {
        let entries = self.read();
        self.controls
            .lock()
            .expect("engineer controls lock poisoned")
            .insert(BookKey::from(key), controls);
        // Every musician is resolved again, client id first, so controls kept for an IP never
        // override those kept for the phone that is there.
        entries
            .iter()
            .filter(|entry| self.restore_kept_controls(entry))
            .map(|entry| entry.address)
            .collect()
    }

    /// Replaces every control the engine keeps with `kept`, as a desk loading a saved session does:
    /// controls kept before and not in `kept` are forgotten, also for a musician who is not here, so
    /// they never come back when that musician registers. Every musician here is resolved again,
    /// client id first, and takes the controls now kept for them or the defaults (no limit, nothing
    /// locked). Returns the addresses of the musicians whose stored mix changed.
    pub(crate) fn replace_kept_controls(
        &self,
        kept: &[(MusicianKey, EngineerControls)],
    ) -> Vec<SocketAddr> {
        let entries = self.read();
        {
            let mut book = self
                .controls
                .lock()
                .expect("engineer controls lock poisoned");
            book.clear();
            for (key, controls) in kept {
                book.insert(BookKey::from(key), *controls);
            }
        }
        entries
            .iter()
            .filter(|entry| self.replace_controls(entry))
            .map(|entry| entry.address)
            .collect()
    }

    /// Adds or updates the target a client announced over the control channel.
    ///
    /// The address is built from the socket's peer IP and the announced UDP port; the message
    /// carries no address, so a client cannot claim another's. Re-registering the same IP updates
    /// the announced port and name, keeps the mix state and counters, and cancels any pending
    /// removal from the grace period. A configured target at the same IP is adopted by the
    /// registration, because the client that is actually there is the authority on where its own
    /// audio should go.
    ///
    /// A `client_id` that an entry at another IP already holds means that phone came back from a
    /// new address (DHCP after a Wi-Fi blip). Its entry moves here instead of a second one being
    /// created: the mix state, counters, report slot and place in the list are kept, the old
    /// address leaves the table at once, and any other entry still keyed to this IP gives way, so
    /// the table keeps one entry per IP. Without a `client_id` nothing moves. Every registration
    /// records the id it announced (or none), so a row belongs to whoever registered from its IP
    /// last. Ids are random and only ever travel on the LAN, so this adds no trust beyond the IP
    /// rule above.
    ///
    /// The engineer's controls kept for the musician (by client id, else by IP) are put in force on
    /// the mix under the same write lock, so the phone's first `mix` after it registers already
    /// meets its locks and limit. A row another musician takes over (a different client id, or
    /// none where there was one, or the other way round) gets that musician's kept controls or the
    /// defaults, never the previous occupant's. The mirror kept for the musician is put in force the
    /// same way (see [Self::set_mirror]): a follower's phone is held off their channels and groups
    /// before its first `mix`, and their mix takes their leader's when the leader is here. A leader
    /// who registers changes nobody's mix: their followers take their mix when it next changes.
    /// [Registration::controls_changed] says whether the mix changed, so the caller can tell the
    /// phone, and [Registration::followed] whether it took the leader's, so the caller can carry it
    /// on to those who follow this musician ([Self::propagate]).
    pub(crate) fn register(
        &self,
        peer: IpAddr,
        udp_port: u16,
        name: Option<String>,
        client_id: Option<&str>,
        membership: &dyn Membership,
    ) -> Registration {
        let address = SocketAddr::new(peer, udp_port);
        let origin = TargetOrigin::Registered {
            name,
            client_id: client_id.map(str::to_owned),
            disconnected_at: None,
        };
        let mut entries = self.entries.write().expect("target registry lock poisoned");
        let returning = client_id.and_then(|id| {
            entries.iter().position(|entry| {
                entry.address.ip() != peer && entry.origin.client_id() == Some(id)
            })
        });
        if let Some(mut index) = returning {
            let moved_from = entries[index].address;
            if let Some(stale) = entries.iter().position(|entry| entry.address.ip() == peer) {
                entries.remove(stale);
                if stale < index {
                    index -= 1;
                }
            }
            let entry = &mut entries[index];
            entry.address = address;
            entry.origin = origin;
            let controls_changed = self.restore_kept_controls(entry);
            let (mirror_changed, followed) = self.restore_mirror(&entries, &entries[index]);
            return Registration {
                address,
                moved_from: Some(moved_from),
                controls_changed: controls_changed || mirror_changed,
                followed,
            };
        }
        if let Some(index) = entries.iter().position(|entry| entry.address.ip() == peer) {
            // The row is reused by whoever registers from its IP. Someone else (another phone, or
            // one that announces no id where a phone was) gets the controls kept for them, or none:
            // never the previous occupant's. The mirror is always the one kept for whoever is
            // there now, or none.
            let existing = &mut entries[index];
            let same_musician = existing.origin.client_id() == client_id;
            existing.address = address;
            existing.origin = origin;
            let controls_changed = if same_musician {
                self.restore_kept_controls(existing)
            } else {
                self.replace_controls(existing)
            };
            let (mirror_changed, followed) = self.restore_mirror(&entries, &entries[index]);
            return Registration {
                address,
                moved_from: None,
                controls_changed: controls_changed || mirror_changed,
                followed,
            };
        }
        let entry = TargetEntry {
            address,
            // Created under the write lock, so it takes the membership in force now (see
            // [Membership]).
            mix_state: Arc::new(membership.new_mix()),
            counters: Arc::new(TargetCounters::default()),
            link_report: LatestLinkReport::default(),
            output_meter: OutputMeter::default(),
            origin,
        };
        let controls_changed = self.restore_kept_controls(&entry);
        let (mirror_changed, followed) = self.restore_mirror(&entries, &entry);
        entries.push(entry);
        Registration {
            address,
            moved_from: None,
            controls_changed: controls_changed || mirror_changed,
            followed,
        }
    }

    /// Marks a registered target as disconnected, starting its grace period.
    ///
    /// Called when the last control channel for the IP closes. A configured target is left alone:
    /// the engineer owns it and it is never auto-removed.
    pub(crate) fn mark_disconnected(&self, ip: IpAddr, now: Instant) {
        let mut entries = self.entries.write().expect("target registry lock poisoned");
        for entry in entries.iter_mut() {
            if entry.address.ip() != ip {
                continue;
            }
            if let TargetOrigin::Registered {
                disconnected_at, ..
            } = &mut entry.origin
            {
                *disconnected_at = Some(now);
            }
        }
    }

    /// Removes registered targets whose grace period has elapsed, returning what was removed.
    ///
    /// Called from the control server's accept loop, which already ticks a few times a second, so
    /// the removal happens without a thread of its own. Configured targets and registered targets
    /// that reconnected (or re-registered) are kept.
    pub(crate) fn sweep_expired(&self, now: Instant) -> Vec<SocketAddr> {
        let mut entries = self.entries.write().expect("target registry lock poisoned");
        let mut removed = Vec::new();
        entries.retain(|entry| {
            if let TargetOrigin::Registered {
                disconnected_at: Some(at),
                ..
            } = entry.origin
            {
                if now.duration_since(at) >= REGISTRATION_GRACE_PERIOD {
                    removed.push(entry.address);
                    return false;
                }
            }
            true
        });
        removed
    }

    /// A read view of the current table. Callers iterate it; no entry is ever mutated in place.
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, Vec<TargetEntry>> {
        self.entries.read().expect("target registry lock poisoned")
    }

    /// Resolves and validates `value`, then adds it as a new target.
    ///
    /// The new entry starts with a neutral mix carrying the group membership in force (see
    /// [Membership]), the engineer's controls and the mirror kept for its IP (a follower takes their
    /// leader's mix at once when the leader is here), and zero counters. A value that cannot be
    /// resolved, or whose IP is already configured, is rejected with an error naming the value; the
    /// table is left exactly as it was.
    pub(crate) fn add(
        &self,
        value: &str,
        membership: &dyn Membership,
    ) -> Result<SocketAddr, String> {
        let address = resolve_target(value)?;
        let mut entries = self.entries.write().expect("target registry lock poisoned");
        if let Some(existing) = entries
            .iter()
            .find(|entry| entry.address.ip() == address.ip())
        {
            return Err(format!(
                "duplicate UDP target IP {} is not supported (target `{value}` is already configured as {})",
                address.ip(),
                existing.address
            ));
        }
        let entry = TargetEntry {
            address,
            mix_state: Arc::new(membership.new_mix()),
            counters: Arc::new(TargetCounters::default()),
            link_report: LatestLinkReport::default(),
            output_meter: OutputMeter::default(),
            origin: TargetOrigin::Configured,
        };
        self.restore_kept_controls(&entry);
        self.restore_mirror(&entries, &entry);
        entries.push(entry);
        Ok(address)
    }

    /// Removes the target whose IP matches `value`.
    ///
    /// `value` is the address string the engine reports, so a UI can hand back what it displayed.
    /// The error names the value when it is not a socket address or not configured. Removal drops
    /// only this entry: the other targets keep their mix states, their counters and their streams.
    pub(crate) fn remove(&self, value: &str) -> Result<SocketAddr, String> {
        let address = value
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid target `{value}`: {error}"))?;
        let mut entries = self.entries.write().expect("target registry lock poisoned");
        let Some(index) = entries
            .iter()
            .position(|entry| entry.address.ip() == address.ip())
        else {
            return Err(format!("target `{value}` is not configured"));
        };
        Ok(entries.remove(index).address)
    }

    /// The mix state for a control peer's IP, or `None` when that IP is no longer a target.
    ///
    /// This is the join the control server uses: a peer whose target was removed gets `None`, which
    /// is the same answer an IP that was never configured has always received.
    pub(crate) fn mix_state(&self, ip: IpAddr) -> Option<Arc<MixState>> {
        self.read()
            .iter()
            .find(|entry| entry.address.ip() == ip)
            .map(|entry| Arc::clone(&entry.mix_state))
    }

    /// The latest-link-report slot for a control peer's IP, or `None` when that IP is not a target.
    ///
    /// The same join as [Self::mix_state], so a `stats` report and a `mix` from the same peer always
    /// land on the same musician, and an IP with no target is refused the same way.
    pub(crate) fn link_report(&self, ip: IpAddr) -> Option<LatestLinkReport> {
        self.read()
            .iter()
            .find(|entry| entry.address.ip() == ip)
            .map(|entry| Arc::clone(&entry.link_report))
    }

    /// Forgets the latest link report for `ip`, leaving the slot itself, the mix state and the
    /// counters exactly as they were. An IP with no target is a no-op.
    ///
    /// A report describes the control session that sent it. When the phone comes back on a new
    /// session, the old report says nothing about the new link: kept, it would go stale while the
    /// new session is connected and show a lost link until the first new report, and forever for a
    /// phone build that never reports. Cleared, the musician starts as unknown until the phone
    /// speaks for itself.
    pub(crate) fn clear_link_report(&self, ip: IpAddr) {
        if let Some(slot) = self.link_report(ip) {
            *slot.lock().expect("link report lock poisoned") = None;
        }
    }
}

/// Resolves one target value to a socket address, naming the value in every error.
///
/// The runtime counterpart of the startup resolution: the UI hands over what the engineer typed,
/// and an unresolvable value must be reported back with the value itself so a typo is easy to spot.
pub(crate) fn resolve_target(value: &str) -> Result<SocketAddr, String> {
    value
        .to_socket_addrs()
        .map_err(|error| format!("invalid target `{value}`: {error}"))?
        .next()
        .ok_or_else(|| format!("invalid target `{value}`: it resolved to no address"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::Group;
    use std::sync::atomic::Ordering;

    fn address(value: &str) -> SocketAddr {
        value.parse().expect("test address should parse")
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("test IP should parse")
    }

    fn addresses(registry: &TargetRegistry) -> Vec<SocketAddr> {
        registry.read().iter().map(|entry| entry.address).collect()
    }

    #[test]
    fn adds_a_resolved_target_and_keeps_the_existing_ones() {
        let registry =
            TargetRegistry::new(&[address("192.168.1.3:50000")], &GroupLayout::default())
                .expect("registry should build");
        let existing = registry
            .mix_state(ip("192.168.1.3"))
            .expect("existing target");

        let added = registry
            .add("192.168.1.9:50000", &GroupLayout::default())
            .expect("a fresh address should be accepted");

        assert_eq!(added, address("192.168.1.9:50000"));
        assert_eq!(
            addresses(&registry),
            vec![address("192.168.1.3:50000"), address("192.168.1.9:50000")]
        );
        // The existing target's state is the same allocation: adding did not rebuild the table.
        assert!(Arc::ptr_eq(
            &existing,
            &registry
                .mix_state(ip("192.168.1.3"))
                .expect("existing target")
        ));
        assert!(registry.mix_state(ip("192.168.1.9")).is_some());
    }

    #[test]
    fn rejects_a_duplicate_ip_naming_the_value() {
        let registry =
            TargetRegistry::new(&[address("192.168.1.3:50000")], &GroupLayout::default())
                .expect("registry should build");

        let error = registry
            .add("192.168.1.3:50001", &GroupLayout::default())
            .expect_err("a duplicate IP must be rejected");

        assert!(
            error.contains("192.168.1.3:50001"),
            "message should name the value: {error}"
        );
        assert!(
            error.contains("192.168.1.3"),
            "message should name the IP: {error}"
        );
        assert_eq!(addresses(&registry).len(), 1);
    }

    #[test]
    fn rejects_an_unresolvable_address_naming_the_value() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");

        let error = registry
            .add("no-such-host.invalid:50000", &GroupLayout::default())
            .expect_err("an unresolvable host must be rejected");

        assert!(
            error.contains("no-such-host.invalid:50000"),
            "message should name the value: {error}"
        );
        assert!(addresses(&registry).is_empty());
    }

    #[test]
    fn removes_only_the_named_target_and_leaves_the_others_untouched() {
        let registry = TargetRegistry::new(
            &[address("192.168.1.3:50000"), address("192.168.1.4:50000")],
            &GroupLayout::default(),
        )
        .expect("registry should build");
        let kept_mix = registry.mix_state(ip("192.168.1.4")).expect("kept target");
        let kept_counters = {
            let entries = registry.read();
            let kept = entries
                .iter()
                .find(|entry| entry.address.ip() == ip("192.168.1.4"))
                .expect("kept target");
            kept.counters.sent.store(7, Ordering::Relaxed);
            Arc::clone(&kept.counters)
        };

        let removed = registry
            .remove("192.168.1.3:50000")
            .expect("configured target should be removed");

        assert_eq!(removed, address("192.168.1.3:50000"));
        assert_eq!(addresses(&registry), vec![address("192.168.1.4:50000")]);
        assert!(registry.mix_state(ip("192.168.1.3")).is_none());
        assert!(Arc::ptr_eq(
            &kept_mix,
            &registry.mix_state(ip("192.168.1.4")).expect("kept target")
        ));
        assert_eq!(kept_counters.sent.load(Ordering::Relaxed), 7);
        assert!(Arc::ptr_eq(&kept_counters, &{
            let entries = registry.read();
            let kept = entries
                .iter()
                .find(|entry| entry.address.ip() == ip("192.168.1.4"))
                .expect("kept target");
            Arc::clone(&kept.counters)
        }));
    }

    #[test]
    fn removing_an_address_that_is_not_configured_names_the_value() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");

        let error = registry
            .remove("192.168.1.9:50000")
            .expect_err("an unknown address must be rejected");

        assert!(
            error.contains("192.168.1.9:50000"),
            "message should name the value: {error}"
        );
    }

    #[test]
    fn construction_rejects_a_duplicate_ip_like_startup_validation() {
        let targets = [address("192.168.1.3:50000"), address("192.168.1.3:50001")];

        let error = match TargetRegistry::new(&targets, &GroupLayout::default()) {
            Ok(_) => panic!("duplicate IPs must be rejected"),
            Err(error) => error,
        };

        assert!(
            error.contains("192.168.1.3"),
            "message names the IP: {error}"
        );
    }

    #[test]
    fn an_added_target_carries_the_group_layout_it_was_added_with() {
        let layout = GroupLayout::new(vec![Group {
            id: "1".to_owned(),
            name: "Drums".to_owned(),
            channels: vec![0],
        }]);
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");

        registry
            .add("192.168.1.9:50000", &layout)
            .expect("a fresh address should be accepted");

        let state = registry.mix_state(ip("192.168.1.9")).expect("added target");
        assert_eq!(state.snapshot().channel_group[0], Some(0));
    }

    #[test]
    fn resolve_target_names_the_value_when_it_cannot_be_resolved() {
        let error = resolve_target("not-an-address").expect_err("a bare value must be rejected");

        assert!(
            error.contains("not-an-address"),
            "message should name the value: {error}"
        );
    }

    fn registered_name(registry: &TargetRegistry, address: IpAddr) -> Option<String> {
        registry
            .read()
            .iter()
            .find(|entry| entry.address.ip() == address)
            .and_then(|entry| entry.origin.name().map(str::to_owned))
    }

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    fn controls_at(registry: &TargetRegistry, at: &str) -> EngineerControls {
        registry
            .mix_state(ip(at))
            .expect("a musician at that IP")
            .current()
            .engineer
    }

    fn limit(limit_percent: u8) -> EngineerControls {
        EngineerControls {
            limit_percent,
            ..EngineerControls::default()
        }
    }

    #[test]
    fn controls_kept_for_a_phone_win_over_those_kept_for_its_ip() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.keep_controls(
            &MusicianKey::Address(address("192.168.1.4:50000")),
            limit(30),
        );
        registry.keep_controls(&MusicianKey::Client(PHONE.to_owned()), limit(80));

        registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(PHONE),
            &GroupLayout::default(),
        );
        registry.register(
            ip("192.168.1.5"),
            50000,
            None,
            None,
            &GroupLayout::default(),
        );

        assert_eq!(controls_at(&registry, "192.168.1.4"), limit(80));
        assert_eq!(
            controls_at(&registry, "192.168.1.5"),
            EngineerControls::default(),
            "nothing is kept for this one"
        );
    }

    #[test]
    fn setting_a_phones_controls_keeps_them_for_its_client_id_and_drops_those_for_its_ip() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.keep_controls(
            &MusicianKey::Address(address("192.168.1.4:50000")),
            limit(30),
        );
        registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(PHONE),
            &GroupLayout::default(),
        );
        assert_eq!(
            controls_at(&registry, "192.168.1.4"),
            limit(30),
            "with nothing kept for the phone, those kept for its IP apply"
        );

        let (key, at, applied) = registry
            .set_controls(
                "192.168.1.4:50000",
                &EngineerUpdate {
                    limit_percent: Some(80),
                    ..EngineerUpdate::default()
                },
            )
            .expect("the phone's controls are set");

        assert_eq!(key, MusicianKey::Client(PHONE.to_owned()));
        assert_eq!(at, address("192.168.1.4:50000"));
        assert_eq!(applied.engineer, limit(80));
        // The phone leaves and a rack is added at its old IP: nothing is kept for that IP now.
        registry
            .remove("192.168.1.4:50000")
            .expect("the phone's row is removed");
        registry
            .add("192.168.1.4:50000", &GroupLayout::default())
            .expect("a fresh address is accepted");
        assert_eq!(
            controls_at(&registry, "192.168.1.4"),
            EngineerControls::default()
        );
    }

    const OTHER_PHONE: &str = "7d1e2f3a-4b5c-4d6e-8f9a-0b1c2d3e4f5a";

    /// A row reused by someone else (another phone at the same IP, or a phone that announces no
    /// id) never keeps the previous occupant's limit and locks: it takes those kept for the new
    /// musician, or none. The phone that comes back gets its own again.
    #[test]
    fn a_different_phone_at_a_reused_address_gets_its_own_controls_or_none() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(PHONE),
            &GroupLayout::default(),
        );
        let (_, _, set) = registry
            .set_controls(
                "192.168.1.4:50000",
                &EngineerUpdate {
                    limit_percent: Some(60),
                    locked: Some(vec![true]),
                },
            )
            .expect("the phone's controls are set");
        let kept = set.engineer;

        let other = registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(OTHER_PHONE),
            &GroupLayout::default(),
        );
        assert!(other.controls_changed, "the other phone's mix lost them");
        assert_eq!(
            controls_at(&registry, "192.168.1.4"),
            EngineerControls::default(),
            "another phone inherits nothing"
        );

        let back = registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(PHONE),
            &GroupLayout::default(),
        );
        assert!(back.controls_changed);
        assert_eq!(
            controls_at(&registry, "192.168.1.4"),
            kept,
            "the phone gets its own back"
        );
        let again = registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            Some(PHONE),
            &GroupLayout::default(),
        );
        assert!(!again.controls_changed, "a rename changes nothing");

        registry.register(
            ip("192.168.1.4"),
            50000,
            None,
            None,
            &GroupLayout::default(),
        );
        assert_eq!(
            controls_at(&registry, "192.168.1.4"),
            EngineerControls::default(),
            "nor does a phone that announces no id"
        );
    }

    #[test]
    fn a_target_added_where_controls_were_kept_starts_with_them() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        let changed = registry.keep_controls(
            &MusicianKey::Address(address("192.168.1.9:50000")),
            limit(45),
        );
        assert!(changed.is_empty(), "nobody is there yet");

        registry
            .add("192.168.1.9:50000", &GroupLayout::default())
            .expect("a fresh address is accepted");

        assert_eq!(controls_at(&registry, "192.168.1.9"), limit(45));
    }

    #[test]
    fn a_registration_adds_a_target_at_the_socket_ip_with_the_announced_port() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");

        let added = registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );

        assert_eq!(added.address, address("192.168.1.50:50000"));
        assert_eq!(added.moved_from, None);
        assert_eq!(addresses(&registry), vec![address("192.168.1.50:50000")]);
        assert_eq!(
            registered_name(&registry, ip("192.168.1.50")).as_deref(),
            Some("Ana")
        );
    }

    #[test]
    fn a_registration_cannot_claim_an_address_other_than_the_socket_ip() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        // The claimed address is not part of the wire shape; if a client sends one it is ignored
        // and the target is built from the socket's IP.
        let command = crate::protocol::parse_register_command(
            r#"{"type":"register","name":"Mallory","udp_port":50000,"address":"10.0.0.99:50000","ip":"10.0.0.99"}"#,
        )
        .expect("the extra fields must be ignored");

        let added = registry.register(
            ip("192.168.1.50"),
            command.udp_port,
            Some("Mallory".to_owned()),
            None,
            &GroupLayout::default(),
        );

        assert_eq!(added.address, address("192.168.1.50:50000"));
        assert_eq!(addresses(&registry), vec![address("192.168.1.50:50000")]);
        assert!(
            registry
                .read()
                .iter()
                .all(|entry| entry.address.ip() != ip("10.0.0.99")),
            "the claimed address must not become a target"
        );
    }

    #[test]
    fn a_registered_target_survives_a_brief_disconnect() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let now = Instant::now();

        registry.mark_disconnected(ip("192.168.1.50"), now);

        // Inside the grace period the target is still there, so the musician's audio is not cut.
        assert!(registry
            .sweep_expired(now + REGISTRATION_GRACE_PERIOD - Duration::from_millis(1))
            .is_empty());
        assert_eq!(addresses(&registry), vec![address("192.168.1.50:50000")]);
    }

    #[test]
    fn a_registered_target_is_removed_after_the_grace_period() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let now = Instant::now();

        registry.mark_disconnected(ip("192.168.1.50"), now);

        let removed = registry.sweep_expired(now + REGISTRATION_GRACE_PERIOD);

        assert_eq!(removed, vec![address("192.168.1.50:50000")]);
        assert!(addresses(&registry).is_empty());
    }

    #[test]
    fn re_registration_during_the_grace_period_cancels_the_removal_and_keeps_the_mix() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let mix = registry
            .mix_state(ip("192.168.1.50"))
            .expect("registered target");
        registry.mark_disconnected(ip("192.168.1.50"), Instant::now());

        // The client comes back before the grace expires, announcing a new listening port.
        let updated = registry.register(
            ip("192.168.1.50"),
            50123,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );

        assert_eq!(updated.address, address("192.168.1.50:50123"));
        assert!(Arc::ptr_eq(
            &mix,
            &registry.mix_state(ip("192.168.1.50")).expect("kept target")
        ));
        // Long after the original grace would have expired, the cancelled removal never fires.
        assert!(registry
            .sweep_expired(Instant::now() + REGISTRATION_GRACE_PERIOD * 2)
            .is_empty());
        assert_eq!(addresses(&registry), vec![address("192.168.1.50:50123")]);
    }

    #[test]
    fn re_registration_with_a_new_name_updates_it_and_keeps_the_target_and_its_mix() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let mix = registry
            .mix_state(ip("192.168.1.50"))
            .expect("registered target");
        // A live mix the musician set before renaming: the re-registration must not rebuild it.
        mix.update(crate::mix::MixValues {
            volume_percent: 42,
            ..Default::default()
        });

        // The musician edits their name while streaming: same socket, same UDP port, new name.
        let updated = registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Bea".to_owned()),
            None,
            &GroupLayout::default(),
        );

        assert_eq!(updated.address, address("192.168.1.50:50000"));
        assert_eq!(
            registered_name(&registry, ip("192.168.1.50")).as_deref(),
            Some("Bea"),
            "a re-registration must replace the stored name"
        );
        assert!(Arc::ptr_eq(
            &mix,
            &registry.mix_state(ip("192.168.1.50")).expect("kept target")
        ));
        assert_eq!(mix.snapshot().volume_percent, 42);
    }

    fn stored_report(received_at: Instant) -> crate::health::LinkReport {
        crate::health::LinkReport {
            interval_ms: 1_000,
            packets_received: 200,
            packets_lost: 1,
            underruns: 0,
            receive_timeouts: 0,
            received_at,
        }
    }

    #[test]
    fn a_link_report_is_looked_up_by_the_target_ip_and_unknown_ips_have_none() {
        let registry =
            TargetRegistry::new(&[address("192.168.1.3:50000")], &GroupLayout::default())
                .expect("registry should build");

        let slot = registry
            .link_report(ip("192.168.1.3"))
            .expect("a configured target has a report slot");

        assert!(slot.lock().expect("report lock").is_none(), "no report yet");
        assert!(registry.link_report(ip("192.168.1.9")).is_none());
    }

    /// The registry itself keeps the report across a registration, so a rename on the same session
    /// does not blank the light; it is the control session that clears it when a new one begins.
    #[test]
    fn a_link_report_survives_re_registration_like_the_counters() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let slot = registry
            .link_report(ip("192.168.1.50"))
            .expect("registered target");
        let report = stored_report(Instant::now());
        *slot.lock().expect("report lock") = Some(report);

        // The phone reconnects and re-registers on a new port with a new name.
        registry.register(
            ip("192.168.1.50"),
            50123,
            Some("Bea".to_owned()),
            None,
            &GroupLayout::default(),
        );

        let kept = registry
            .link_report(ip("192.168.1.50"))
            .expect("re-registered target");
        assert!(Arc::ptr_eq(&slot, &kept), "the slot is the same allocation");
        assert_eq!(*kept.lock().expect("report lock"), Some(report));
    }

    #[test]
    fn clearing_a_link_report_forgets_only_the_report() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        registry.register(
            ip("192.168.1.50"),
            50000,
            Some("Ana".to_owned()),
            None,
            &GroupLayout::default(),
        );
        let slot = registry
            .link_report(ip("192.168.1.50"))
            .expect("registered target");
        *slot.lock().expect("report lock") = Some(stored_report(Instant::now()));
        let mix = registry
            .mix_state(ip("192.168.1.50"))
            .expect("registered target");
        let counters = Arc::clone(&registry.read()[0].counters);

        registry.clear_link_report(ip("192.168.1.50"));
        // An IP with no target has nothing to clear and must not disturb the others.
        registry.clear_link_report(ip("192.168.1.9"));

        assert!(
            slot.lock().expect("report lock").is_none(),
            "the report is forgotten"
        );
        let entries = registry.read();
        assert_eq!(entries.len(), 1);
        assert!(Arc::ptr_eq(&slot, &entries[0].link_report), "same slot");
        assert!(Arc::ptr_eq(&mix, &entries[0].mix_state), "mix kept");
        assert!(
            Arc::ptr_eq(&counters, &entries[0].counters),
            "counters kept"
        );
        assert_eq!(entries[0].origin.name(), Some("Ana"));
    }

    #[test]
    fn a_configured_target_is_never_removed_by_the_grace_period() {
        let registry =
            TargetRegistry::new(&[address("192.168.1.50:50000")], &GroupLayout::default())
                .expect("registry should build");
        let now = Instant::now();

        registry.mark_disconnected(ip("192.168.1.50"), now);

        assert!(registry
            .sweep_expired(now + REGISTRATION_GRACE_PERIOD * 10)
            .is_empty());
        assert_eq!(addresses(&registry), vec![address("192.168.1.50:50000")]);
    }

    const PHONE_A: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const PHONE_B: &str = "7d2e4c1a-9b3f-4e6d-8a5c-1f0e2d3c4b5a";

    /// Registers a phone from `peer` the way the control channel does, with an optional client id.
    fn register_phone(
        registry: &TargetRegistry,
        peer: &str,
        udp_port: u16,
        name: &str,
        client_id: Option<&str>,
    ) -> Registration {
        registry.register(
            ip(peer),
            udp_port,
            Some(name.to_owned()),
            client_id,
            &GroupLayout::default(),
        )
    }

    /// The live case: Wi-Fi comes back and DHCP hands the phone a new IP. The phone announces the
    /// same client id, so its row follows it: one row at the new address with the same mix, counters
    /// and report slot, and the old address gone at once instead of lingering as a lost musician
    /// until its grace period ends.
    #[test]
    fn a_known_client_id_from_a_new_ip_moves_the_musicians_row() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        let mix = registry
            .mix_state(ip("192.168.1.10"))
            .expect("registered target");
        mix.update(crate::mix::MixValues {
            volume_percent: 42,
            ..Default::default()
        });
        let (counters, report) = {
            let entries = registry.read();
            entries[0].counters.sent.store(7, Ordering::Relaxed);
            (
                Arc::clone(&entries[0].counters),
                Arc::clone(&entries[0].link_report),
            )
        };
        // The old connection may already have closed, starting the grace period.
        let now = Instant::now();
        registry.mark_disconnected(ip("192.168.1.10"), now);

        let moved = register_phone(&registry, "192.168.1.3", 50001, "Ana", Some(PHONE_A));

        assert_eq!(
            moved,
            Registration {
                address: address("192.168.1.3:50001"),
                moved_from: Some(address("192.168.1.10:50000")),
                controls_changed: false,
                followed: false,
            }
        );
        assert_eq!(addresses(&registry), vec![address("192.168.1.3:50001")]);
        assert!(
            registry.mix_state(ip("192.168.1.10")).is_none(),
            "the old address is gone at once"
        );
        assert!(Arc::ptr_eq(
            &mix,
            &registry.mix_state(ip("192.168.1.3")).expect("moved target")
        ));
        assert_eq!(mix.snapshot().volume_percent, 42);
        {
            let entries = registry.read();
            assert!(
                Arc::ptr_eq(&counters, &entries[0].counters),
                "counters kept"
            );
            assert_eq!(entries[0].counters.sent.load(Ordering::Relaxed), 7);
            assert!(
                Arc::ptr_eq(&report, &entries[0].link_report),
                "the report slot travels with the row; the control session decides whether to clear it"
            );
            assert_eq!(entries[0].origin.name(), Some("Ana"));
            assert_eq!(entries[0].origin.client_id(), Some(PHONE_A));
        }
        // The move is a live registration: the grace period that had started no longer runs.
        assert!(registry
            .sweep_expired(now + REGISTRATION_GRACE_PERIOD * 2)
            .is_empty());
    }

    /// Without an id nothing changes: a new IP is a new musician, even while another row (with or
    /// without an id) waits out its grace period.
    #[test]
    fn without_a_client_id_a_new_ip_is_a_separate_row() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.10", 50000, "Ana", None);
        register_phone(&registry, "192.168.1.11", 50000, "Eva", Some(PHONE_A));

        let added = register_phone(&registry, "192.168.1.3", 50000, "Ana", None);

        assert_eq!(added.moved_from, None);
        assert_eq!(
            addresses(&registry),
            vec![
                address("192.168.1.10:50000"),
                address("192.168.1.11:50000"),
                address("192.168.1.3:50000"),
            ]
        );
    }

    /// After a move, the old IP's leftover connection (a half-open socket the heartbeat closes
    /// later) ends and marks its IP disconnected. That IP has no row any more, so the moved musician
    /// stays connected and is never removed by a grace period it did not earn.
    #[test]
    fn the_old_ip_disconnecting_after_a_move_leaves_the_moved_row_alone() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        register_phone(&registry, "192.168.1.3", 50000, "Ana", Some(PHONE_A));
        let now = Instant::now();

        registry.mark_disconnected(ip("192.168.1.10"), now);

        assert!(
            matches!(
                registry.read()[0].origin,
                TargetOrigin::Registered {
                    disconnected_at: None,
                    ..
                }
            ),
            "the moved row is still connected"
        );
        assert!(registry
            .sweep_expired(now + REGISTRATION_GRACE_PERIOD * 2)
            .is_empty());
        assert_eq!(addresses(&registry), vec![address("192.168.1.3:50000")]);
    }

    #[test]
    fn two_client_ids_keep_two_rows_and_only_the_returning_one_moves() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        register_phone(&registry, "192.168.1.11", 50000, "Bea", Some(PHONE_B));
        let bea_mix = registry.mix_state(ip("192.168.1.11")).expect("Bea's row");
        assert_eq!(addresses(&registry).len(), 2, "two ids, two rows");

        let moved = register_phone(&registry, "192.168.1.3", 50000, "Ana", Some(PHONE_A));

        assert_eq!(moved.moved_from, Some(address("192.168.1.10:50000")));
        // The moved row keeps its place in the list, so the desktop does not reorder musicians.
        assert_eq!(
            addresses(&registry),
            vec![address("192.168.1.3:50000"), address("192.168.1.11:50000")]
        );
        assert!(Arc::ptr_eq(
            &bea_mix,
            &registry.mix_state(ip("192.168.1.11")).expect("Bea's row")
        ));
        assert_eq!(
            registered_name(&registry, ip("192.168.1.11")).as_deref(),
            Some("Bea")
        );
    }

    /// One row per IP is what every lookup relies on. When a returning phone lands on an IP that a
    /// stale row still holds (another phone's, waiting out its grace period), that row gives way: the
    /// phone actually at the IP is the authority on it, as it is over a configured target.
    #[test]
    fn a_move_onto_an_ip_a_stale_row_still_holds_replaces_that_row() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.3", 50000, "Bea", Some(PHONE_B));
        registry.mark_disconnected(ip("192.168.1.3"), Instant::now());
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        let ana_mix = registry.mix_state(ip("192.168.1.10")).expect("Ana's row");

        let moved = register_phone(&registry, "192.168.1.3", 50001, "Ana", Some(PHONE_A));

        assert_eq!(
            moved,
            Registration {
                address: address("192.168.1.3:50001"),
                moved_from: Some(address("192.168.1.10:50000")),
                controls_changed: false,
                followed: false,
            }
        );
        assert_eq!(addresses(&registry), vec![address("192.168.1.3:50001")]);
        assert!(Arc::ptr_eq(
            &ana_mix,
            &registry.mix_state(ip("192.168.1.3")).expect("Ana's row")
        ));
        assert_eq!(registry.read()[0].origin.client_id(), Some(PHONE_A));
    }

    /// A registration records the id it announced, or the lack of one. Whoever registers from an IP
    /// owns its row, so a phone that left and later comes back from elsewhere cannot pull the row
    /// away from the client that took over its old IP.
    #[test]
    fn a_registration_records_the_client_id_it_announced() {
        let registry = TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry");
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));

        // An older app build (no id) is handed the same IP and takes the row over, as today.
        register_phone(&registry, "192.168.1.10", 50000, "Eva", None);
        assert_eq!(registry.read()[0].origin.client_id(), None);

        // The first phone returns from another IP: its id names no row now, so it starts its own.
        let added = register_phone(&registry, "192.168.1.3", 50000, "Ana", Some(PHONE_A));

        assert_eq!(added.moved_from, None);
        assert_eq!(
            addresses(&registry),
            vec![address("192.168.1.10:50000"), address("192.168.1.3:50000")]
        );
        assert_eq!(
            registered_name(&registry, ip("192.168.1.10")).as_deref(),
            Some("Eva")
        );
    }

    /// Three configured racks, A, B and C, for the mirror tests.
    fn racks() -> TargetRegistry {
        TargetRegistry::new(
            &[
                address("192.168.1.3:50000"),
                address("192.168.1.4:50000"),
                address("192.168.1.5:50000"),
            ],
            &GroupLayout::default(),
        )
        .expect("registry should build")
    }

    const A: &str = "192.168.1.3:50000";
    const B: &str = "192.168.1.4:50000";
    const C: &str = "192.168.1.5:50000";

    fn mix_at(registry: &TargetRegistry, at: &str) -> Arc<MixState> {
        registry
            .mix_state(address(at).ip())
            .expect("a musician is there")
    }

    /// Moves the first channels of the musician at `at` from the desk.
    fn desk_gains(registry: &TargetRegistry, at: &str, gains: &[i32]) {
        mix_at(registry, at)
            .apply(&crate::mix::MixUpdate {
                channel_gains: Some(gains.to_vec()),
                group_levels: Some(vec![gains[0]]),
                ..crate::mix::MixUpdate::default()
            })
            .expect("a valid update applies");
    }

    fn gains_at(registry: &TargetRegistry, at: &str, count: usize) -> Vec<u8> {
        mix_at(registry, at).current().channel_gains[..count].to_vec()
    }

    #[test]
    fn starting_a_mirror_copies_the_leaders_mix_and_names_the_leader_to_the_follower() {
        let registry = racks();
        desk_gains(&registry, B, &[30, 40]);

        let set = registry
            .set_mirror(A, Some((B, "Bass")))
            .expect("A can follow B");

        assert_eq!(set.follower, MusicianKey::Address(address(A)));
        assert_eq!(set.leader, Some(MusicianKey::Address(address(B))));
        assert_eq!(set.name.as_deref(), Some("Bass"));
        assert_eq!(set.changed, vec![address(A)]);
        assert_eq!(set.mix, mix_at(&registry, A).current());
        assert_eq!(gains_at(&registry, A, 2), [30, 40]);
        assert_eq!(set.mix.group_levels[0], 30, "the group levels too");
        assert_eq!(mix_at(&registry, A).following().as_deref(), Some("Bass"));
        let statuses = registry.mirror_statuses(&registry.read());
        assert_eq!(
            statuses.get(&ip("192.168.1.3")),
            Some(&MirrorStatus {
                leader: MusicianKey::Address(address(B)),
                name: "Bass".to_owned(),
                leader_address: Some(address(B)),
            })
        );
        assert_eq!(statuses.len(), 1, "only the follower follows anyone");
    }

    #[test]
    fn a_mirror_never_goes_round_in_a_circle() {
        let registry = racks();
        registry.set_mirror(A, Some((B, "B"))).expect("A follows B");
        registry.set_mirror(B, Some((C, "C"))).expect("B follows C");

        for (follower, leader) in [(B, A), (C, A), (C, B), (A, A)] {
            let refused = registry
                .set_mirror(follower, Some((leader, "x")))
                .expect_err("a circle is refused");
            assert!(
                refused.contains("circle") || refused.contains("own"),
                "{follower} following {leader}: {refused}"
            );
        }
        let unknown = registry
            .set_mirror(A, Some(("192.168.1.9:50000", "x")))
            .expect_err("an unknown leader is refused");
        assert!(unknown.contains("not configured"), "{unknown}");
        assert_eq!(
            mix_at(&registry, A).following().as_deref(),
            Some("B"),
            "a refusal changes nothing"
        );
        assert_eq!(mix_at(&registry, C).following(), None);
    }

    #[test]
    fn every_change_of_a_leaders_mix_reaches_its_followers_and_theirs() {
        let registry = racks();
        registry.set_mirror(A, Some((B, "B"))).expect("A follows B");
        registry.set_mirror(C, Some((A, "A"))).expect("C follows A");

        desk_gains(&registry, B, &[11, 22]);
        let moved = registry.propagate(ip("192.168.1.4"));

        assert_eq!(moved, vec![address(A), address(C)]);
        assert_eq!(gains_at(&registry, A, 2), [11, 22]);
        assert_eq!(
            gains_at(&registry, C, 2),
            [11, 22],
            "a follower of a follower too"
        );
        assert!(
            registry.propagate(ip("192.168.1.4")).is_empty(),
            "nothing left to take"
        );
    }

    #[test]
    fn stopping_a_mirror_leaves_the_followers_mix_as_it_is_and_frees_it() {
        let registry = racks();
        desk_gains(&registry, B, &[30, 40]);
        registry.set_mirror(A, Some((B, "B"))).expect("A follows B");

        let stopped = registry.set_mirror(A, None).expect("A stops following");

        assert_eq!(stopped.leader, None);
        assert_eq!(stopped.name, None);
        assert_eq!(stopped.changed, vec![address(A)], "the phone is told");
        assert_eq!(
            gains_at(&registry, A, 2),
            [30, 40],
            "the mix stays as it was"
        );
        assert_eq!(mix_at(&registry, A).following(), None);
        desk_gains(&registry, B, &[50, 60]);
        assert!(registry.propagate(ip("192.168.1.4")).is_empty());
        assert_eq!(gains_at(&registry, A, 2), [30, 40]);
        assert!(registry.mirror_statuses(&registry.read()).is_empty());
    }

    #[test]
    fn a_followers_mirror_is_kept_for_its_phone_and_put_in_force_when_it_registers() {
        let registry = racks();
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        let set = registry
            .set_mirror("192.168.1.10:50000", Some((B, "Bass")))
            .expect("Ana follows B");
        assert_eq!(set.follower, MusicianKey::Client(PHONE_A.to_owned()));
        let gone = Instant::now();
        registry.mark_disconnected(ip("192.168.1.10"), gone);
        registry.sweep_expired(gone + REGISTRATION_GRACE_PERIOD * 2);
        desk_gains(&registry, B, &[25, 35]);

        let back = register_phone(&registry, "192.168.1.11", 50000, "Ana", Some(PHONE_A));

        assert!(back.controls_changed, "the phone is told whom it follows");
        assert!(back.followed, "its mix took the leader's");
        let mix = mix_at(&registry, "192.168.1.11:50000");
        assert_eq!(mix.following().as_deref(), Some("Bass"));
        assert_eq!(gains_at(&registry, "192.168.1.11:50000", 2), [25, 35]);
        let held = mix
            .apply_from_phone(&crate::mix::MixUpdate {
                channel_gains: Some(vec![90, 90]),
                volume_percent: Some(40),
                ..crate::mix::MixUpdate::default()
            })
            .expect("a valid mix applies");
        assert_eq!(
            &held.channel_gains[..2],
            &[25, 35],
            "its phone cannot move them"
        );
        assert_eq!(held.volume_percent, 40);
    }

    #[test]
    fn an_absent_leader_keeps_its_follower_on_the_last_mix_and_its_return_changes_nothing() {
        let registry = racks();
        register_phone(&registry, "192.168.1.20", 50000, "Bass", Some(PHONE_B));
        desk_gains(&registry, "192.168.1.20:50000", &[30, 40]);
        registry
            .set_mirror(A, Some(("192.168.1.20:50000", "")))
            .expect("A follows the bass player");
        let gone = Instant::now();
        registry.mark_disconnected(ip("192.168.1.20"), gone);
        registry.sweep_expired(gone + REGISTRATION_GRACE_PERIOD * 2);

        assert_eq!(
            registry
                .mirror_statuses(&registry.read())
                .get(&ip("192.168.1.3")),
            Some(&MirrorStatus {
                leader: MusicianKey::Client(PHONE_B.to_owned()),
                name: "Bass".to_owned(),
                leader_address: None,
            }),
            "named as the phone announced itself, and away"
        );
        assert_eq!(gains_at(&registry, A, 2), [30, 40]);
        assert_eq!(mix_at(&registry, A).following().as_deref(), Some("Bass"));

        let back = register_phone(&registry, "192.168.1.21", 50000, "Bass", Some(PHONE_B));
        assert!(!back.followed);
        assert_eq!(
            gains_at(&registry, A, 2),
            [30, 40],
            "a new mix is not copied"
        );
        desk_gains(&registry, "192.168.1.21:50000", &[70, 80]);
        assert_eq!(registry.propagate(ip("192.168.1.21")), vec![address(A)]);
        assert_eq!(gains_at(&registry, A, 2), [70, 80]);
    }

    /// Two changes of a leader's mix at nearly the same moment, each carried on to the follower:
    /// the first propagation is held right after it read the leader's mix while the second change
    /// and its propagation run. The follower must end with the leader's newest mix, never with the
    /// older one landing after it.
    #[test]
    fn two_leader_changes_at_once_leave_the_follower_with_the_newest_mix() {
        let registry = Arc::new(racks());
        registry.set_mirror(A, Some((B, "B"))).expect("A follows B");
        let (parked, held) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel::<()>();
        *registry.after_leader_read.lock().expect("test hook lock") = Some(Box::new(move || {
            parked.send(()).expect("the test waits for the hold");
            // Held until the second propagation is done, or for as long as it could take.
            let _ = finished.recv_timeout(Duration::from_millis(300));
        }));
        desk_gains(&registry, B, &[11, 11]);

        let first = {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || registry.propagate(ip("192.168.1.4")))
        };
        held.recv_timeout(Duration::from_secs(5))
            .expect("the first propagation read the leader's mix");
        let second = {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || {
                desk_gains(&registry, B, &[22, 22]);
                let moved = registry.propagate(ip("192.168.1.4"));
                let _ = done.send(());
                moved
            })
        };
        first.join().expect("the first propagation ends");
        second.join().expect("the second propagation ends");

        assert_eq!(
            gains_at(&registry, A, 2),
            [22, 22],
            "the leader's newest mix, never the older one after it"
        );
    }

    #[test]
    fn another_musician_taking_a_followers_row_is_never_held_by_its_mirror() {
        let registry = racks();
        desk_gains(&registry, B, &[30, 40]);
        register_phone(&registry, "192.168.1.10", 50000, "Ana", Some(PHONE_A));
        registry
            .set_mirror("192.168.1.10:50000", Some((B, "B")))
            .expect("Ana follows B");

        let taken = register_phone(&registry, "192.168.1.10", 50000, "Eva", Some(PHONE_B));

        assert!(
            taken.controls_changed,
            "Eva's phone is told it follows nobody"
        );
        assert!(!taken.followed);
        let mix = mix_at(&registry, "192.168.1.10:50000");
        assert_eq!(mix.following(), None);
        let free = mix
            .apply_from_phone(&crate::mix::MixUpdate {
                channel_gains: Some(vec![12]),
                ..crate::mix::MixUpdate::default()
            })
            .expect("a valid mix applies");
        assert_eq!(free.channel_gains[0], 12, "Eva's phone moves her channels");
        assert!(registry.mirror_statuses(&registry.read()).is_empty());
        let back = register_phone(&registry, "192.168.1.11", 50000, "Ana", Some(PHONE_A));
        assert!(back.followed, "Ana's mirror is still kept for her phone");
        assert_eq!(gains_at(&registry, "192.168.1.11:50000", 2), [30, 40]);
    }

    #[test]
    fn a_target_added_where_a_mirror_is_kept_follows_its_leader_at_once() {
        let registry = racks();
        desk_gains(&registry, B, &[33, 44]);
        registry
            .keep_mirror(
                &MusicianKey::Address(address("192.168.1.7:50000")),
                &MusicianKey::Address(address(B)),
                "Bass",
            )
            .expect("kept for a rack not added yet");

        registry
            .add("192.168.1.7:50000", &GroupLayout::default())
            .expect("a fresh address");

        let mix = mix_at(&registry, "192.168.1.7:50000");
        assert_eq!(mix.following().as_deref(), Some("Bass"));
        assert_eq!(gains_at(&registry, "192.168.1.7:50000", 2), [33, 44]);
        desk_gains(&registry, B, &[55, 66]);
        assert_eq!(
            registry.propagate(ip("192.168.1.4")),
            vec![address("192.168.1.7:50000")]
        );
    }

    #[test]
    fn kept_mirrors_refuse_two_addresses_on_one_ip_as_one_musician() {
        let registry = racks();

        let refused = registry
            .keep_mirror(
                &MusicianKey::Address(address("192.168.1.9:50000")),
                &MusicianKey::Address(address("192.168.1.9:50001")),
                "Rack",
            )
            .expect_err("the engine keeps an address by its IP");

        assert!(refused.contains("own mix"), "{refused}");
    }

    #[test]
    fn kept_mirrors_reach_the_followers_here_and_refuse_a_circle() {
        let registry = racks();
        desk_gains(&registry, B, &[30, 40]);
        let key = |at: &str| MusicianKey::Address(address(at));

        let moved = registry
            .keep_mirror(&key(A), &key(B), "Bass")
            .expect("A follows B");

        assert_eq!(moved, vec![address(A)]);
        assert_eq!(gains_at(&registry, A, 2), [30, 40]);
        assert_eq!(mix_at(&registry, A).following().as_deref(), Some("Bass"));
        assert!(registry.keep_mirror(&key(B), &key(A), "A").is_err());
        assert!(registry.keep_mirror(&key(C), &key(C), "C").is_err());
        assert_eq!(mix_at(&registry, B).following(), None);
        let later = MusicianKey::Client(PHONE_A.to_owned());
        assert_eq!(
            registry.keep_mirror(&later, &key(A), "A"),
            Ok(Vec::new()),
            "kept for a phone that is not here yet"
        );
        register_phone(&registry, "192.168.1.30", 50000, "Eva", Some(PHONE_A));
        assert_eq!(gains_at(&registry, "192.168.1.30:50000", 2), [30, 40]);
    }

    #[test]
    fn replacing_the_kept_controls_forgets_those_dropped_also_for_a_phone_not_here() {
        let registry = racks();
        let locked = EngineerControls {
            limit_percent: 100,
            locked: 0b1,
        };
        registry.keep_controls(&MusicianKey::Client(PHONE_A.to_owned()), locked);
        registry.keep_controls(&MusicianKey::Address(address(A)), limit(40));
        assert_eq!(controls_at(&registry, "192.168.1.3"), limit(40));

        let changed = registry.replace_kept_controls(&[
            (MusicianKey::Address(address(B)), limit(70)),
            (MusicianKey::Client(PHONE_B.to_owned()), limit(55)),
        ]);

        assert_eq!(
            changed,
            vec![address(A), address(B)],
            "A is freed, B limited"
        );
        assert_eq!(
            controls_at(&registry, "192.168.1.3"),
            EngineerControls::default()
        );
        assert_eq!(controls_at(&registry, "192.168.1.4"), limit(70));
        register_phone(&registry, "192.168.1.30", 50000, "Ana", Some(PHONE_A));
        assert_eq!(
            controls_at(&registry, "192.168.1.30"),
            EngineerControls::default(),
            "the lock dropped while Ana's phone was away does not come back"
        );
        register_phone(&registry, "192.168.1.31", 50000, "Eva", Some(PHONE_B));
        assert_eq!(
            controls_at(&registry, "192.168.1.31"),
            limit(55),
            "a phone away gets the loaded controls when it registers"
        );
    }

    #[test]
    fn replacing_the_kept_mirrors_frees_a_follower_here_and_forgets_one_not_here() {
        let registry = racks();
        desk_gains(&registry, B, &[30, 40]);
        desk_gains(&registry, C, &[70, 80]);
        let key = |at: &str| MusicianKey::Address(address(at));
        registry
            .keep_mirror(&key(A), &key(B), "Bass")
            .expect("A follows B");
        registry
            .keep_mirror(&MusicianKey::Client(PHONE_A.to_owned()), &key(B), "Bass")
            .expect("kept for a phone not here");

        let (changed, refused) = registry.replace_mirrors(&[
            (key(C), key(B), "Bass".to_owned()),
            (key(B), key(C), "Keys".to_owned()),
            (
                MusicianKey::Client(PHONE_B.to_owned()),
                key(C),
                "Keys".to_owned(),
            ),
        ]);

        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].contains("circle"), "{refused:?}");
        assert_eq!(changed, vec![address(A), address(C)]);
        assert_eq!(mix_at(&registry, A).following(), None, "A is freed");
        assert_eq!(
            gains_at(&registry, A, 2),
            [30, 40],
            "and keeps the mix it had"
        );
        assert_eq!(mix_at(&registry, C).following().as_deref(), Some("Bass"));
        assert_eq!(gains_at(&registry, C, 2), [30, 40]);
        let back = register_phone(&registry, "192.168.1.30", 50000, "Ana", Some(PHONE_A));
        assert!(
            !back.followed,
            "the mirror dropped while Ana was away does not come back"
        );
        assert_eq!(mix_at(&registry, "192.168.1.30:50000").following(), None);
        let eva = register_phone(&registry, "192.168.1.31", 50000, "Eva", Some(PHONE_B));
        assert!(
            eva.followed,
            "a phone away is put on the loaded mirror when it registers"
        );
        assert_eq!(gains_at(&registry, "192.168.1.31:50000", 2), [30, 40]);
    }
}
