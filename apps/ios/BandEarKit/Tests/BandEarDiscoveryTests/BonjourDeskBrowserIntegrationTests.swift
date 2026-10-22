import Foundation
import Network
import Testing

@testable import BandEarDiscovery

/// A real round trip through Bonjour: announce a desk from this process with `NWListener`, find it
/// with ``BonjourDeskBrowser``, stop announcing, and watch it go.
///
/// Opt-in (`BANDEAR_BONJOUR_IT=1`), because it announces a service on the local network for a few
/// seconds and depends on that network being there. The second half is the evidence behind
/// ``DeskDirectory`` having no staleness timer: a desk that stops announcing must leave the list on
/// its own, through the browser.
@Suite(
    "Bonjour desk browser (network)",
    .enabled(if: ProcessInfo.processInfo.environment["BANDEAR_BONJOUR_IT"] == "1"))
struct BonjourDeskBrowserIntegrationTests {

    @Test("finds a desk announced on this network, and loses it when the desk stops")
    func roundTrip() throws {
        let name = "BandEar IT \(UUID().uuidString.prefix(8))"
        let seen = Observed()

        let listener = try NWListener(using: .tcp)
        listener.service = NWListener.Service(
            name: name, type: DeskDirectory.serviceType,
            txtRecord: NWTXTRecord(["v": "1", "sr": "48000"]))
        listener.newConnectionHandler = { $0.cancel() }
        listener.stateUpdateHandler = { seen.note("listener \($0)") }
        listener.start(queue: DispatchQueue(label: "bandear.it.listener"))

        let browser = BonjourDeskBrowser()
        browser.start(onDesks: { seen.desks = $0 }, onStatus: { seen.note("browser \($0)") })
        defer { browser.stop() }

        let found = waitUntil(seconds: 10) { seen.desks.contains { $0.name == name } }
        #expect(found, "never saw \(name); events: \(seen.events)")
        guard found, let desk = seen.desks.first(where: { $0.name == name }) else {
            listener.cancel()
            return
        }
        #expect(desk.sampleRate == 48_000)
        guard case .bonjour(let foundName, let type, let domain) = desk.endpoint else {
            Issue.record("expected a Bonjour endpoint, got \(desk.endpoint)")
            listener.cancel()
            return
        }
        #expect(foundName == name)
        // Printed rather than asserted: this is how the browser spells them, recorded for the code
        // that opens the connection.
        print("bonjour endpoint as browsed: type=\(type.debugDescription) domain=\(domain.debugDescription)")

        listener.cancel()
        let lost = waitUntil(seconds: 10) { !seen.desks.contains { $0.name == name } }
        #expect(lost, "the desk stayed listed after it stopped announcing; events: \(seen.events)")
    }
}

/// What the callbacks reported, read from the test thread.
private final class Observed: @unchecked Sendable {
    private let lock = NSLock()
    private var storedDesks: [DiscoveredDesk] = []
    private var storedEvents: [String] = []

    var desks: [DiscoveredDesk] {
        get { lock.lock(); defer { lock.unlock() }; return storedDesks }
        set { lock.lock(); defer { lock.unlock() }; storedDesks = newValue }
    }

    var events: [String] {
        lock.lock(); defer { lock.unlock() }
        return storedEvents
    }

    func note(_ event: String) {
        lock.lock(); defer { lock.unlock() }
        storedEvents.append(event)
    }
}

/// Polls `condition` until it holds or `seconds` pass.
private func waitUntil(seconds: Double, _ condition: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if condition() { return true }
        Thread.sleep(forTimeInterval: 0.05)
    }
    return condition()
}
