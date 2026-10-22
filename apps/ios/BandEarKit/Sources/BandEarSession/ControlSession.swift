import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation

/// The musician's session with one desk: the control connection, and the mix it carries both ways.
///
/// Ported from the Android client's `ControlChannel.kt` and the session half of
/// `MonitorViewModel.kt`, without reconnecting yet (T6c):
/// - On every open it registers first, then sends the opening mix (`ControlChannel.kt:134-146`):
///   the desk adds this musician as a target from the registration, so the mix that follows finds
///   one to join.
/// - Every event, the desk's and the musician's, goes through
///   ``MixSession/reducedNumberingSends(by:)``, and the phone sends ``MixSession/outgoingMix()``
///   whenever that numbered a new mix (`MonitorViewModel.kt:346-348,390-394`).
/// - A `mix` or `stats` sent while the connection is not open is dropped, never queued, as Android
///   drops it (`ControlChannel.kt:405-411,436-440`): the next open sends the newest mix anyway, and
///   a report describes one interval and is worthless later.
/// - A socket that closes or fails does not end the session; only ``stop()`` does.
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
        /// The desk closed the connection. The session still runs.
        case disconnected
        /// The connection could not be opened or broke. The session still runs.
        case failed(reason: String)
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
    /// Called on the main actor after anything above may have changed.
    public var stateDidChange: (() -> Void)?

    private let transport: any ControlTransport
    /// Who this session registers as; sent on every open.
    private var registration: Registration?
    /// How many connections the session has opened, so each one's events can be told apart.
    private var opened = 0
    /// The connection whose events count, or nil when none does: before a start, after a stop, and
    /// once the current one ended. Events of any other are late and ignored, as Android ignores the
    /// callbacks of a socket that is no longer current (`ControlChannel.kt:134-149,278-289`).
    private var current: Int?

    public init(transport: any ControlTransport = WebSocketTransport()) {
        self.transport = transport
    }

    /// Starts a session with the desk `request` names, registering as `musicianName` with this
    /// phone's audio port and, when given, its `clientID`, and opens the connection. A session already
    /// running is replaced: its connection is no longer heard.
    ///
    /// Throws, and starts nothing, for a name the desk would store as no name
    /// (``ClientMessageError/nameIsBlank``).
    public func start(
        _ request: ConnectionRequest, musicianName: String, clientID: String? = nil
    ) throws {
        registration = try Registration(
            name: musicianName, udpPort: request.audioPort, clientID: clientID)
        mixHeldByDesk = false
        playbackBuffer = .auto
        redundancyHoldBlocks = 0
        lastError = nil
        reduce(.sessionStarted)
        connection = .connecting

        opened += 1
        let attempt = opened
        current = attempt
        transport.open(request.endpoint) { [weak self] event in
            // The main queue runs blocks in the order they were queued, so the events keep theirs.
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.handle(event, from: attempt) }
            }
        }
        stateDidChange?()
    }

    /// Ends the session: the connection closes, and nothing it reports afterwards is heard.
    public func stop() {
        current = nil
        transport.close()
        reduce(.sessionEnded)
        connection = .idle
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

    /// Runs `event` through the mix, and sends the mix when that numbered a new one.
    private func reduce(_ event: MixEvent) {
        let numbered = mixSession.sync.latest
        mixSession = mixSession.reducedNumberingSends(by: event)
        if mixSession.sync.latest != numbered {
            send(.mix(mixSession.outgoingMix()))
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
            connection = .connected
            if let registration {
                send(.register(registration))
            }
            send(.mix(mixSession.outgoingMix()))
        case .received(let text):
            receive(text)
        case .closed:
            current = nil
            connection = .disconnected
        case .failed(let reason):
            current = nil
            connection = .failed(reason: reason)
        }
        stateDidChange?()
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
