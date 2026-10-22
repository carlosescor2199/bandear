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

    init(advertisingAs name: String?) throws {
        let parameters = NWParameters.tcp
        let webSocket = NWProtocolWebSocket.Options()
        webSocket.autoReplyPing = true
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

    private func accept(_ connection: NWConnection) {
        guard self.connection == nil else {
            connection.cancel()
            return
        }
        self.connection = connection
        connection.stateUpdateHandler = { [weak self] state in
            self?.note("connection \(state)")
            if case .ready = state {
                self?.send(Data(Self.config.utf8), NWProtocolWebSocket.Metadata(opcode: .text))
                self?.receive(on: connection)
            }
        }
        connection.start(queue: queue)
    }

    private func receive(on connection: NWConnection) {
        connection.receiveMessage { [weak self] content, context, _, error in
            guard let self else { return }
            if let error {
                note("receive \(error)")
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
            case nil:
                note("stream ended")
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
