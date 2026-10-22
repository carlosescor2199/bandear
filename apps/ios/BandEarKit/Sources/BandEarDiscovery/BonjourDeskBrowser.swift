import Foundation
import Network

/// Browses Bonjour for desks and reports the list and the browsing status.
///
/// The adapter between `NWBrowser` and the pure ``DeskDirectory``: it maps every result set the
/// browser reports to ``BonjourRecord``s and hands the directory's list on, and maps the browser's
/// state to a ``DiscoveryStatus``. It decides nothing itself, so it stays small enough to trust
/// from its round-trip test (`BonjourDeskBrowserIntegrationTests`), which is the only way to test
/// code whose input is a network.
///
/// Both callbacks run on the browser's own serial queue; a screen hops to the main actor itself.
/// `browser` is only ever touched on that queue, which is what makes this class safe to share.
public final class BonjourDeskBrowser: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.bandear.discovery")
    private var browser: NWBrowser?

    public init() {}

    /// Starts browsing for desks, replacing any browsing already under way so the latest callbacks
    /// are the only ones that hear anything.
    public func start(
        onDesks: @escaping @Sendable ([DiscoveredDesk]) -> Void,
        onStatus: @escaping @Sendable (DiscoveryStatus) -> Void
    ) {
        queue.async { [self] in
            silenceAndCancel()
            let browser = NWBrowser(
                for: .bonjourWithTXTRecord(type: DeskDirectory.serviceType, domain: nil),
                using: NWParameters())
            browser.browseResultsChangedHandler = { results, _ in
                onDesks(DeskDirectory.desks(from: results.compactMap(BonjourRecord.init)))
            }
            browser.stateUpdateHandler = { state in
                onStatus(DiscoveryStatus(state))
            }
            self.browser = browser
            browser.start(queue: queue)
        }
    }

    /// Stops browsing. Nothing more is reported afterwards.
    public func stop() {
        queue.async { [self] in silenceAndCancel() }
    }

    /// Clears the handlers before cancelling, so the old browser's `cancelled` cannot land after a
    /// new browser already said it is browsing and make a live screen read idle.
    private func silenceAndCancel() {
        browser?.browseResultsChangedHandler = nil
        browser?.stateUpdateHandler = nil
        browser?.cancel()
        browser = nil
    }
}

extension BonjourRecord {
    /// The record a browse result carries, or nil for a result that is not a Bonjour service.
    init?(_ result: NWBrowser.Result) {
        guard case .service(let name, let type, let domain, _) = result.endpoint else { return nil }
        let txt: [String: String]?
        if case .bonjour(let record) = result.metadata {
            txt = record.dictionary
        } else {
            txt = nil
        }
        self.init(name: name, type: type, domain: domain, txt: txt)
    }
}
