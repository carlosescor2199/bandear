import BandEarProtocol
import Foundation
import Testing

@testable import BandEarPlayback

// MARK: - Helpers

/// Waits until `condition` holds, for at most `seconds`. The condition is polled on a thread of its
/// own and the test thread waits on a semaphore with a timeout, so even a condition that hangs
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
            Thread.sleep(forTimeInterval: 0.005)
        }
    }.start()
    return met.wait(timeout: .now() + seconds + 0.5) == .success
}

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

/// One 10 ms packet of stereo at 48 kHz, as the desk sends it.
private func packet(sequence: UInt64) -> [UInt8] {
    var bytes = Array("PMON".utf8) + [1, 2]
    bytes += withUnsafeBytes(of: UInt32(48_000).littleEndian, Array.init)
    bytes += withUnsafeBytes(of: sequence.littleEndian, Array.init)
    bytes += withUnsafeBytes(of: UInt16(960).littleEndian, Array.init)
    bytes += [UInt8](repeating: 0, count: 960 * 2)
    return bytes
}

// MARK: - Fakes shared with the receiver thread

/// Everything the receiver told its listener, with when its stops came.
private final class Heard: ReceiverListener, @unchecked Sendable {
    private let lock = NSLock()
    private var storedErrors: [any Error] = []
    private var storedStops: [Date] = []
    private var storedStats = 0

    var errors: [any Error] { locked { storedErrors } }
    var stops: [Date] { locked { storedStops } }
    var stats: Int { locked { storedStats } }

    func onStats(_ stats: ReceiverStats) { locked { storedStats += 1 } }
    func onLinkStats(_ report: LinkStatsReport) {}
    func onError(_ error: any Error) { locked { storedErrors.append(error) } }
    func onStopped() { locked { storedStops.append(Date()) } }

    private func locked<Value>(_ body: () -> Value) -> Value {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

/// An output that takes every frame and counts the writes and closes.
private final class Sink: PlaybackSink, @unchecked Sendable {
    private let lock = NSLock()
    private var storedWrites = 0
    private var storedCloses = 0

    var writes: Int { locked { storedWrites } }
    var closes: Int { locked { storedCloses } }

    let capacityFrames = 24_000
    let underrunCount = 0
    let presentedFrame: Int64? = nil

    func write(_ samples: UnsafeBufferPointer<Int16>) -> Int {
        locked { storedWrites += 1 }
        return samples.count / 2
    }

    func setTargetFrames(_ frames: Int) -> Int? { frames }
    func close() { locked { storedCloses += 1 } }

    private func locked<Value>(_ body: () -> Value) -> Value {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

private final class Factory: PlaybackSinkFactory, @unchecked Sendable {
    let sink = Sink()

    func open(sampleRate: Int, packetFrames: Int) throws -> any PlaybackSink { sink }
}

/// A real socket, noting which thread used it and how.
private final class WatchedSource: DatagramSource, @unchecked Sendable {
    private let inner: UDPDatagramSource
    private let lock = NSLock()
    private var receivingThreads: Set<String> = []
    private var qualities: Set<QualityOfService> = []
    private var wakes = 0
    private var closingThreads: [String] = []

    init(port: UInt16) throws { inner = try UDPDatagramSource(port: port) }

    var port: UInt16 { inner.port }
    var receivedOn: Set<String> { locked { receivingThreads } }
    var receivedAt: Set<QualityOfService> { locked { qualities } }
    var wakeCount: Int { locked { wakes } }
    var closedOn: [String] { locked { closingThreads } }

    func receive(into buffer: UnsafeMutableRawBufferPointer) throws -> Int? {
        let thread = Thread.current
        locked {
            receivingThreads.insert(thread.name ?? "")
            qualities.insert(thread.qualityOfService)
        }
        return try inner.receive(into: buffer)
    }

    func wake() {
        locked { wakes += 1 }
        inner.wake()
    }

    func close() {
        let name = Thread.current.name ?? ""
        locked { closingThreads.append(name) }
        inner.close()
    }

    private func locked<Value>(_ body: () -> Value) -> Value {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

/// The sources a receiver opened, in order, so a test can reach the one it is reading.
private final class Opened: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [WatchedSource] = []

    var sources: [WatchedSource] {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func open(port: UInt16) throws -> any DatagramSource {
        let source = try WatchedSource(port: port)
        lock.lock()
        defer { lock.unlock() }
        stored.append(source)
        return source
    }
}

/// A source that fails on its first receive, as a socket that dies under the loop.
private final class FailingSource: DatagramSource, @unchecked Sendable {
    struct Failed: Error {}

    func receive(into buffer: UnsafeMutableRawBufferPointer) throws -> Int? { throw Failed() }
    func wake() {}
    func close() {}
}

private struct CannotBind: Error {}

/// A receiver started on a watched socket of its own, already reading it.
private struct Running {
    let receiver: AudioReceiver
    let heard: Heard
    let factory: Factory
    let opened: Opened
    let source: WatchedSource
    let running: RunningFlag
}

private func startReceiver() throws -> Running {
    let heard = Heard()
    let factory = Factory()
    let opened = Opened()
    let running = RunningFlag()
    let receiver = AudioReceiver(
        sinkFactory: factory, controls: PlayerControls(), listener: heard, running: running,
        openSource: opened.open)
    receiver.start(port: 0)
    try #require(waitUntil(seconds: 2) { !opened.sources.isEmpty }, "no socket opened")
    let source = try #require(opened.sources.first)
    // Let it settle into its first receive.
    try #require(waitUntil(seconds: 2) { !source.receivedOn.isEmpty }, "no receive started")
    return Running(
        receiver: receiver, heard: heard, factory: factory, opened: opened, source: source,
        running: running)
}

/// Stops `receiver` and waits for its loop to report the stop, so no thread outlives its test.
private func stopAndWait(_ running: Running) {
    running.receiver.stop()
    _ = waitUntil(seconds: 2) { !running.heard.stops.isEmpty }
}

// MARK: - Tests

/// The receiver thread around the loop, on a real socket on 127.0.0.1: the part of
/// `PmonPlayer.kt` that launches the loop (`launch`, `stopReceiving`, the loop's `finally`).
@Suite("Audio receiver")
struct AudioReceiverTests {

    @Test func stopEndsARunningReceiverWellUnderOneTimeout() throws {
        let started = try startReceiver()
        #expect(started.receiver.isAlive)

        let stoppedAt = Date()
        started.receiver.stop()

        #expect(waitUntil(seconds: 2) { !started.receiver.isAlive })
        let firstStop = try #require(started.heard.stops.first)
        let took = firstStop.timeIntervalSince(stoppedAt)
        #expect(took < 0.1, "the stop took \(took) s")
    }

    @Test func aStopIsReportedOnceAndNotAsAnError() throws {
        let started = try startReceiver()

        stopAndWait(started)
        // Nothing more comes, not even after a receive timeout.
        #expect(!waitUntil(seconds: 0.4) { started.heard.stops.count > 1 })

        #expect(started.heard.stops.count == 1)
        #expect(started.heard.errors.isEmpty)
        #expect(!started.running.value)
    }

    @Test func stoppingThenStartingAgainOnTheSamePortAtOnceBindsAndReceives() throws {
        let started = try startReceiver()
        let port = started.source.port

        started.receiver.stop()
        try #require(waitUntil(seconds: 0.1) { !started.receiver.isAlive }, "still alive")
        started.receiver.start(port: port)
        defer {
            started.receiver.stop()
            _ = waitUntil(seconds: 2) { started.heard.stops.count == 2 }
        }

        try #require(
            waitUntil(seconds: 2) {
                started.opened.sources.count == 2
                    && !started.opened.sources[1].receivedOn.isEmpty
            }, "the second launch is not receiving")
        try send(packet(sequence: 1), toPort: port)
        #expect(waitUntil(seconds: 2) { started.factory.sink.writes == 1 })
        #expect(started.opened.sources[1].port == port)
        #expect(started.heard.errors.isEmpty)
    }

    @Test func aDatagramSentToTheReceiverIsPlayed() throws {
        let started = try startReceiver()
        defer { stopAndWait(started) }

        try send(packet(sequence: 1), toPort: started.source.port)

        #expect(waitUntil(seconds: 2) { started.factory.sink.writes == 1 })
        #expect(waitUntil(seconds: 2) { started.heard.stats == 1 })
    }

    @Test func theSinkIsClosedWhenTheReceiverStops() throws {
        let started = try startReceiver()
        try send(packet(sequence: 1), toPort: started.source.port)
        try #require(waitUntil(seconds: 2) { started.factory.sink.writes == 1 })

        stopAndWait(started)

        #expect(started.factory.sink.closes == 1)
    }

    @Test func theReceiverReadsOnAThreadOfItsOwnNamedAsOnAndroid() throws {
        let started = try startReceiver()
        defer { stopAndWait(started) }

        #expect(started.source.receivedOn == ["pmon-udp-receiver"])
        #expect(started.source.receivedAt == [.userInteractive])
    }

    @Test func aStopWakesTheSocketAndTheReceiverThreadClosesIt() throws {
        let started = try startReceiver()

        started.receiver.stop()
        #expect(started.source.wakeCount == 1)
        #expect(waitUntil(seconds: 2) { !started.heard.stops.isEmpty })

        #expect(started.source.closedOn == ["pmon-udp-receiver"])
    }

    @Test func aSocketThatCannotOpenIsReportedAndEndsTheSession() throws {
        let heard = Heard()
        let running = RunningFlag()
        let receiver = AudioReceiver(
            sinkFactory: Factory(), controls: PlayerControls(), listener: heard, running: running,
            openSource: { _ in throw CannotBind() })

        receiver.start(port: 0)

        #expect(waitUntil(seconds: 2) { heard.stops.count == 1 })
        #expect(heard.errors.count == 1)
        #expect(heard.errors.first is CannotBind)
        #expect(!running.value)
        #expect(waitUntil(seconds: 1) { !receiver.isAlive })
    }

    @Test func aReceiverWhoseSocketFailsEndsTheSessionOnItsOwn() throws {
        let heard = Heard()
        let running = RunningFlag()
        let receiver = AudioReceiver(
            sinkFactory: Factory(), controls: PlayerControls(), listener: heard, running: running,
            openSource: { _ in FailingSource() })

        receiver.start(port: 0)

        #expect(waitUntil(seconds: 2) { heard.stops.count == 1 })
        #expect(heard.errors.count == 1)
        #expect(heard.errors.first is FailingSource.Failed)
        #expect(!running.value)
    }

    @Test func aReceiverIsAliveFromItsStartUntilItsStopIsReported() throws {
        let heard = Heard()
        let receiver = AudioReceiver(
            sinkFactory: Factory(), controls: PlayerControls(), listener: heard,
            openSource: { _ in try UDPDatagramSource(port: 0) })
        #expect(!receiver.isAlive)

        receiver.start(port: 0)
        #expect(receiver.isAlive)

        receiver.stop()
        #expect(waitUntil(seconds: 2) { !receiver.isAlive })
        #expect(heard.stops.count == 1)
    }
}
