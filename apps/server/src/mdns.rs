use std::time::Duration;

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo, UnregisterStatus};

use crate::discovery::DISCOVERY_VERSION;

/// The Bonjour (mDNS/DNS-SD) announcement of the desk, for iPhones.
///
/// iOS only lets an app receive UDP broadcast or multicast with an entitlement that Apple grants on
/// request to paid developer accounts, so an iPhone cannot hear the `MLNK` beacon (see
/// [crate::discovery]). Browsing Bonjour with `NWBrowser` needs only the local-network permission.
/// This announcement is additive: the beacon, the control channel and the audio stream are
/// untouched, and a phone that already uses the beacon never sees a difference.
///
/// The service is the control channel, which is TCP (a WebSocket), hence `_tcp`. The type label
/// `bandear` is at most 15 bytes, as DNS-SD requires.
pub(crate) const SERVICE_TYPE: &str = "_bandear._tcp.local.";

/// What every instance name starts with, and the whole name when the host name is unknown.
const INSTANCE_PREFIX: &str = "BandEar";

/// A DNS label, and so an instance name or a host label, is at most 63 bytes on the wire.
const MAX_LABEL_BYTES: usize = 63;

/// How long a stop waits for the daemon to confirm each of the unregister and the shutdown. The
/// engine stop must never hang on the network, so an unconfirmed step is logged and given up.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// The record set the desk announces, as plain data so it can be checked without a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceDescription {
    pub(crate) service_type: &'static str,
    /// What a phone shows in its list of desks, for example `BandEar on STUDIO-PC`.
    pub(crate) instance_name: String,
    /// The name the address records are published under. It is our own name, never the machine's
    /// `<host>.local.`: macOS and Windows already answer for that one through their own mDNS
    /// responders, and two responders claiming one name is a conflict.
    pub(crate) host_name: String,
    /// The control port: where a phone opens the WebSocket, exactly as the beacon carries it.
    pub(crate) port: u16,
    /// TXT `v` (the discovery protocol version, the beacon's) and `sr` (the sample rate in Hz).
    pub(crate) properties: Vec<(&'static str, String)>,
}

/// Describes the service for a desk on `host_name` whose control channel listens on
/// `control_port`, streaming at `sample_rate`.
///
/// Returns `None` for control port 0: the OS picks the port then, so there is no fixed port to put
/// in the record, and announcing port 0 would only send phones nowhere.
pub(crate) fn describe_service(
    host_name: Option<&str>,
    control_port: u16,
    sample_rate: u32,
) -> Option<ServiceDescription> {
    if control_port == 0 {
        return None;
    }
    let host = host_name.map(display_host).unwrap_or_default();
    Some(ServiceDescription {
        service_type: SERVICE_TYPE,
        instance_name: instance_name(&host),
        host_name: dns_host_name(&host),
        port: control_port,
        properties: vec![
            ("v", DISCOVERY_VERSION.to_string()),
            ("sr", sample_rate.to_string()),
        ],
    })
}

/// The host name as a person reads it: control characters dropped, surrounding blanks trimmed, and
/// a trailing `.local` removed (macOS's `hostname` reports `Studio-Mac.local`).
fn display_host(host_name: &str) -> String {
    let cleaned: String = host_name.chars().filter(|c| !c.is_control()).collect();
    let mut host = cleaned.trim().trim_end_matches('.');
    // `get` rather than indexing: a non-ASCII name may not have a character boundary there.
    let suffix_start = host.len().saturating_sub(6);
    if host
        .get(suffix_start..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".local"))
    {
        host = &host[..suffix_start];
    }
    host.trim().to_owned()
}

/// `BandEar on <host>`, cut to the 63-byte label limit on a character boundary, or just `BandEar`
/// when the host name is unknown.
fn instance_name(host: &str) -> String {
    if host.is_empty() {
        return INSTANCE_PREFIX.to_owned();
    }
    let full = format!("{INSTANCE_PREFIX} on {host}");
    truncate_to_bytes(&full, MAX_LABEL_BYTES)
        .trim_end()
        .to_owned()
}

/// `bandear-<host>.local.`, where the host part keeps only lowercase ASCII letters and digits with
/// single hyphens between runs, so it is a valid DNS host label whatever the machine is called.
fn dns_host_name(host: &str) -> String {
    let mut label = INSTANCE_PREFIX.to_ascii_lowercase();
    let mut pending_hyphen = true;
    for c in host.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_hyphen {
                label.push('-');
                pending_hyphen = false;
            }
            label.push(c.to_ascii_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    let label = truncate_to_bytes(&label, MAX_LABEL_BYTES).trim_end_matches('-');
    format!("{label}.local.")
}

/// The longest prefix of `text` that fits in `max` bytes without splitting a character.
fn truncate_to_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl ServiceDescription {
    /// The description as mdns-sd's service record. No address is given: with automatic addresses
    /// the daemon publishes whatever the enabled interfaces carry, and follows them as they change
    /// (a laptop moving from Wi-Fi to Ethernet keeps being found).
    pub(crate) fn to_service_info(&self) -> Result<ServiceInfo, mdns_sd::Error> {
        let info = ServiceInfo::new(
            self.service_type,
            &self.instance_name,
            &self.host_name,
            "",
            self.port,
            self.properties.as_slice(),
        )?;
        Ok(info.enable_addr_auto())
    }
}

/// The machine's host name, without any crate: `COMPUTERNAME` is always set on Windows, `HOSTNAME`
/// sometimes on Linux, and `hostname` answers on macOS and Linux. `None` when nothing does, and the
/// desk is then announced as plain `BandEar`.
fn machine_host_name() -> Option<String> {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    let output = std::process::Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// A running announcement: the daemon that answers for the service, and the service's full name to
/// unregister it by.
///
/// It is stopped by [BonjourAdvertiser::stop] or, failing that, when it is dropped, so a start that
/// fails after it was created still withdraws the record instead of leaving the daemon behind.
pub(crate) struct BonjourAdvertiser {
    daemon: Option<ServiceDaemon>,
    fullname: String,
}

/// Starts announcing the desk over Bonjour, next to the beacon and with the same control port and
/// sample rate.
///
/// Every failure here is logged and returns `None`: Bonjour is a convenience for iPhones, and the
/// engine and the beacon run exactly as before without it.
pub(crate) fn start_bonjour(control_port: u16, sample_rate: u32) -> Option<BonjourAdvertiser> {
    let Some(description) =
        describe_service(machine_host_name().as_deref(), control_port, sample_rate)
    else {
        crate::log_info!(
            "bonjour",
            "Bonjour announcement skipped: the control port is chosen by the OS, so there is no fixed port to announce"
        );
        return None;
    };
    let info = match description.to_service_info() {
        Ok(info) => info,
        Err(error) => {
            crate::log_warn!("bonjour", "Bonjour service record error: {error}");
            return None;
        }
    };
    let fullname = info.get_fullname().to_owned();
    let daemon = match ServiceDaemon::new() {
        Ok(daemon) => daemon,
        Err(error) => {
            crate::log_warn!("bonjour", "Bonjour daemon start error: {error}");
            return None;
        }
    };
    // Phones on the LAN are the audience: the loopback interfaces would only announce the desk to
    // itself.
    if let Err(error) = daemon.disable_interface(vec![IfKind::LoopbackV4, IfKind::LoopbackV6]) {
        crate::log_warn!("bonjour", "Bonjour loopback exclusion error: {error}");
    }
    if let Err(error) = daemon.register(info) {
        crate::log_warn!("bonjour", "Bonjour registration error: {error}");
        shut_down(daemon);
        return None;
    }
    crate::log_info!(
        "bonjour",
        "Announcing \"{}\" as {SERVICE_TYPE} on port {}",
        description.instance_name,
        description.port
    );
    Some(BonjourAdvertiser {
        daemon: Some(daemon),
        fullname,
    })
}

impl BonjourAdvertiser {
    /// Withdraws the record (the daemon sends the goodbye packets, so browsing phones drop the desk
    /// at once instead of when the record expires) and shuts the daemon down. Each step waits at
    /// most [STOP_TIMEOUT]; a second call does nothing.
    pub(crate) fn stop(&mut self) {
        let Some(daemon) = self.daemon.take() else {
            return;
        };
        match daemon.unregister(&self.fullname) {
            Ok(status) => match status.recv_timeout(STOP_TIMEOUT) {
                Ok(UnregisterStatus::OK) => {}
                Ok(UnregisterStatus::NotFound) => crate::log_warn!(
                    "bonjour",
                    "Bonjour unregister error: {} was not registered",
                    self.fullname
                ),
                Err(error) => crate::log_warn!("bonjour", "Bonjour unregister error: {error}"),
            },
            Err(error) => crate::log_warn!("bonjour", "Bonjour unregister error: {error}"),
        }
        shut_down(daemon);
    }
}

impl Drop for BonjourAdvertiser {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Asks the daemon thread to exit and waits at most [STOP_TIMEOUT] for it to say so.
fn shut_down(daemon: ServiceDaemon) {
    match daemon.shutdown() {
        Ok(status) => {
            if let Err(error) = status.recv_timeout(STOP_TIMEOUT) {
                crate::log_warn!("bonjour", "Bonjour daemon shutdown error: {error}");
            }
        }
        Err(error) => crate::log_warn!("bonjour", "Bonjour daemon shutdown error: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn described(host_name: Option<&str>) -> ServiceDescription {
        describe_service(host_name, 50001, 48_000).expect("a fixed control port is announced")
    }

    #[test]
    fn the_service_is_the_bandear_tcp_type_on_the_control_port() {
        let description = described(Some("STUDIO-PC"));

        assert_eq!(description.service_type, "_bandear._tcp.local.");
        assert_eq!(description.port, 50001);
    }

    #[test]
    fn the_txt_record_carries_the_discovery_version_and_the_sample_rate() {
        let description = described(Some("STUDIO-PC"));

        assert_eq!(
            description.properties,
            vec![("v", "1".to_owned()), ("sr", "48000".to_owned())]
        );
        assert_eq!(DISCOVERY_VERSION, 1);
    }

    #[test]
    fn the_instance_is_named_after_the_host() {
        let description = described(Some("STUDIO-PC"));

        assert_eq!(description.instance_name, "BandEar on STUDIO-PC");
        assert_eq!(description.host_name, "bandear-studio-pc.local.");
    }

    #[test]
    fn a_macos_local_suffix_and_stray_whitespace_are_dropped_from_the_name() {
        let description = described(Some("  Studio-Mac.local.\n"));

        assert_eq!(description.instance_name, "BandEar on Studio-Mac");
        assert_eq!(description.host_name, "bandear-studio-mac.local.");
    }

    #[test]
    fn an_unknown_or_blank_host_falls_back_to_plain_bandear() {
        for host_name in [None, Some(""), Some("   "), Some(".local")] {
            let description = described(host_name);

            assert_eq!(description.instance_name, "BandEar", "{host_name:?}");
            assert_eq!(description.host_name, "bandear.local.", "{host_name:?}");
        }
    }

    #[test]
    fn a_long_host_name_is_cut_to_the_dns_label_limit_on_a_character_boundary() {
        // "BandEar on x" is 12 bytes and every 'é' is two, so a plain byte cut at 63 would land
        // inside one: the name stops one byte short instead.
        let host = format!("x{}", "é".repeat(40));

        let description = described(Some(&host));

        assert!(description.instance_name.starts_with("BandEar on xé"));
        assert_eq!(description.instance_name.len(), 62);

        let long_ascii = described(Some(&"studio".repeat(20)));
        let label = long_ascii.host_name.trim_end_matches(".local.");
        assert_eq!(label.len(), MAX_LABEL_BYTES);
        assert!(label.starts_with("bandear-studio"));
    }

    #[test]
    fn the_host_label_keeps_only_dns_safe_characters() {
        let description = described(Some("Carlos's Mac_mini (2)"));

        assert_eq!(
            description.instance_name,
            "BandEar on Carlos's Mac_mini (2)"
        );
        assert_eq!(description.host_name, "bandear-carlos-s-mac-mini-2.local.");
    }

    #[test]
    fn an_os_chosen_control_port_is_not_announced() {
        assert_eq!(describe_service(Some("STUDIO-PC"), 0, 48_000), None);
    }

    /// The description is accepted by mdns-sd as it is, with nothing sent: building the record
    /// opens no socket.
    #[test]
    fn the_description_builds_an_mdns_sd_record_with_automatic_addresses() {
        let info = described(Some("STUDIO-PC"))
            .to_service_info()
            .expect("the record builds");

        assert_eq!(info.get_type(), "_bandear._tcp.local.");
        assert_eq!(
            info.get_fullname(),
            "BandEar on STUDIO-PC._bandear._tcp.local."
        );
        assert_eq!(info.get_hostname(), "bandear-studio-pc.local.");
        assert_eq!(info.get_port(), 50001);
        assert_eq!(info.get_property_val_str("v"), Some("1"));
        assert_eq!(info.get_property_val_str("sr"), Some("48000"));
        assert!(info.is_addr_auto());
    }
}
