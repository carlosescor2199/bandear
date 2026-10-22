import BandEarDiscovery
import Foundation
import Network
import Testing

@testable import BandEarSession

/// How ``WebSocketTransport`` names a desk to Network.
@Suite("WebSocket transport")
struct WebSocketTransportTests {

    @Test("reaches a desk found over Bonjour by its service, which Network resolves itself")
    func bonjourEndpoint() {
        #expect(
            WebSocketTransport.networkEndpoint(
                .bonjour(name: "BandEar on Stage", type: "_bandear._tcp", domain: "local."))
                == .service(
                    name: "BandEar on Stage", type: "_bandear._tcp", domain: "local.", interface: nil))
    }

    /// Not `.hostPort`: Network's WebSocket client sends no handshake to an address given that way.
    @Test(
        "reaches a desk typed by hand at a ws:// URL of its host and control port",
        arguments: [
            ("192.168.1.20", "ws://192.168.1.20:50001"),
            ("desk.local", "ws://desk.local:50001"),
            ("fe80::2", "ws://[fe80::2]:50001"),
        ])
    func manualEndpoint(host: String, url: String) throws {
        #expect(
            WebSocketTransport.networkEndpoint(.manual(host: host, controlPort: 50_001))
                == .url(try #require(URL(string: url))))
    }

    /// A space is one of the characters no URL host can hold (so are `/`, `?`, `#` and `@`).
    @Test("reaches a host no URL can hold by host and port, whose failure the connection reports")
    func manualEndpointNoURLCanHold() {
        #expect(
            WebSocketTransport.networkEndpoint(.manual(host: "bad host", controlPort: 50_001))
                == .hostPort(host: "bad host", port: 50_001))
    }

    /// Android's `ControlChannelReconnectTest.theClientPingsOftenEnoughForTheServerButNotAggressively`.
    @Test("pings every five seconds, inside the desk's ten-second silence limit")
    func theClientPingsOftenEnoughForTheServerButNotAggressively() {
        #expect(WebSocketTransport.defaultPingInterval == .seconds(5))
    }

    /// OkHttp's default, which the Android client keeps (`ControlTransport.kt:77-79` sets none).
    @Test("gives a connection ten seconds to open")
    func theConnectTimeoutIsOkHttpsDefault() {
        #expect(WebSocketTransport.defaultConnectTimeout == .seconds(10))
    }
}

/// A real round trip through ``WebSocketTransport``: a WebSocket desk in this process
/// (`NWListener` with `NWProtocolWebSocket`), reached once by address and once over Bonjour.
///
/// Each run checks what the desk depends on (`apps/server/src/control.rs`): a text message each
/// way, the desk's pings answered with the connection staying open (it pings every 2 s and drops a
/// client silent for 10, so a transport that never answers would be cut off mid-show), and a clean
/// close, from the desk in one run and from the phone in the other.
///
/// Opt-in (`BANDEAR_WS_IT=1`), because it listens on a port and the Bonjour run announces a service
/// on the local network for a second or two. The adapter has no other coverage: run it whenever
/// `WebSocketTransport` changes.
@Suite(
    "WebSocket transport (network)",
    .enabled(if: ProcessInfo.processInfo.environment["BANDEAR_WS_IT"] == "1"))
struct WebSocketTransportIntegrationTests {

    @Test("reaches a desk by address: text both ways, pings answered, the desk's close heard")
    func manualRoundTrip() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")

        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }

        try exchange(with: desk, through: transport, heard: heard)

        desk.sendClose()
        #expect(
            waitUntil(seconds: 5) { heard.events.last == .closed },
            "the desk's close never reached the phone; \(heard.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config), .closed])
    }

    @Test("reaches a desk over Bonjour: text both ways, pings answered, the phone closes cleanly")
    func bonjourRoundTrip() throws {
        let name = "BandEar WS IT \(UUID().uuidString.prefix(8))"
        let desk = try TestDesk(advertisingAs: name)
        defer { desk.stop() }
        try #require(desk.waitForPort() != nil, "the desk never listened; \(desk.events)")

        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(
            .bonjour(name: name, type: DeskDirectory.serviceType, domain: "local."),
            events: { heard.record($0) })

        try exchange(with: desk, through: transport, heard: heard)

        transport.close()
        #expect(
            waitUntil(seconds: 5) { desk.closeFrames == 1 },
            "the phone's close never reached the desk; \(desk.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config)])
    }

    // MARK: Opening that never ends

    @Test("gives up on a host that accepts the connection and never answers the handshake")
    func timesOutOnASilentHost() throws {
        let host = try SilentHost()
        defer { host.stop() }
        let port = try #require(host.waitForPort(), "the host never listened")

        let transport = WebSocketTransport(connectTimeout: .milliseconds(300))
        let heard = Heard()
        let opened = Date()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }

        #expect(
            waitUntil(seconds: 5) { !heard.events.isEmpty }, "the open never ended; \(host.events)")
        let elapsed = Date().timeIntervalSince(opened)
        #expect(elapsed >= 0.25, "ended after \(elapsed) s, before its timeout: \(heard.events)")
        #expect(waitUntil(seconds: 5) { host.events.contains("ended") }, "never cancelled; \(host.events)")
        #expect(heard.isOneFailure, "\(heard.events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    @Test("gives up on a desk found over Bonjour that is no longer announced")
    func timesOutOnAGoneBonjourDesk() {
        let name = "BandEar WS IT gone \(UUID().uuidString.prefix(8))"
        let transport = WebSocketTransport(connectTimeout: .milliseconds(300))
        let heard = Heard()
        transport.open(
            .bonjour(name: name, type: DeskDirectory.serviceType, domain: "local."),
            events: { heard.record($0) })
        defer { transport.close() }

        #expect(waitUntil(seconds: 5) { !heard.events.isEmpty }, "the open never ended")
        #expect(heard.isOneFailure, "\(heard.events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    // MARK: The phone's own pings

    /// Five pings take longer than the connect timeout, so the connection also outlives its deadline.
    @Test("pings an open connection, which stays open past its connect deadline while the desk answers")
    func pingsKeepALiveLinkOpen() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")

        let transport = WebSocketTransport(
            connectTimeout: .milliseconds(300), pingInterval: .milliseconds(100))
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }

        #expect(waitUntil(seconds: 5) { desk.pingsHeard >= 5 }, "pings: \(desk.pingsHeard); \(desk.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config)])
    }

    /// A Wi-Fi drop with no FIN looks like this to the phone: an open connection that goes silent.
    @Test("reports a desk that stops answering pings as failed, once")
    func anUnansweredPingFailsTheConnection() throws {
        let desk = try TestDesk(advertisingAs: nil, answersPings: false)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")

        let transport = WebSocketTransport(pingInterval: .milliseconds(100))
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }

        #expect(
            waitUntil(seconds: 5) { heard.events.count == 3 }, "phone: \(heard.events); desk: \(desk.events)")
        #expect(desk.pingsHeard >= 1)
        let events = heard.events
        try #require(events.count == 3)
        #expect(Array(events.prefix(2)) == [.opened, .received(TestDesk.config)])
        #expect(Heard.isFailure(events[2]), "\(events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    // MARK: How a connection fails

    /// Network waits, rather than fails, on a refused connection; the transport reports it as failed.
    @Test("reports a port nobody listens on as failed, once")
    func aRefusedPortFails() throws {
        let port = try freePort()
        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }

        #expect(waitUntil(seconds: 5) { !heard.events.isEmpty }, "the open never ended")
        #expect(heard.isOneFailure, "\(heard.events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    @Test("reports a host no URL can hold as failed, once")
    func aHostNoURLCanHoldFails() {
        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(.manual(host: "bad host", controlPort: 50_001), events: { heard.record($0) })
        defer { transport.close() }

        #expect(waitUntil(seconds: 5) { !heard.events.isEmpty }, "the open never ended")
        #expect(heard.isOneFailure, "\(heard.events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    @Test("reports a desk that resets the connection as failed, once")
    func aResetFails() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")
        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }
        try #require(waitUntil(seconds: 5) { heard.events.count == 2 }, "\(heard.events)")

        desk.reset()

        #expect(waitUntil(seconds: 5) { heard.events.count == 3 }, "\(heard.events); \(desk.events)")
        let events = heard.events
        try #require(events.count == 3)
        #expect(Heard.isFailure(events[2]), "\(events)")
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    @Test("reports a stream that ends with no close frame as closed, once")
    func aStreamEndingWithoutACloseFrameCloses() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")
        let transport = WebSocketTransport()
        let heard = Heard()
        transport.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        defer { transport.close() }
        try #require(waitUntil(seconds: 5) { heard.events.count == 2 }, "\(heard.events)")

        desk.endStream()

        #expect(waitUntil(seconds: 5) { heard.events.count == 3 }, "\(heard.events); \(desk.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config), .closed])
        #expect(staysQuiet(heard, seconds: 0.3), "\(heard.events)")
    }

    // MARK: Letting go

    @Test("is released after it closes an open connection")
    func releasedAfterClose() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")
        var transport: WebSocketTransport? = WebSocketTransport()
        let released = Released(transport)
        let heard = Heard()
        transport?.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        try #require(waitUntil(seconds: 5) { heard.events.count == 2 }, "\(heard.events)")

        transport?.close()
        transport = nil

        #expect(waitUntil(seconds: 5) { released.isReleased }, "the transport is still held")
        #expect(waitUntil(seconds: 5) { desk.closeFrames == 1 }, "\(desk.events)")
    }

    /// What a connection's handlers hold must not hold the transport, or a transport dropped while
    /// open would keep its connection, and the desk its musician, for good.
    @Test("is released when dropped while open, and its connection goes with it")
    func releasedWhenDroppedWhileOpen() throws {
        let desk = try TestDesk(advertisingAs: nil)
        defer { desk.stop() }
        let port = try #require(desk.waitForPort(), "the desk never listened; \(desk.events)")
        var transport: WebSocketTransport? = WebSocketTransport(pingInterval: .milliseconds(100))
        let released = Released(transport)
        let heard = Heard()
        transport?.open(.manual(host: "127.0.0.1", controlPort: port), events: { heard.record($0) })
        try #require(waitUntil(seconds: 5) { heard.events.count == 2 }, "\(heard.events)")
        try #require(waitUntil(seconds: 5) { desk.pingsHeard >= 1 }, "never pinged; \(desk.events)")

        transport = nil

        #expect(waitUntil(seconds: 5) { released.isReleased }, "the transport is still held")
        #expect(waitUntil(seconds: 5) { desk.connectionEnded }, "\(desk.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config)])
    }

    /// The part both runs share: the phone opens and hears the desk's `config`, the desk hears the
    /// phone's text, and three pings come back while the connection stays open.
    private func exchange(
        with desk: TestDesk, through transport: WebSocketTransport, heard: Heard
    ) throws {
        try #require(
            waitUntil(seconds: 10) { heard.events.contains(.received(TestDesk.config)) },
            "never heard the desk; phone: \(heard.events); desk: \(desk.events)")
        #expect(heard.events.first == .opened)

        transport.send(#"{"type":"register","name":"Ana","udp_port":50000}"#)
        #expect(
            waitUntil(seconds: 5) {
                desk.received == [#"{"type":"register","name":"Ana","udp_port":50000}"#]
            }, "the desk never heard the phone; \(desk.events)")

        for _ in 0..<3 {
            desk.ping()
            Thread.sleep(forTimeInterval: 0.2)
        }
        #expect(waitUntil(seconds: 5) { desk.pongs == 3 }, "pongs: \(desk.pongs); \(desk.events)")
        #expect(heard.events == [.opened, .received(TestDesk.config)])
    }
}

/// A WebSocket desk on a port of its own, answering nothing: it sends `config` on accept, as the
/// real desk does, and records what it hears.
private final class TestDesk: @unchecked Sendable {
    static let config = #"{"type":"config","source_channels":2,"groups":[]}"#

    private let listener: NWListener
    private let queue = DispatchQueue(label: "bandear.it.ws-desk")
    private let lock = NSLock()
    private var connection: NWConnection?
    private var storedReceived: [String] = []
    private var storedEvents: [String] = []
    private var storedPongs = 0
    private var storedCloseFrames = 0
    private var storedPingsHeard = 0
    private var storedConnectionEnded = false
    private let answersPings: Bool

    /// A desk that answers the phone's pings, unless `answersPings` is false, as a link gone silent.
    init(advertisingAs name: String?, answersPings: Bool = true) throws {
        self.answersPings = answersPings
        let parameters = NWParameters.tcp
        let webSocket = NWProtocolWebSocket.Options()
        // Answered here rather than by Network, so the desk can count the pings and leave them be.
        webSocket.autoReplyPing = false
        parameters.defaultProtocolStack.applicationProtocols.insert(webSocket, at: 0)
        listener = try NWListener(using: parameters)
        if let name {
            listener.service = NWListener.Service(name: name, type: DeskDirectory.serviceType)
        }
        listener.stateUpdateHandler = { [weak self] state in self?.note("listener \(state)") }
        listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
        listener.start(queue: queue)
    }

    var received: [String] { locked { storedReceived } }
    var events: [String] { locked { storedEvents } }
    var pongs: Int { locked { storedPongs } }
    var closeFrames: Int { locked { storedCloseFrames } }
    /// How many of the phone's pings reached the desk.
    var pingsHeard: Int { locked { storedPingsHeard } }
    /// True once the desk's connection to the phone ended, whichever side ended it.
    var connectionEnded: Bool { locked { storedConnectionEnded } }

    /// The port the desk listens on, once it does.
    func waitForPort() -> UInt16? {
        guard waitUntil(seconds: 5, { self.listener.port.map { $0 != .any } ?? false }) else {
            return nil
        }
        return listener.port?.rawValue
    }

    func ping() {
        let metadata = NWProtocolWebSocket.Metadata(opcode: .ping)
        metadata.setPongHandler(queue) { [weak self] error in
            guard let self, error == nil else { return }
            locked { storedPongs += 1 }
        }
        send(Data("bandear".utf8), metadata)
    }

    func sendClose() {
        let metadata = NWProtocolWebSocket.Metadata(opcode: .close)
        metadata.closeCode = .protocolCode(.normalClosure)
        send(nil, metadata)
    }

    func stop() {
        listener.cancel()
        queue.async { self.connection?.cancel() }
    }

    /// Drops the connection with a reset, as a desk that crashes does.
    func reset() {
        queue.async { self.connection?.forceCancel() }
    }

    /// Ends the stream with no close frame, as a desk that drops a connection does.
    func endStream() {
        queue.async { self.connection?.cancel() }
    }

    private func accept(_ connection: NWConnection) {
        guard self.connection == nil else {
            connection.cancel()
            return
        }
        self.connection = connection
        connection.stateUpdateHandler = { [weak self] state in
            self?.note("connection \(state)")
            switch state {
            case .ready:
                self?.send(Data(Self.config.utf8), NWProtocolWebSocket.Metadata(opcode: .text))
                self?.receive(on: connection)
            case .failed, .cancelled:
                self?.locked { self?.storedConnectionEnded = true }
            default:
                break
            }
        }
        connection.start(queue: queue)
    }

    private func receive(on connection: NWConnection) {
        connection.receiveMessage { [weak self] content, context, _, error in
            guard let self else { return }
            if let error {
                note("receive \(error)")
                locked { storedConnectionEnded = true }
                return
            }
            let metadata =
                context?.protocolMetadata(definition: NWProtocolWebSocket.definition)
                as? NWProtocolWebSocket.Metadata
            switch metadata?.opcode {
            case .text?:
                let text = String(decoding: content ?? Data(), as: UTF8.self)
                locked { storedReceived.append(text) }
            case .close?:
                locked { storedCloseFrames += 1 }
                note("close frame")
                return
            case .ping?:
                locked { storedPingsHeard += 1 }
                if answersPings {
                    send(content ?? Data(), NWProtocolWebSocket.Metadata(opcode: .pong))
                }
            case nil:
                note("stream ended")
                locked { storedConnectionEnded = true }
                return
            default:
                break
            }
            receive(on: connection)
        }
    }

    private func send(_ content: Data?, _ metadata: NWProtocolWebSocket.Metadata) {
        queue.async { [self] in
            let context = NWConnection.ContentContext(identifier: "it", metadata: [metadata])
            connection?.send(
                content: content, contentContext: context, isComplete: true,
                completion: .contentProcessed { [weak self] error in
                    if let error { self?.note("send \(error)") }
                })
        }
    }

    private func note(_ event: String) {
        locked { storedEvents.append(event) }
    }

    private func locked<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

/// What the transport reported to the phone, read from the test thread.
private final class Heard: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [TransportEvent] = []

    var events: [TransportEvent] {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    /// True when the transport reported one thing, and that was a failure.
    var isOneFailure: Bool {
        let events = events
        return events.count == 1 && Self.isFailure(events[0])
    }

    static func isFailure(_ event: TransportEvent) -> Bool {
        if case .failed = event { return true }
        return false
    }

    func record(_ event: TransportEvent) {
        lock.lock()
        defer { lock.unlock() }
        stored.append(event)
    }
}

/// Waits until `condition` holds, for at most `seconds`. The condition is polled on a thread of
/// its own and the test thread waits on a semaphore with a timeout, so even a condition that hangs
/// cannot hang the run (Swift Testing's time limit cannot stop a synchronous wait).
private func waitUntil(seconds: Double, _ condition: @escaping @Sendable () -> Bool) -> Bool {
    let met = DispatchSemaphore(value: 0)
    let deadline = Date().addingTimeInterval(seconds)
    Thread {
        while Date() < deadline {
            if condition() {
                met.signal()
                return
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
    }.start()
    return met.wait(timeout: .now() + seconds + 0.5) == .success
}

/// True when nothing more reaches `heard` for `seconds`: how a test says an end was reported once.
private func staysQuiet(_ heard: Heard, seconds: Double) -> Bool {
    let count = heard.events.count
    return !waitUntil(seconds: seconds) { heard.events.count != count }
}

/// A TCP port nothing listens on: one a listener was given and has let go.
private func freePort() throws -> UInt16 {
    let host = try SilentHost()
    defer { host.stop() }
    return try #require(host.waitForPort(), "no port to borrow")
}

/// A host that accepts TCP connections and never says a word, as a desk that hangs mid-handshake;
/// it notes each connection it accepts and each that ends.
private final class SilentHost: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "bandear.it.silent-host")
    private let lock = NSLock()
    private var connections: [NWConnection] = []
    private var storedEvents: [String] = []

    init() throws {
        listener = try NWListener(using: .tcp)
        listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
        listener.start(queue: queue)
    }

    var events: [String] {
        lock.lock()
        defer { lock.unlock() }
        return storedEvents
    }

    func waitForPort() -> UInt16? {
        guard waitUntil(seconds: 5, { self.listener.port.map { $0 != .any } ?? false }) else {
            return nil
        }
        return listener.port?.rawValue
    }

    func stop() {
        listener.cancel()
        queue.async { self.connections.forEach { $0.cancel() } }
    }

    private func accept(_ connection: NWConnection) {
        connections.append(connection)
        note("accepted")
        connection.start(queue: queue)
        read(connection)
    }

    /// Reads and drops what the phone sends, until its side ends.
    private func read(_ connection: NWConnection) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) {
            [weak self] _, _, isComplete, error in
            if isComplete || error != nil {
                self?.note("ended")
                return
            }
            self?.read(connection)
        }
    }

    private func note(_ event: String) {
        lock.lock()
        defer { lock.unlock() }
        storedEvents.append(event)
    }
}

/// Says whether an object was let go, without holding it.
private final class Released: @unchecked Sendable {
    private let lock = NSLock()
    private weak var object: AnyObject?

    init(_ object: AnyObject?) {
        self.object = object
    }

    var isReleased: Bool {
        lock.lock()
        defer { lock.unlock() }
        return object == nil
    }
}
