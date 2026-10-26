import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarSession
import Combine
import Foundation
import UIKit

/// The session with one desk, its control connection and its audio, published for the screens.
///
/// Wiring only, and so with no tests of its own: the rules live in `BandEarSession`, where
/// `swift test` proves them. The coordinator runs on the main actor and says when it changed;
/// each change is copied into the values a view reads.
@MainActor
final class SessionModel: ObservableObject {
    /// The desk the session was started with, or nil while none runs.
    @Published private(set) var desk: DiscoveredDesk?
    @Published private(set) var connection: ControlSession.ConnectionState = .idle
    /// Why the connection was lost; see `ControlSession.connectionError`.
    @Published private(set) var connectionError: String?
    /// How many strips the desk's `config` announced.
    @Published private(set) var stripCount = 0
    /// Packets of the desk's audio played this session.
    @Published private(set) var packetsPlayed: Int64 = 0
    /// Why the receiver last refused a packet or lost its output, or nil.
    @Published private(set) var audioProblem: String?
    /// Whether the audio output was reaching the musician at the last packet played; `true` until
    /// a packet says otherwise.
    @Published private(set) var outputPlaying = true
    /// Why the last connect started nothing, or nil.
    @Published private(set) var refusal: String?
    /// The name as typed. Empty connects under ``deviceName``.
    @Published var musicianName: String

    /// The name a musician who typed none connects under.
    let deviceName = UIDevice.current.name

    /// Kept for the app's life: a receiver still running holds its own thread, and only the
    /// coordinator stops it.
    private let audio: MonitorAudio

    init() {
        let session = ControlSession(
            transport: WebSocketTransport(), scheduler: MainQueueScheduler(),
            store: UserDefaultsMusicianStore(UserDefaults.standard))
        audio = MonitorAudio(session: session, sinkFactory: EngineOutputFactory())
        musicianName = session.musicianName
        // The coordinator owns the session's own callback; this is the one to observe.
        audio.stateDidChange = { [weak self] in self?.mirror() }
        mirror()
    }

    /// Starts a session with `desk`, this phone listening for audio on its default port.
    func connect(to desk: DiscoveredDesk) {
        // A second Connect while a session runs starts nothing, so it changes no desk either.
        guard self.desk == nil else { return }
        let typed = musicianName.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let request = try ConnectionForm().request(for: desk).get()
            try audio.start(request, musicianName: typed.isEmpty ? deviceName : typed)
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
        audio.stop()
    }

    private func mirror() {
        let session = audio.session
        connection = session.connection
        connectionError = session.connectionError
        stripCount = session.mixSession.mix.strips.count
        packetsPlayed = audio.receiverStats?.packets ?? 0
        audioProblem = audio.receiverError.map { "\($0)" }
        outputPlaying = audio.receiverStats?.outputPlaying ?? true
        // A session that ended on its own, its receiver gone, takes its screen with it.
        if desk != nil, !session.mixSession.isSessionRunning {
            desk = nil
        }
    }
}
