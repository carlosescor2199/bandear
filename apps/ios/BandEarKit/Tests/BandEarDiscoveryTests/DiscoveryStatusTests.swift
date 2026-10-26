import Network
import Testing

@testable import BandEarDiscovery

/// The browser's states, read as what the connect screen has to say. The one that matters most on
/// iOS is local network access: without it browsing never fails outright, it waits, with
/// `kDNSServiceErr_PolicyDenied` (-65570, `dns_sd.h`), and a screen that only shows "searching"
/// leaves the musician waiting for a desk that can never appear.
@Suite("Discovery status")
struct DiscoveryStatusTests {

    @Test("is browsing once the browser is ready")
    func ready() {
        #expect(DiscoveryStatus(.ready) == .browsing)
    }

    @Test("is idle before the browser starts and after it is cancelled")
    func idle() {
        #expect(DiscoveryStatus(.setup) == .idle)
        #expect(DiscoveryStatus(.cancelled) == .idle)
    }

    @Test("names a denied local network, whether the browser waits or fails on it")
    func localNetworkDenied() {
        #expect(DiscoveryStatus(.waiting(.dns(-65570))) == .localNetworkDenied)
        #expect(DiscoveryStatus(.failed(.dns(-65570))) == .localNetworkDenied)
    }

    @Test(
        "reports anything else the browser waits or fails on as unavailable",
        arguments: [
            NWBrowser.State.waiting(.dns(-65537)),
            .failed(.dns(-65540)),
            .failed(.posix(.ENETDOWN)),
            .waiting(.posix(.ENETUNREACH)),
        ])
    func unavailable(state: NWBrowser.State) {
        guard case .unavailable(let reason) = DiscoveryStatus(state) else {
            Issue.record("expected unavailable, got \(DiscoveryStatus(state))")
            return
        }
        #expect(!reason.isEmpty)
    }
}
