import Network

/// What desk discovery is doing, as the connect screen has to say it.
public enum DiscoveryStatus: Equatable, Sendable {
    /// Not browsing: not started yet, or stopped.
    case idle
    /// Browsing; the desk list is live.
    case browsing
    /// The musician has not allowed local network access, so no desk can ever appear. On iOS the
    /// browser does not fail for this, it waits, which is why it needs a case of its own: a screen
    /// that only says "searching" would leave the musician waiting for nothing. It also shows while
    /// the system's permission prompt is up, and clears once access is granted.
    case localNetworkDenied
    /// Anything else the browser waits or fails on, described for diagnostics.
    case unavailable(reason: String)

    /// `kDNSServiceErr_PolicyDenied` (`usr/include/dns_sd.h:801` in the iOS 27 SDK): the error a
    /// browser reports when local network access is not allowed.
    static let policyDenied: DNSServiceErrorType = -65_570

    public init(_ state: NWBrowser.State) {
        switch state {
        case .setup, .cancelled:
            self = .idle
        case .ready:
            self = .browsing
        case .waiting(let error), .failed(let error):
            self = Self(error)
        @unknown default:
            self = .unavailable(reason: "\(state)")
        }
    }

    private init(_ error: NWError) {
        if case .dns(let code) = error, code == Self.policyDenied {
            self = .localNetworkDenied
        } else {
            self = .unavailable(reason: error.debugDescription)
        }
    }
}
