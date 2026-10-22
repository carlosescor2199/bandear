import BandEarProtocol
import Testing

@testable import BandEarPlayback

// MARK: - Datagrams

/// The bytes of one audio datagram as the desk sends it (`apps/server/src/protocol.rs:476-498`).
private func datagram(
    sequence: UInt64, samples: [Int16], channels: UInt8 = 2, sampleRate: UInt32 = 48_000
) -> [UInt8] {
    var bytes = Array("PMON".utf8)
    bytes.append(1)
    bytes.append(channels)
    bytes += littleEndian(sampleRate)
    bytes += littleEndian(sequence)
    bytes += littleEndian(UInt16(samples.count))
    for sample in samples { bytes += littleEndian(UInt16(bitPattern: sample)) }
    return bytes
}

private func littleEndian<Value: FixedWidthInteger>(_ value: Value) -> [UInt8] {
    (0..<(Value.bitWidth / 8)).map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) }
}

/// `frames` stereo frames, every sample `value`.
private func stereo(frames: Int, value: Int16 = 1_000) -> [Int16] {
    Array(repeating: value, count: frames * 2)
}

/// A 10 ms packet at 48 kHz, the size the tests play unless they say otherwise.
private let packetFrames = 480

// MARK: - Fakes

/// Hands the loop one prepared answer per receive: a datagram, or `nil` for a timeout. With nothing
/// left it runs `whenEmpty`, then times out unless that threw. `whileWaiting` runs inside every
/// receive, as something another thread does while the loop waits.
private final class FakeSource: DatagramSource {
    var arrivals: [[UInt8]?] = []
    var failure: (any Error)?
    var whenEmpty: (() throws -> Void)?
    var whileWaiting: (() -> Void)?

    func receive(into buffer: UnsafeMutableRawBufferPointer) throws -> Int? {
        whileWaiting?()
        if let failure { throw failure }
        guard !arrivals.isEmpty else {
            try whenEmpty?()
            return nil
        }
        guard let bytes = arrivals.removeFirst() else { return nil }
        buffer.copyBytes(from: bytes)
        return bytes.count
    }

    func wake() {}
    func close() {}
}

/// Notes everything the loop asks of the output.
private final class FakeSink: PlaybackSink {
    let capacityFrames: Int
    /// The most frames one write takes; `nil` takes every frame.
    var framesPerWrite: Int?
    /// What a target answers: the target itself unless a test says otherwise.
    var applies: (Int) -> Int? = { $0 }
    var underruns = 0
    var presentedFrame: Int64?

    private(set) var writes: [[Int16]] = []
    private(set) var targets: [Int] = []
    private(set) var underrunReads = 0
    private(set) var closes = 0

    init(capacityFrames: Int) { self.capacityFrames = capacityFrames }

    var underrunCount: Int {
        underrunReads += 1
        return underruns
    }

    func write(_ samples: UnsafeBufferPointer<Int16>) -> Int {
        writes.append(Array(samples))
        let frames = samples.count / 2
        return min(frames, framesPerWrite ?? frames)
    }

    func setTargetFrames(_ frames: Int) -> Int? {
        targets.append(frames)
        return applies(frames)
    }

    func close() { closes += 1 }
}

private struct Open: Equatable {
    let sampleRate: Int
    let packetFrames: Int
}

private struct OpenFailed: Error, Equatable {}

private final class FakeFactory: PlaybackSinkFactory {
    let sink: FakeSink
    /// How many opens fail before one succeeds.
    var failures = 0
    private(set) var opens: [Open] = []

    init(sink: FakeSink) { self.sink = sink }

    func open(sampleRate: Int, packetFrames: Int) throws -> any PlaybackSink {
        if failures > 0 {
            failures -= 1
            throw OpenFailed()
        }
        opens.append(Open(sampleRate: sampleRate, packetFrames: packetFrames))
        return sink
    }
}

private final class FakeClock: MonotonicClock {
    var nowNanoseconds: UInt64 = 0

    func set(milliseconds: Int64) { nowNanoseconds = UInt64(milliseconds) * 1_000_000 }
}

private final class Heard: ReceiverListener {
    private(set) var stats: [ReceiverStats] = []
    private(set) var reports: [LinkStatsReport] = []
    private(set) var errors: [any Error] = []
    private(set) var stops = 0

    var receiveErrors: [ReceiveError] { errors.compactMap { $0 as? ReceiveError } }

    func onStats(_ stats: ReceiverStats) { self.stats.append(stats) }
    func onLinkStats(_ report: LinkStatsReport) { reports.append(report) }
    func onError(_ error: any Error) { errors.append(error) }
    func onStopped() { stops += 1 }
}

private struct SocketFailed: Error, Equatable {}

/// A loop with every collaborator faked, started at 0 ms, driven one wake-up at a time.
private final class Rig {
    let source = FakeSource()
    let sink: FakeSink
    let factory: FakeFactory
    let controls: PlayerControls
    let heard = Heard()
    let clock = FakeClock()
    let loop: ReceiveLoop

    init(_ settings: PlayerSettings = PlayerSettings(), capacityFrames: Int = 24_000) {
        sink = FakeSink(capacityFrames: capacityFrames)
        factory = FakeFactory(sink: sink)
        controls = PlayerControls(settings)
        loop = ReceiveLoop(
            source: source, sinkFactory: factory, controls: controls, listener: heard,
            clock: clock)
    }

    /// One wake-up at `milliseconds` on which `bytes` arrived.
    func arrive(_ bytes: [UInt8], at milliseconds: Int64) throws {
        clock.set(milliseconds: milliseconds)
        source.arrivals = [bytes]
        try loop.step()
    }

    /// One wake-up at `milliseconds` on which packet `sequence` arrived, 10 ms of stereo at 48 kHz.
    func arrive(sequence: UInt64, at milliseconds: Int64, value: Int16 = 1_000) throws {
        try arrive(datagram(sequence: sequence, samples: stereo(frames: packetFrames, value: value)), at: milliseconds)
    }

    /// One wake-up at `milliseconds` on which nothing arrived.
    func timeOut(at milliseconds: Int64) throws {
        clock.set(milliseconds: milliseconds)
        source.arrivals = [nil]
        try loop.step()
    }
}

// MARK: - Tests

/// The receive loop ported from the Android client's `PmonPlayer.kt:124-357`, wake-up by wake-up,
/// against a fake socket, a fake output and a clock the test sets. Android has no test of its own
/// for the loop, so each case pins one rule of the Kotlin as it reads.
@Suite("Receive loop")
struct ReceiveLoopTests {

    // MARK: The output

    @Test func theFirstPacketOpensTheSinkAtItsRateAndPacketFrames() throws {
        let rig = Rig()

        try rig.arrive(
            datagram(sequence: 1, samples: stereo(frames: 441), sampleRate: 44_100), at: 0)

        #expect(rig.factory.opens == [Open(sampleRate: 44_100, packetFrames: 441)])
        #expect(rig.sink.writes.count == 1)
    }

    @Test func theFirstPacketAsksTheSinkForTheStartCushion() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)

        #expect(rig.sink.targets == [CushionPolicy.startPackets * packetFrames])
    }

    @Test func aBufferTheDeskSetBeforeTheAudioStartedAppliesFromTheFirstPacket() throws {
        // 30 ms at 48 kHz in 480-frame packets: three packets.
        let rig = Rig(PlayerSettings(playbackBuffer: .fixed(milliseconds: 30)))

        try rig.arrive(sequence: 1, at: 0)

        #expect(rig.sink.targets == [3 * packetFrames])
    }

    @Test func laterPacketsPlayThroughTheSinkTheFirstOpened() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.arrive(sequence: 2, at: 10)
        try rig.arrive(sequence: 3, at: 20)

        #expect(rig.factory.opens.count == 1)
        #expect(rig.sink.writes.count == 3)
    }

    @Test func aSinkThatFailsToOpenIsAnErrorAndTheNextPacketOpensItAgain() throws {
        let rig = Rig()
        rig.factory.failures = 1

        try rig.arrive(sequence: 1, at: 0)
        #expect(rig.heard.errors.count == 1)
        #expect(rig.heard.errors.first is OpenFailed)
        #expect(rig.sink.writes.isEmpty)

        try rig.arrive(sequence: 2, at: 10)
        #expect(rig.factory.opens.count == 1)
        #expect(rig.sink.writes.count == 1)
    }

    // MARK: Local protection

    @Test func localProtectionScalesThePacketWhileTheDeskDoesNotOwnTheMix() throws {
        let rig = Rig(PlayerSettings(volumePercent: 50, serverOwnsMix: false))

        try rig.arrive(sequence: 1, at: 0, value: 1_000)

        let written = try #require(rig.sink.writes.first)
        #expect(written == stereo(frames: packetFrames, value: 500))
    }

    @Test func thePacketPlaysAsSentOnceTheDeskOwnsTheMix() throws {
        let rig = Rig(PlayerSettings(volumePercent: 50, serverOwnsMix: true))

        try rig.arrive(sequence: 1, at: 0, value: 1_000)

        let written = try #require(rig.sink.writes.first)
        #expect(written == stereo(frames: packetFrames, value: 1_000))
    }

    @Test func aControlChangedOnAnotherThreadAppliesFromTheNextWakeUp() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0, value: 1_000)
        rig.controls.update { $0.muted = true }
        try rig.arrive(sequence: 2, at: 10, value: 1_000)

        #expect(rig.sink.writes.count == 2)
        #expect(rig.sink.writes.last == stereo(frames: packetFrames, value: 0))
    }

    // MARK: The hold

    @Test func aDuplicatePacketIsSkipped() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.arrive(sequence: 1, at: 5)

        #expect(rig.sink.writes.count == 1)
        #expect(rig.heard.stats.count == 1)
    }

    @Test func aPacketArrivingAfterANewerOneWasPlayedIsSkipped() throws {
        let rig = Rig()

        try rig.arrive(sequence: 2, at: 0, value: 2)
        try rig.arrive(sequence: 1, at: 5, value: 1)

        #expect(rig.sink.writes == [stereo(frames: packetFrames, value: 2)])
    }

    @Test func aSkippedPacketIsNotTimedAsAnArrival() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 1_000)
        try rig.arrive(sequence: 1, at: 1_003)
        try rig.arrive(sequence: 2, at: 1_010)

        let stats = try #require(rig.heard.stats.last)
        #expect(stats.interArrivalMilliseconds == 10)
    }

    @Test func theRedundancyHoldIsReadFromTheControls() throws {
        let rig = Rig(PlayerSettings(redundancyHoldPackets: 1))

        try rig.arrive(sequence: 1, at: 0, value: 1)
        #expect(rig.sink.writes.isEmpty)

        try rig.arrive(sequence: 2, at: 10, value: 2)
        #expect(rig.sink.writes == [stereo(frames: packetFrames, value: 1)])
    }

    @Test func aTimeoutReleasesWhatTheHoldKeeps() throws {
        let rig = Rig(PlayerSettings(redundancyHoldPackets: 1))

        try rig.arrive(sequence: 1, at: 0)
        try rig.timeOut(at: 250)

        #expect(rig.sink.writes.count == 1)
    }

    @Test func aTimeoutIsCountedInTheNextReport() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.timeOut(at: 500)
        try rig.timeOut(at: 1_000)

        let report = try #require(rig.heard.reports.first)
        #expect(report.receiveTimeouts == 1)
    }

    // MARK: Link reports

    @Test func noReportIsHandedOverBeforeOneIsDue() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.timeOut(at: 999)

        #expect(rig.heard.reports.isEmpty)
    }

    @Test func aDueReportCarriesTheCushionTheSinkAppliedAndTheBacklogItPresented() throws {
        let rig = Rig()
        rig.sink.applies = { _ in 1_200 }

        try rig.arrive(sequence: 1, at: 0)
        rig.sink.presentedFrame = 240
        try rig.timeOut(at: 1_000)

        let report = try #require(rig.heard.reports.first)
        #expect(rig.heard.reports.count == 1)
        // 1 200 frames applied at 48 kHz, and 480 written less 240 presented.
        #expect(report.cushionMilliseconds == 25)
        #expect(report.backlogMilliseconds == 5)
        #expect(report.intervalMilliseconds == 1_000)
        #expect(report.packetsReceived == 1)
    }

    @Test func aReportHasNoBacklogWhileTheSinkHasPresentedNothing() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.timeOut(at: 1_000)

        let report = try #require(rig.heard.reports.first)
        #expect(report.backlogMilliseconds == nil)
        #expect(report.cushionMilliseconds == 20)
    }

    @Test func aReportCarriesTheSinksUnderruns() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        rig.sink.underruns = 3
        try rig.timeOut(at: 1_000)

        let report = try #require(rig.heard.reports.first)
        #expect(report.underruns == 3)
    }

    @Test func aReportIsTakenBeforeTheWakeUpReceives() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        try rig.arrive(sequence: 2, at: 1_000)

        let report = try #require(rig.heard.reports.first)
        #expect(report.packetsReceived == 1)
        #expect(rig.sink.writes.count == 2)
    }

    @Test func theSinksUnderrunsAreReadOnlyOnceAReportIsDue() throws {
        let rig = Rig()

        for sequence in 1...5 {
            try rig.arrive(sequence: UInt64(sequence), at: Int64(sequence) * 10)
        }
        #expect(rig.sink.underrunReads == 0)

        try rig.timeOut(at: 1_010)
        #expect(rig.sink.underrunReads == 1)
    }

    // MARK: Per-packet stats

    @Test func eachPlayedPacketReportsTheReceiversCounts() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 1_000)
        // Packet 2 never comes.
        try rig.arrive(sequence: 3, at: 1_010)

        #expect(rig.heard.stats.count == 2)
        let stats = try #require(rig.heard.stats.last)
        #expect(
            stats
                == ReceiverStats(
                    packets: 2, lost: 1, sampleRate: 48_000, packetsPerSecond: 2 / 1.01,
                    interArrivalMilliseconds: 10, jitterMilliseconds: 0, discardedSamples: 0,
                    packetsWithDiscardedSamples: 0, playbackBacklogMilliseconds: nil))
    }

    @Test func theBacklogIsSeededWithItsFirstValueThenSmoothedByATenth() throws {
        let rig = Rig()
        rig.sink.presentedFrame = 0

        // 480 frames queued is 10 ms, then 960 is 20 ms.
        try rig.arrive(sequence: 1, at: 0)
        try rig.arrive(sequence: 2, at: 10)

        #expect(rig.heard.stats.map(\.playbackBacklogMilliseconds) == [10, 11])
    }

    @Test func noBacklogIsMeasuredWhileTheSinkHasPresentedNothing() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)

        let stats = try #require(rig.heard.stats.first)
        #expect(stats.playbackBacklogMilliseconds == nil)
    }

    // MARK: Short writes

    @Test func aShortWriteCountsTheSamplesItDiscarded() throws {
        let rig = Rig()
        rig.sink.framesPerWrite = 100

        try rig.arrive(sequence: 1, at: 0)
        try rig.arrive(sequence: 2, at: 10)

        let stats = try #require(rig.heard.stats.last)
        // 960 samples sent, 200 taken, each time.
        #expect(stats.discardedSamples == 1_520)
        #expect(stats.packetsWithDiscardedSamples == 2)
    }

    @Test func onlyTheFramesTheSinkTookCountAsWritten() throws {
        let rig = Rig()
        rig.sink.framesPerWrite = 120
        rig.sink.presentedFrame = 0

        try rig.arrive(sequence: 1, at: 0)

        let stats = try #require(rig.heard.stats.first)
        // 120 frames at 48 kHz.
        #expect(stats.playbackBacklogMilliseconds == 2.5)
    }

    // MARK: Errors

    @Test func aSampleRateChangeIsAnErrorForEveryLaterPacketAndNeverReopensTheSink() throws {
        let rig = Rig()

        try rig.arrive(datagram(sequence: 1, samples: stereo(frames: 480), sampleRate: 48_000), at: 0)
        try rig.arrive(datagram(sequence: 2, samples: stereo(frames: 441), sampleRate: 44_100), at: 10)
        try rig.arrive(datagram(sequence: 3, samples: stereo(frames: 441), sampleRate: 44_100), at: 20)

        #expect(
            rig.heard.receiveErrors == [
                .sampleRateChanged(from: 48_000, to: 44_100),
                .sampleRateChanged(from: 48_000, to: 44_100),
            ])
        #expect(rig.factory.opens.count == 1)
        #expect(rig.sink.writes.count == 1)
    }

    @Test func aPacketWithMoreThan32ChannelsIsRejected() throws {
        let rig = Rig()

        try rig.arrive(
            datagram(sequence: 1, samples: Array(repeating: 1, count: 33 * 10), channels: 33),
            at: 0)

        #expect(rig.heard.receiveErrors == [.tooManyChannels(33)])
        #expect(rig.factory.opens.isEmpty)
    }

    @Test func aPacketWith32ChannelsIsPlayed() throws {
        let rig = Rig()

        try rig.arrive(
            datagram(sequence: 1, samples: Array(repeating: 1, count: 32 * 30), channels: 32),
            at: 0)

        #expect(rig.heard.errors.isEmpty)
        #expect(rig.sink.writes.count == 1)
    }

    @Test func anEmptyDatagramWhileTheSessionRunsIsAnError() throws {
        let rig = Rig()

        try rig.arrive([], at: 0)

        #expect(rig.heard.errors.count == 1)
        #expect(
            rig.heard.errors.first as? AudioDatagramDecodingError == .headerTooShort(byteCount: 0))
    }

    @Test func anErrorInOnePacketDoesNotStopTheLoop() throws {
        let rig = Rig()

        try rig.arrive(Array("not audio".utf8), at: 0)
        try rig.arrive(sequence: 1, at: 10)

        #expect(rig.heard.errors.count == 1)
        #expect(rig.heard.errors.first is AudioDatagramDecodingError)
        #expect(rig.sink.writes.count == 1)
    }

    // MARK: The cushion

    @Test func aSecondOfAudioAdjustsTheCushionWithTheSinksUnderrunCount() throws {
        let rig = Rig()
        rig.sink.underruns = 1

        // 100 packets of 10 ms: one second of audio, so one adjustment.
        for sequence in 1...100 {
            try rig.arrive(sequence: UInt64(sequence), at: Int64(sequence) * 10)
        }

        #expect(rig.sink.targets == [2 * packetFrames, 3 * packetFrames])
    }

    @Test func aCleanSecondLeavesTheCushionWhereItIs() throws {
        let rig = Rig()

        for sequence in 1...100 {
            try rig.arrive(sequence: UInt64(sequence), at: Int64(sequence) * 10)
        }

        #expect(rig.sink.targets == [2 * packetFrames])
    }

    @Test func aBufferTheDeskSetsWhilePlayingIsAskedOfTheSink() throws {
        let rig = Rig()

        try rig.arrive(sequence: 1, at: 0)
        rig.controls.update { $0.playbackBuffer = .fixed(milliseconds: 40) }
        try rig.arrive(sequence: 2, at: 10)

        #expect(rig.sink.targets == [2 * packetFrames, 4 * packetFrames])
    }

    // MARK: Running

    @Test func theLoopRunsWhileTheFlagIsUp() throws {
        let rig = Rig()
        let running = RunningFlag(true)
        rig.source.arrivals = [
            datagram(sequence: 1, samples: stereo(frames: packetFrames)),
            datagram(sequence: 2, samples: stereo(frames: packetFrames)),
        ]
        rig.source.whenEmpty = { running.value = false }

        rig.loop.run(while: running)

        #expect(rig.sink.writes.count == 2)
        #expect(rig.heard.errors.isEmpty)
    }

    @Test func aSourceThatFailsWhileRunningIsReportedAndEndsTheLoop() throws {
        let rig = Rig()
        let running = RunningFlag(true)
        rig.source.failure = SocketFailed()

        rig.loop.run(while: running)

        #expect(rig.heard.errors.count == 1)
        #expect(rig.heard.errors.first is SocketFailed)
    }

    @Test func aSourceClosedByAStopIsNotAnError() throws {
        let rig = Rig()
        let running = RunningFlag(true)
        rig.source.whenEmpty = {
            running.value = false
            throw DatagramSourceError.closed
        }

        rig.loop.run(while: running)

        #expect(rig.heard.errors.isEmpty)
    }

    @Test func whatWakesAReceiveAfterAStopIsNeverRead() throws {
        let rig = Rig()
        let running = RunningFlag(true)
        // The stop's wake-up: the flag goes down while the loop waits, then an empty datagram.
        rig.source.arrivals = [[]]
        rig.source.whileWaiting = { running.value = false }

        rig.loop.run(while: running)

        #expect(rig.heard.errors.isEmpty)
        #expect(rig.source.arrivals.isEmpty)
    }

    @Test func theSinkIsClosedOnceWhenTheLoopEnds() throws {
        let rig = Rig()
        let running = RunningFlag(true)
        rig.source.arrivals = [datagram(sequence: 1, samples: stereo(frames: packetFrames))]
        rig.source.whenEmpty = { running.value = false }

        rig.loop.run(while: running)
        rig.loop.close()

        #expect(rig.sink.closes == 1)
    }
}
