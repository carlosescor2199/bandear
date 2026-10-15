use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use crate::health::LatestLinkReport;
use crate::mix::{GroupLayout, MixState};
use crate::network::TargetCounters;

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

/// What a registration did: the address the target now has and, when a known client came back
/// from another IP, the address its entry moved from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Registration {
    pub(crate) address: SocketAddr,
    pub(crate) moved_from: Option<SocketAddr>,
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
    pub(crate) origin: TargetOrigin,
}

/// The live routing table: which UDP targets exist, each with its mix state and send counters.
///
/// This is the piece that makes the target list editable at runtime. The network thread reads the
/// table once per packet, the control server looks a peer's IP up in it per message, and the engine
/// adds and removes entries under a short write lock. Adding or removing one entry therefore cannot
/// disturb any other entry, and no thread has to be restarted: the same capture stream, control
/// server and discovery beacon keep running, exactly as a device switch leaves them running.
pub(crate) struct TargetRegistry {
    entries: RwLock<Vec<TargetEntry>>,
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
                origin: TargetOrigin::Configured,
            });
        }
        Ok(Self {
            entries: RwLock::new(entries),
        })
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
    pub(crate) fn register(
        &self,
        peer: IpAddr,
        udp_port: u16,
        name: Option<String>,
        client_id: Option<&str>,
        layout: &GroupLayout,
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
            return Registration {
                address,
                moved_from: Some(moved_from),
            };
        }
        if let Some(existing) = entries.iter_mut().find(|entry| entry.address.ip() == peer) {
            existing.address = address;
            existing.origin = origin;
            return Registration {
                address,
                moved_from: None,
            };
        }
        entries.push(TargetEntry {
            address,
            mix_state: Arc::new(MixState::new(layout)),
            counters: Arc::new(TargetCounters::default()),
            link_report: LatestLinkReport::default(),
            origin,
        });
        Registration {
            address,
            moved_from: None,
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
    /// The new entry starts with a neutral mix carrying the supplied group layout and zero counters.
    /// A value that cannot be resolved, or whose IP is already configured, is rejected with an
    /// error naming the value; the table is left exactly as it was.
    pub(crate) fn add(&self, value: &str, layout: &GroupLayout) -> Result<SocketAddr, String> {
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
        entries.push(TargetEntry {
            address,
            mix_state: Arc::new(MixState::new(layout)),
            counters: Arc::new(TargetCounters::default()),
            link_report: LatestLinkReport::default(),
            origin: TargetOrigin::Configured,
        });
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
}
