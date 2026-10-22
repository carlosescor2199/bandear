import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation

/// The musician's session with one desk: the control connection, and the mix it carries both ways.
///
/// Ported from the Android client's `ControlChannel.kt` and the session half of
/// `MonitorViewModel.kt`:
/// - On every open it registers first, then sends the opening mix (`ControlChannel.kt:134-146`):
///   the desk adds this musician as a target from the registration, so the mix that follows finds
///   one to join. A reopened connection does exactly the same, so the desk gets the musician's name
///   and mix back under the newest number.
/// - Every event, the desk's and the musician's, goes through
///   ``MixSession/reducedNumberingSends(by:)``, and the phone sends ``MixSession/outgoingMix()``
///   whenever that numbered a new mix (`MonitorViewModel.kt:346-348,390-394`).
/// - A `mix` or `stats` sent while the connection is not open is dropped, never queued, as Android
///   drops it (`ControlChannel.kt:405-411,436-440`): the next open sends the newest mix anyway, and
///   a report describes one interval and is worthless later.
/// - A connection that closes or fails while the session runs is opened again after
///   ``ReconnectPolicy``'s wait, to the same desk, for as long as the session runs
///   (`ControlChannel.kt:290-325`). Only ``stop()`` ends the session.
/// - What the phone remembers lives in a ``MusicianStore``, as Android's ViewModel keeps it in its
///   `MonitorStore` (`MonitorViewModel.kt:322-324,424,490,519-525`): the name and the channel are
///   read back when the session is made, the client id when a session starts, the name is written
///   when a session starts with it, and the musician's channel whenever it changes.
///
/// Everything happens on the main actor. The transport reports from its own thread, and every
/// event is handed to the main queue in the order it came, so the session's state is only touched
/// here and a screen can read it directly.
@MainActor
public final class ControlSession {
    /// Where the control connection stands.
    public enum ConnectionState: Equatable, Sendable {
        /// No session: not started, or stopped.
        case idle
        /// Started; the connection is opening.
        case connecting
        /// Open: what the musician changes reaches the desk.
        case connected
        /// The connection was lost, and the session opens it again after ``ReconnectPolicy``'s wait.
        /// It stays here until a new connection opens; ``connectionError`` says why, when it failed.
        case reconnecting
    }

    /// The mix, and how the desk's answers are matched to it. Kept across sessions, as Android keeps
    /// it: the numbers only grow, and a new session sends the mix the phone already has.
    public private(set) var mixSession = MixSession()
    public private(set) var connection: ConnectionState = .idle
    /// True once the desk acknowledged one of this session's mixes, numbered or not, readable or not:
    /// the desk then plays the musician's mix itself (Android's `mixAcknowledged`, shown as
    /// `mixHeldByServer`).
    public private(set) var mixHeldByDesk = false
    /// The cushion the desk set, for the player (T7). Every session starts on Auto, as Android's does
    /// (`MonitorViewModel.kt:254-256`): a desk that sets one says so again when the musician registers.
    public private(set) var playbackBuffer: PlaybackBufferSetting = .auto
    /// How many packets late the desk sends its redundant copies, for the player (T7); none until the
    /// desk says so (`MonitorViewModel.kt:257-258`).
    public private(set) var redundancyHoldBlocks: UInt8 = 0
    /// The desk's latest `error`, for a screen to show.
    public private(set) var lastError: DeskError?
    /// Why the connection was lost, said once per outage, as Android reports one error per outage
    /// (`ControlChannel.kt:296-318`): the reason of the outage's first failure, kept while its retries
    /// keep failing. An outage ends when a connection hears from the desk, which forgets the reason;
    /// so do ``stop()``, a new start, and the session ending on its own (``MixEvent/sessionEnded``).
    /// A desk closing the connection is no failure, so an outage that began with a close reports its
    /// first failure instead.
    public private(set) var connectionError: String?
    /// The name the musician last started a session with, as the desk stores it; remembered across
    /// launches, and empty before the first.
    public private(set) var musicianName: String
    /// Called on the main actor after anything above may have changed.
    public var stateDidChange: (() -> Void)?

    private let transport: any ControlTransport
    private let scheduler: any ControlScheduler
    private let store: any MusicianStore
    /// Who this session registers as; sent on every open.
    private var registration: Registration?
    /// The desk the session reopens its connection to.
    private var endpoint: DeskEndpoint?
    /// How many connections the session has opened, so each one's events can be told apart.
    private var opened = 0
    /// The connection whose events count, or nil when none does: before a start, after a stop, and
    /// once the current one ended. Events of any other are late and ignored, as Android ignores the
    /// callbacks of a socket that is no longer current (`ControlChannel.kt:134-149,278-289`).
    private var current: Int?
    /// Set by ``stop()``: nothing reconnects until the next start (Android's `stopped`).
    private var stopped = true
    private var reconnectPolicy = ReconnectPolicy()
    private var pendingRetry: (any ScheduledTask)?
    /// Whether the current connection has heard from the desk yet.
    private var connectionProved = false

    public init(
        transport: any ControlTransport = WebSocketTransport(),
        scheduler: any ControlScheduler = MainQueueScheduler(),
        store: any MusicianStore
    ) {
        self.transport = transport
        self.scheduler = scheduler
        self.store = store
        // What an earlier launch left, as Android restores it when the app starts
        // (`MonitorViewModel.kt:519-525`). The channel is on the mix before any `config`, which then
        // keeps it, moves it to its pair's first side, or forgets it, as it would a chosen one.
        musicianName = store.musicianName
        mixSession.moreOfMe.musicianChannel = store.musicianChannel
    }

    /// Starts a session with the desk `request` names, registering as `musicianName` with this
    /// phone's audio port and this installation's ``ClientIdentity``, and opens the connection. The
    /// name is remembered as the desk stores it. A session already running is replaced: its
    /// connection is no longer heard, and a retry it was waiting for is called off.
    ///
    /// Throws, and starts nothing, for a name the desk would store as no name
    /// (``ClientMessageError/nameIsBlank``).
    public func start(_ request: ConnectionRequest, musicianName: String) throws {
        let registration = try Registration(
            name: musicianName, udpPort: request.audioPort,
            clientID: ClientIdentity.clientID(in: store))
        self.registration = registration
        self.musicianName = registration.name
        store.musicianName = registration.name
        endpoint = request.endpoint
        // The previous connection is set aside before anything below could send: what this session
        // sends goes out once its own connection opens.
        current = nil
        connection = .connecting
        cancelPendingRetry()
        stopped = false
        reconnectPolicy.reset()
        connectionError = nil
        mixHeldByDesk = false
        playbackBuffer = .auto
        redundancyHoldBlocks = 0
        lastError = nil
        reduce(.sessionStarted)
        openConnection()
        stateDidChange?()
    }

    /// Ends the session: the connection closes, a retry it was waiting for is called off, and nothing
    /// the connection reports afterwards is heard.
    public func stop() {
        current = nil
        connection = .idle
        stopped = true
        cancelPendingRetry()
        connectionError = nil
        transport.close()
        reduce(.sessionEnded)
        stateDidChange?()
    }

    /// The musician's change.
    public func apply(_ event: MixEvent) {
        reduce(event)
        stateDidChange?()
    }

    /// Sends one link-stats report while the connection is open, and drops it otherwise.
    public func send(stats: LinkStats) {
        send(.stats(stats))
    }

    /// Opens a connection to the session's desk and makes it the one whose events count.
    private func openConnection() {
        guard let endpoint else { return }
        opened += 1
        let attempt = opened
        current = attempt
        connectionProved = false
        transport.open(endpoint) { [weak self] event in
            // The main queue runs blocks in the order they were queued, so the events keep theirs.
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.handle(event, from: attempt) }
            }
        }
    }

    /// Runs `event` through the mix, and sends the mix when that numbered a new one.
    private func reduce(_ event: MixEvent) {
        let before = mixSession
        mixSession = mixSession.reducedNumberingSends(by: event)
        if mixSession.sync.latest != before.sync.latest {
            send(.mix(mixSession.outgoingMix()))
        }
        // The musician's channel is remembered whenever it changes: chosen (the mix takes only a
        // channel the desk announced), moved to its pair's first side by a `config`, or forgotten by
        // one that no longer has it (`MonitorViewModel.kt:322-324,424`).
        if mixSession.moreOfMe.musicianChannel != before.moreOfMe.musicianChannel {
            store.musicianChannel = mixSession.moreOfMe.musicianChannel
        }
        // An outage belongs to a running session: one that ended on its own, as the player (T7) will
        // end it, has none left to report, exactly as after ``stop()`` (T6c's review, `R3-002`).
        if before.isSessionRunning, !mixSession.isSessionRunning {
            connectionError = nil
        }
    }

    private func send(_ message: ClientMessage) {
        // Encoding cannot fail: a `Double` is the only thing `JSONEncoder` refuses, and `LinkStats`
        // drops a non-finite one when it is made.
        guard connection == .connected, let json = try? message.encodedJSON() else { return }
        transport.send(String(decoding: json, as: UTF8.self))
    }

    private func handle(_ event: TransportEvent, from attempt: Int) {
        guard attempt == current else { return }
        switch event {
        case .opened:
            // Opening alone proves nothing: a desk that accepts and drops at once must still back
            // off (`ControlChannel.kt:258-261`).
            connection = .connected
            if let registration {
                send(.register(registration))
            }
            send(.mix(mixSession.outgoingMix()))
        case .received(let text):
            if !connectionProved {
                // The desk sends `config` right after the open: the connection works, so the backoff
                // starts again from its shortest wait and the outage is over
                // (`ControlChannel.kt:272-288`).
                connectionProved = true
                reconnectPolicy.reset()
                connectionError = nil
            }
            receive(text)
        case .closed:
            connectionEnded(failure: nil)
        case .failed(let reason):
            connectionEnded(failure: reason)
        }
        stateDidChange?()
    }

    /// The current connection ended, whichever way: while the session runs, the next one is opened
    /// after the policy's wait (`ControlChannel.kt:290-325`). The mix, its numbers and whether the
    /// desk holds it are left exactly as they were.
    private func connectionEnded(failure: String?) {
        current = nil
        guard
            ReconnectPolicy.shouldReconnect(
                sessionRunning: mixSession.isSessionRunning, stopped: stopped)
        else {
            connection = .idle
            return
        }
        connection = .reconnecting
        if let failure, connectionError == nil {
            connectionError = failure
        }
        let ended = opened
        pendingRetry = scheduler.schedule(after: reconnectPolicy.nextDelay()) { [weak self] in
            self?.retry(after: ended)
        }
    }

    /// Opens the connection again, unless the session has moved on since the connection numbered
    /// `ended` was lost: a new one was opened (a new start), the session was stopped, or it ended on
    /// its own. A retry already due when that happened then opens nothing (`ControlChannel.kt:228-244`).
    private func retry(after ended: Int) {
        guard ended == opened, current == nil else { return }
        pendingRetry = nil
        guard
            ReconnectPolicy.shouldReconnect(
                sessionRunning: mixSession.isSessionRunning, stopped: stopped)
        else {
            if !stopped {
                connection = .idle
                stateDidChange?()
            }
            return
        }
        openConnection()
    }

    private func cancelPendingRetry() {
        pendingRetry?.cancel()
        pendingRetry = nil
    }

    /// One message from the desk. A message this build cannot read is ignored like one it does not
    /// know: Android reports the first as an error, but the desk only ever sends what it documents,
    /// and nothing the musician could do about it would follow.
    private func receive(_ text: String) {
        guard let message = try? ServerMessage(decodingJSON: Data(text.utf8)) else { return }
        switch message {
        case .configuration(let configuration):
            reduce(.configReceived(configuration))
        case .mixAck, .unreadableMixAck:
            mixHeldByDesk = true
        case .playbackBuffer(let setting):
            playbackBuffer = setting
        case .redundancyHold(let blocks):
            redundancyHoldBlocks = blocks
        case .error(let error):
            lastError = error
        case .mixState, .unrecognized:
            break
        }
        if let event = MixEvent(reportedBy: message) {
            reduce(event)
        }
    }
}
