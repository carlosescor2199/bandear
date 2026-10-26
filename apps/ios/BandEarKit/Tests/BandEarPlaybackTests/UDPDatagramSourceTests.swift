import Foundation
import Testing

@testable import BandEarPlayback

private struct SendFailed: Error {
    let code: Int32
}

/// Sends `bytes` as one datagram to `port` on 127.0.0.1, from a socket of its own.
private func send(_ bytes: [UInt8], toPort port: UInt16) throws {
    let descriptor = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
    guard descriptor >= 0 else { throw SendFailed(code: errno) }
    defer { close(descriptor) }
    var address = sockaddr_in()
    address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    address.sin_family = sa_family_t(AF_INET)
    address.sin_port = port.bigEndian
    address.sin_addr.s_addr = inet_addr("127.0.0.1")
    let sent = bytes.withUnsafeBytes { raw in
        withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                sendto(
                    descriptor, raw.baseAddress, raw.count, 0, $0,
                    socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
    }
    guard sent == bytes.count else { throw SendFailed(code: errno) }
}

/// What one receive answered, handed back across the thread it ran on.
private final class Outcome: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Result<[UInt8]?, any Error>?

    func record(_ result: Result<[UInt8]?, any Error>) {
        lock.lock()
        defer { lock.unlock() }
        stored = result
    }

    var result: Result<[UInt8]?, any Error>? {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }
}

/// One receive from `source` on a thread of its own, waited for at most `seconds`: the bytes, `nil`
/// for a timeout, or the error; `nil` as a whole when the receive did not end in time.
///
/// Swift Testing cannot stop a synchronous wait, so a receive that never ends would stall the run;
/// waiting on a semaphore from outside turns that into a failed expectation instead.
private func receive(from source: UDPDatagramSource, within seconds: Double) -> Result<
    [UInt8]?, any Error
>? {
    let outcome = Outcome()
    let done = DispatchSemaphore(value: 0)
    Thread {
        var buffer = [UInt8](repeating: 0, count: ReceiveLoop.maximumDatagramBytes)
        outcome.record(
            Result {
                try buffer.withUnsafeMutableBytes { try source.receive(into: $0) }
                    .map { Array(buffer[..<$0]) }
            })
        done.signal()
    }.start()
    guard done.wait(timeout: .now() + seconds) == .success else { return nil }
    return outcome.result
}

/// The BSD socket the receive loop reads, on 127.0.0.1: what Android's `DatagramSocket(port)` with
/// `soTimeout = 250` does (`PmonPlayer.kt:160-163`).
@Suite("UDP datagram source")
struct UDPDatagramSourceTests {

    @Test func aDatagramSentFromALocalSocketIsReceivedByteForByte() throws {
        let source = try UDPDatagramSource(port: 0)
        defer { source.close() }
        let sent = (0..<1_940).map { UInt8(truncatingIfNeeded: $0 &* 37) }

        try send(sent, toPort: source.port)

        let outcome = try #require(receive(from: source, within: 2))
        #expect(try outcome.get() == sent)
    }

    @Test func aSourceOnPortZeroIsGivenAPortOfItsOwn() throws {
        let source = try UDPDatagramSource(port: 0)
        defer { source.close() }

        #expect(source.port != 0)
    }

    @Test func aWaitWithNothingToReceiveAnswersNilAfterTheTimeout() throws {
        let source = try UDPDatagramSource(port: 0)
        defer { source.close() }
        let started = Date()

        let outcome = try #require(receive(from: source, within: 2))

        let waited = Date().timeIntervalSince(started)
        #expect(try outcome.get() == nil)
        #expect(waited >= 0.2, "waited \(waited) s")
        #expect(waited < 0.6, "waited \(waited) s")
    }

    @Test func aClosedSourceThrowsInsteadOfReceiving() throws {
        let source = try UDPDatagramSource(port: 0)
        try send([1, 2, 3], toPort: source.port)

        source.close()

        let outcome = try #require(receive(from: source, within: 2))
        #expect(throws: DatagramSourceError.closed) { try outcome.get() }
    }

    @Test func aWakeUpAnswersAReceiveUnderWayAtOnceWithAnEmptyDatagram() throws {
        let source = try UDPDatagramSource(port: 0)
        defer { source.close() }
        // A thread of its own: a busy run can hold a global queue's work back past the timeout.
        Thread {
            Thread.sleep(forTimeInterval: 0.05)
            source.wake()
        }.start()
        let started = Date()

        let outcome = try #require(receive(from: source, within: 2))

        let waited = Date().timeIntervalSince(started)
        #expect(try outcome.get() == [])
        #expect(waited < 0.15, "waited \(waited) s")
    }

    @Test func aClosedSourceWakesNoNewerSocketOnItsPort() throws {
        let old = try UDPDatagramSource(port: 0)
        old.close()
        let newer = try UDPDatagramSource(port: old.port)
        defer { newer.close() }

        old.wake()

        let outcome = try #require(receive(from: newer, within: 2))
        #expect(try outcome.get() == nil)
    }

    @Test func aClosedSourcesPortCanBeBoundAgainAtOnce() throws {
        let old = try UDPDatagramSource(port: 0)
        old.close()

        let newer = try UDPDatagramSource(port: old.port)
        defer { newer.close() }

        #expect(newer.port == old.port)
    }

    @Test func aPortAlreadyBoundIsRefused() throws {
        let first = try UDPDatagramSource(port: 0)
        defer { first.close() }

        #expect(throws: DatagramSourceError.socket(operation: "bind", code: EADDRINUSE)) {
            _ = try UDPDatagramSource(port: first.port)
        }
    }
}
