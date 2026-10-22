import BandEarDiscovery
import Combine
import Dispatch

/// The desks Bonjour finds and what discovery is doing, published for the screen.
///
/// A thin wrapper: the rules live in `BandEarDiscovery`, where they are tested. The browser calls
/// back on its own queue, so every report is handed to the main queue, in order, before it touches
/// anything a view reads.
@MainActor
final class DeskDiscoveryModel: ObservableObject {
    @Published private(set) var desks: [DiscoveredDesk] = []
    @Published private(set) var status: DiscoveryStatus = .idle

    private let browser: BonjourDeskBrowser
    /// Counts the runs, so a report already queued from a run that has since been stopped or
    /// replaced is dropped instead of overwriting what the current run says.
    private var run = 0

    init(browser: BonjourDeskBrowser = BonjourDeskBrowser()) {
        self.browser = browser
    }

    deinit {
        browser.stop()
    }

    func start() {
        run += 1
        let run = run
        browser.start(
            onDesks: { [weak self] desks in
                DispatchQueue.main.async {
                    guard let self, self.run == run else { return }
                    self.desks = desks
                }
            },
            onStatus: { [weak self] status in
                DispatchQueue.main.async {
                    guard let self, self.run == run else { return }
                    self.status = status
                }
            })
    }

    /// Stops browsing and clears what the stopped run found: once stopped, the browser reports
    /// nothing more, so the list would otherwise go stale on screen.
    func stop() {
        run += 1
        browser.stop()
        desks = []
        status = .idle
    }
}
