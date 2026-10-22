import BandEarDiscovery
import BandEarMix
import BandEarSession
import Combine
import Foundation
import UIKit

/// The control session with one desk, published for the screens.
///
/// Wiring only, and so with no tests of its own: the rules live in `BandEarSession`, where
/// `swift test` proves them. The session runs on the main actor and says when it changed; each
/// change is copied into the values a view reads.
@MainActor
final class SessionModel: ObservableObject {
    /// The desk the session was started with, or nil while none runs.
    @Published private(set) var desk: DiscoveredDesk?
    @Published private(set) var connection: ControlSession.ConnectionState = .idle
    /// Why the connection was lost; see `ControlSession.connectionError`.
    @Published private(set) var connectionError: String?
    /// How many strips the desk's `config` announced.
    @Published private(set) var stripCount = 0
    /// Why the last connect started nothing, or nil.
    @Published private(set) var refusal: String?
    /// The name as typed. Empty connects under ``deviceName``.
    @Published var musicianName: String

    /// The name a musician who typed none connects under.
    let deviceName = UIDevice.current.name

    private let session: ControlSession

    init() {
        session = ControlSession(
            transport: WebSocketTransport(), scheduler: MainQueueScheduler(),
            store: UserDefaultsMusicianStore(UserDefaults.standard))
        musicianName = session.musicianName
        session.stateDidChange = { [weak self] in self?.mirror() }
        mirror()
    }

    /// Starts a session with `desk`, this phone listening for audio on its default port.
    func connect(to desk: DiscoveredDesk) {
        let typed = musicianName.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let request = try ConnectionForm().request(for: desk).get()
            try session.start(request, musicianName: typed.isEmpty ? deviceName : typed)
            self.desk = desk
            refusal = nil
        } catch {
            refusal = "Could not connect: \(error)"
        }
    }

    /// Ends the session, if one runs.
    func disconnect() {
        guard desk != nil else { return }
        desk = nil
        session.stop()
    }

    private func mirror() {
        connection = session.connection
        connectionError = session.connectionError
        stripCount = session.mixSession.mix.strips.count
    }
}
