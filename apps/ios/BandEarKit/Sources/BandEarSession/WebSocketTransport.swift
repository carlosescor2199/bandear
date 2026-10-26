import BandEarDiscovery
import Foundation
import Network

/// The control connection over a WebSocket, through `NWConnection` with `NWProtocolWebSocket`.
///
/// The thin adapter between Network and ``ControlTransport``: it opens, sends text, reads text and
/// says how the connection ended, and decides nothing else, so it stays small enough to trust from
/// its round trip (`WebSocketTransportIntegrationTests`), the only way to test code whose input is a
/// network. The desk accepts a WebSocket at any path with no subprotocol
/// (`apps/server/src/control.rs:661`), so none is asked for.
///
/// A desk found over Bonjour is reached by its service: Network resolves its address and control
/// port when the connection opens. A desk typed by hand is reached by a `ws://` URL (see
/// ``networkEndpoint(_:)``). The desk pings every 2 s and drops a client silent for 10
/// (`control.rs:42,48`), so pings are answered by Network itself (`autoReplyPing`).
///
/// Two clocks of its own end a connection that would otherwise hang, as OkHttp's do on Android:
/// - A connection not open within ``defaultConnectTimeout`` is given up and reported failed. Network
///   would otherwise keep preparing for good: a Bonjour desk that is no longer announced stays
///   unresolved until it is announced again, and a host dropping SYNs waits for the system's TCP
///   timeout or longer.
/// - The phone pings an open connection every ``defaultPingInterval``, and a ping whose pong has not
///   come back by the next one fails it (`ControlTransport.kt:12-19`), so a link that went silent
///   with no close (Wi-Fi gone) is noticed within two intervals rather than never.
///
/// Everything happens on the transport's own serial queue, `connection` and both clocks included,
/// which is what makes this class safe to share; events are reported from that queue. Nothing a
/// connection or a clock holds keeps the transport alive.
public final class WebSocketTransport: ControlTransport, @unchecked Sendable {
    /// How long a connection may take to open, TCP and the WebSocket handshake together, before it
    /// is given up: OkHttp's default connect timeout, which the Android client keeps
    /// (`ControlTransport.kt:77-79` sets none).
    public static let defaultConnectTimeout: Duration = .seconds(10)
    /// How often the phone pings an open connection, inside the desk's ten-second silence limit
    /// (`CONTROL_PING_INTERVAL_SECONDS`, `ControlTransport.kt:19`).
    public static let defaultPingInterval: Duration = .seconds(5)

    private let queue = DispatchQueue(label: "com.bandear.control")
    private let connectTimeout: Duration
    private let pingInterval: Duration
    private var connection: NWConnection?
    /// Gives up on `connection` if it is not open in time; nil once it opened or ended.
    private var openingDeadline: DispatchWorkItem?
    /// Pings `connection` while it is open.
    private var pinger: DispatchSourceTimer?
    /// True from a ping until its pong.
    private var awaitingPong = false

    /// A transport that gives a connection `connectTimeout` to open and pings an open one every
    /// `pingInterval`. Only tests need other values than the defaults.
    public init(
        connectTimeout: Duration = WebSocketTransport.defaultConnectTimeout,
        pingInterval: Duration = WebSocketTransport.defaultPingInterval
    ) {
        self.connectTimeout = connectTimeout
        self.pingInterval = pingInterval
    }

    deinit {
        stopClocks()
        connection?.stateUpdateHandler = nil
        connection?.cancel()
    }

    public func open(_ endpoint: DeskEndpoint, events: @escaping @Sendable (TransportEvent) -> Void) {
        queue.async { [self] in
            silenceAndCancel()
            let connection = NWConnection(to: Self.networkEndpoint(endpoint), using: Self.parameters())
            connection.stateUpdateHandler = { [weak self, weak connection] state in
                guard let self, let connection else { return }
                self.connection(connection, changedTo: state, events: events)
            }
            self.connection = connection
            giveUp(on: connection, unlessOpenWithin: connectTimeout, events: events)
            connection.start(queue: queue)
        }
    }

    public func send(_ text: String) {
        queue.async { [self] in
            let message = NWConnection.ContentContext(
                identifier: "text", metadata: [NWProtocolWebSocket.Metadata(opcode: .text)])
            // A send that fails goes with a connection that failed, which its state reports.
            connection?.send(
                content: Data(text.utf8), contentContext: message, isComplete: true,
                completion: .contentProcessed { _ in })
        }
    }

    /// Sends the desk a close frame with a normal closure and lets the connection go once it is
    /// out; a connection that never opened is just cancelled.
    public func close() {
        queue.async { [self] in
            stopClocks()
            guard let connection else { return }
            connection.stateUpdateHandler = nil
            self.connection = nil
            guard connection.state == .ready else {
                connection.cancel()
                return
            }
            let closeFrame = NWProtocolWebSocket.Metadata(opcode: .close)
            closeFrame.closeCode = .protocolCode(.normalClosure)
            connection.send(
                content: nil,
                contentContext: NWConnection.ContentContext(identifier: "close", metadata: [closeFrame]),
                isComplete: true,
                completion: .contentProcessed { _ in connection.cancel() })
        }
    }

    /// Where Network should connect for `endpoint`.
    ///
    /// A desk typed by hand is a `ws://` URL, as on Android (`"ws://$host:$port"`,
    /// `ControlChannel.kt:246`), and not a host and port: given an address as `.hostPort`, Network's
    /// WebSocket client aborts (`ECONNABORTED`) before it sends the handshake, while a URL works for
    /// an address and a name alike (seen on macOS 27; the round trip reaches `127.0.0.1` this way).
    /// An IPv6 address goes in brackets. A host no URL can hold falls back to `.hostPort`, whose
    /// failure the connection then reports.
    static func networkEndpoint(_ endpoint: DeskEndpoint) -> NWEndpoint {
        switch endpoint {
        case .bonjour(let name, let type, let domain):
            return .service(name: name, type: type, domain: domain, interface: nil)
        case .manual(let host, let controlPort):
            var url = URLComponents()
            url.scheme = "ws"
            url.host = host.contains(":") ? "[\(host)]" : host
            url.port = Int(controlPort)
            return url.url.map(NWEndpoint.url)
                ?? .hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: controlPort) ?? .any)
        }
    }

    /// TCP over IPv4 with a WebSocket on top, answering the desk's pings.
    ///
    /// IPv4 only, because that is the one family the desk listens on (`0.0.0.0`,
    /// `apps/server/src/control.rs:357`) while its Bonjour announcement also carries IPv6
    /// link-local addresses. Resolved over an interface where only those exist (the desk on the same
    /// Mac, over `lo0`), an unrestricted connection is refused at every address until its deadline.
    /// Android reaches the IPv4 source address of the desk's beacon, so it is IPv4 in practice too.
    static func parameters() -> NWParameters {
        let webSocket = NWProtocolWebSocket.Options()
        webSocket.autoReplyPing = true
        let parameters = NWParameters.tcp
        parameters.defaultProtocolStack.applicationProtocols.insert(webSocket, at: 0)
        (parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options)?.version = .v4
        return parameters
    }

    private func connection(
        _ connection: NWConnection, changedTo state: NWConnection.State,
        events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        guard connection === self.connection else { return }
        switch state {
        case .ready:
            openingDeadline?.cancel()
            openingDeadline = nil
            events(.opened)
            startPinging(connection, events: events)
            receive(on: connection, events: events)
        case .waiting(let error), .failed(let error):
            // A connection waits, rather than fails, when the desk refuses it or cannot be reached,
            // and would only try again once the network changes. Whether and when to try again is
            // the session's decision (``ReconnectPolicy``), so waiting ends the connection too.
            end(reporting: .failed(error.debugDescription), to: events)
        case .setup, .preparing, .cancelled:
            // Only this transport cancels, and it silences a connection first.
            break
        @unknown default:
            break
        }
    }

    /// Reads one message, reports it if it is text, and reads the next, until the connection ends.
    private func receive(
        on connection: NWConnection, events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        connection.receiveMessage { [weak self] content, context, _, error in
            guard let self, connection === self.connection else { return }
            if let error {
                end(reporting: .failed(error.debugDescription), to: events)
                return
            }
            let frame =
                context?.protocolMetadata(definition: NWProtocolWebSocket.definition)
                as? NWProtocolWebSocket.Metadata
            switch frame?.opcode {
            case .text?:
                events(.received(String(decoding: content ?? Data(), as: UTF8.self)))
            case .close?, nil:
                // The desk's close frame, or the stream ending under it.
                end(reporting: .closed, to: events)
                return
            default:
                // Binary frames carry nothing for the phone; pings and pongs are Network's.
                break
            }
            receive(on: connection, events: events)
        }
    }

    /// Reports `connection` as failed and lets it go if it has not opened within `timeout`.
    private func giveUp(
        on connection: NWConnection, unlessOpenWithin timeout: Duration,
        events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        let deadline = DispatchWorkItem { [weak self, weak connection] in
            guard let self, let connection, connection === self.connection else { return }
            end(reporting: .failed("not open after \(timeout)"), to: events)
        }
        openingDeadline = deadline
        queue.asyncAfter(deadline: .now() + timeout.seconds, execute: deadline)
    }

    /// Pings `connection` every `pingInterval` while it is open. A ping still unanswered when the
    /// next one is due fails the connection, as OkHttp's `pingInterval` does.
    private func startPinging(
        _ connection: NWConnection, events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        awaitingPong = false
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + pingInterval.seconds, repeating: pingInterval.seconds)
        timer.setEventHandler { [weak self, weak connection] in
            guard let self, let connection, connection === self.connection else { return }
            ping(connection, events: events)
        }
        pinger = timer
        timer.resume()
    }

    private func ping(
        _ connection: NWConnection, events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        guard !awaitingPong else {
            end(reporting: .failed("no pong within \(pingInterval)"), to: events)
            return
        }
        awaitingPong = true
        let ping = NWProtocolWebSocket.Metadata(opcode: .ping)
        ping.setPongHandler(queue) { [weak self, weak connection] error in
            // A pong handler hears an error when its connection ends, which the state reports.
            guard let self, let connection, connection === self.connection, error == nil else {
                return
            }
            awaitingPong = false
        }
        connection.send(
            content: Data(),
            contentContext: NWConnection.ContentContext(identifier: "ping", metadata: [ping]),
            isComplete: true, completion: .contentProcessed { _ in })
    }

    /// Stops both clocks, so neither touches a connection that is gone.
    private func stopClocks() {
        openingDeadline?.cancel()
        openingDeadline = nil
        pinger?.cancel()
        pinger = nil
        awaitingPong = false
    }

    /// Reports how the connection ended, once, and lets it go.
    private func end(
        reporting event: TransportEvent, to events: @escaping @Sendable (TransportEvent) -> Void
    ) {
        events(event)
        silenceAndCancel()
    }

    /// Clears the handler before cancelling, so nothing the old connection reports afterwards (its
    /// `cancelled`, a receive failing) reaches anyone.
    private func silenceAndCancel() {
        stopClocks()
        connection?.stateUpdateHandler = nil
        connection?.cancel()
        connection = nil
    }
}
