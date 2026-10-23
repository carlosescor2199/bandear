import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarProtocol
import BandEarScreens
import BandEarSession
import Combine
import Foundation
import UIKit

/// The session with one desk, its control connection and its audio, and the screens' own state,
/// published for the views.
///
/// Wiring only, and so with no tests of its own: the rules live in `BandEarSession`, where
/// `swift test` proves them, and every word the screens show lives in `BandEarScreens`. The
/// coordinator runs on the main actor and says when it changed; each change is copied into the
/// values a view reads. The views call the actions below and nothing else.
@MainActor
final class SessionModel: ObservableObject {
    // MARK: The session

    /// The desk the session was started with, or nil while none runs. A desk typed by hand is
    /// named "host:port" (``ConnectionWords/deskLabel(_:)``).
    @Published private(set) var desk: DiscoveredDesk?
    /// Whether a session runs, with its receiver (``MonitorAudio/isRunning``).
    @Published private(set) var isRunning = false
    @Published private(set) var connection: ControlSession.ConnectionState = .idle
    /// Why the connection was lost; see `ControlSession.connectionError`.
    @Published private(set) var connectionError: String?
    /// The desk's latest `error` reply.
    @Published private(set) var deskError: DeskError?
    /// Whether the desk acknowledged a mix and so plays it; false while this phone mixes on its own.
    @Published private(set) var mixHeldByDesk = false
    /// The mix as the musician edits it, with what the desk last reported.
    @Published private(set) var mixSession = MixSession()
    /// The name as typed, cut as the desk keeps it (``ControlSession/musicianName``).
    @Published private(set) var musicianName: String

    // MARK: The audio

    @Published private(set) var receiverStats: ReceiverStats?
    @Published private(set) var linkReport: LinkStatsReport?
    /// Underruns added up over this session's link reports.
    @Published private(set) var underrunTotal: Int64 = 0
    /// Why the receiver last refused a packet, failed to open the output, or lost its socket.
    @Published private(set) var receiverError: (any Error)?

    // MARK: The screens

    /// The tab shown while a session runs. A session start shows Mix, as Android's does.
    @Published var tab: AppTab = .mix
    /// Whether the manual address form is open over Connect; a session start closes it.
    @Published var manualAddressOpen = false
    /// The manual address as typed. Its audio port also applies to a desk found by discovery, as
    /// on Android.
    @Published var form = ConnectionForm()
    /// Why the last connect started nothing, in Android's words, or nil.
    @Published private(set) var refusal: String?

    // MARK: Kept for the provisional session screen until T8d replaces it

    /// How many strips the desk's `config` announced.
    @Published private(set) var stripCount = 0
    /// Packets of the desk's audio played this session.
    @Published private(set) var packetsPlayed: Int64 = 0
    /// Why the receiver last refused a packet or lost its output, or nil.
    @Published private(set) var audioProblem: String?
    /// Whether the audio output was reaching the musician at the last packet played; `true` until
    /// a packet says otherwise.
    @Published private(set) var outputPlaying = true

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

    // MARK: Actions

    /// Starts a session with `desk`, this phone listening for audio on the form's audio port.
    func connect(to desk: DiscoveredDesk) {
        start(form.request(for: desk), naming: desk)
    }

    /// Starts a session with the address typed in the form, once the form checks out.
    func connectManually() {
        let request = form.manualRequest()
        let desk = (try? request.get()).map {
            DiscoveredDesk(
                name: ConnectionWords.deskLabel($0.endpoint), endpoint: $0.endpoint, sampleRate: nil)
        }
        start(request, naming: desk)
    }

    /// Ends the session, if one runs.
    func disconnect() {
        guard desk != nil else { return }
        desk = nil
        audio.stop()
    }

    /// Keeps the name as typed. While a session runs, the desk hears it after a pause in typing.
    func setName(_ name: String) {
        audio.session.setMusicianName(name)
        musicianName = audio.session.musicianName
    }

    /// An edit to the mix: a level, a mute, a pan, the master, More of me, the musician's channel.
    func apply(_ event: MixEvent) {
        audio.session.apply(event)
    }

    // MARK: Plumbing

    private func start(
        _ request: Result<ConnectionRequest, ConnectionFormError>, naming desk: DiscoveredDesk?
    ) {
        // A second Connect while a session runs starts nothing, so it changes no desk either.
        guard self.desk == nil, !isRunning else { return }
        do {
            try audio.start(try request.get(), musicianName: audio.session.musicianName)
            self.desk = desk
            refusal = nil
            tab = .mix
            manualAddressOpen = false
        } catch {
            refusal = ConnectWords.problem(for: error)
        }
    }

    private func mirror() {
        let session = audio.session
        isRunning = audio.isRunning
        connection = session.connection
        connectionError = session.connectionError
        deskError = session.lastError
        mixHeldByDesk = session.mixHeldByDesk
        mixSession = session.mixSession
        if musicianName != session.musicianName { musicianName = session.musicianName }
        receiverStats = audio.receiverStats
        linkReport = audio.linkReport
        underrunTotal = audio.underrunTotal
        receiverError = audio.receiverError
        stripCount = session.mixSession.mix.strips.count
        packetsPlayed = audio.receiverStats?.packets ?? 0
        audioProblem = audio.receiverError.map { "\($0)" }
        outputPlaying = audio.receiverStats?.outputPlaying ?? true
        // A session that ended on its own, its receiver gone, takes its screen with it.
        if desk != nil, !audio.isRunning {
            desk = nil
        }
    }
}
