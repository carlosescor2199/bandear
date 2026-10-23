import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// What the parity suite reaches only indirectly: the hop from the receive thread to the main
/// actor, what the coordinator hands its receiver, how one launch pairs with one session, a stop
/// report still on its way when the next session starts, and what the screens can read.
@Suite("Monitor audio")
@MainActor
struct MonitorAudioTests {
    private let transport = FakeTransport()
    private let receivers = FakeReceivers()
    private let sinkFactory = FakeSinkFactory()
    private let session: ControlSession
    private let desk: FakeDesk
    private let audio: MonitorAudio

    init() {
        session = ControlSession(
            transport: transport, scheduler: ManualScheduler(), store: FakeMusicianStore())
        desk = FakeDesk(transport)
        audio = MonitorAudio(session: session, sinkFactory: sinkFactory, makeReceiver: receivers.make)
    }

    private func connected() async throws {
        try audio.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: 2))
    }

    private static let stats = ReceiverStats(
        packets: 500, lost: 5, sampleRate: 48_000, packetsPerSecond: 100,
        interArrivalMilliseconds: 10, jitterMilliseconds: 0.3, discardedSamples: 0,
        packetsWithDiscardedSamples: 0, playbackBacklogMilliseconds: 15, outputPlaying: true)

    private static let report = LinkStatsReport(
        intervalMilliseconds: 1_000, packetsReceived: 100, packetsLost: 1, underruns: 2,
        receiveTimeouts: 3, jitterMilliseconds: 1.234, recovered: 4)

    // MARK: What the receiver is handed

    @Test("hands its one receiver the output factory it was given and the controls it fills")
    func handsTheReceiverItsCollaborators() async throws {
        try await connected()
        session.apply(.masterVolumeChanged(level: 40))

        let receiver = try receivers.only()
        #expect(receiver.sinkFactory as? FakeSinkFactory === sinkFactory)
        #expect(receiver.controls.snapshot.volumePercent == 40)
    }

    @Test("launches the receiver with what a new session starts from: Auto, no hold, the mix the phone's own")
    func launchesWithWhatTheSessionStartsFrom() async throws {
        try await connected()
        session.apply(.masterVolumeChanged(level: 40))
        await desk.receive(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
        await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
        try await desk.receive(mixState(levels: [], type: "mix_ack"))
        let receiver = try receivers.only()
        try #require(receiver.controls.snapshot.serverOwnsMix)
        audio.stop()
        try receiver.finishStopping()
        await settle()

        try audio.start(toStage, musicianName: "Ana")

        let launch = try #require(receiver.settingsAtLaunch.last)
        #expect(
            launch
                == PlayerSettings(
                    volumePercent: 40, maxLevelPercent: 100, muted: false, serverOwnsMix: false,
                    playbackBuffer: .auto, redundancyHoldPackets: 0))
    }

    // MARK: The receiver's start and stop pair with the session's

    @Test("launches one receiver per session on its audio port, and stops each one once")
    func oneLaunchAndOneStopPerSession() async throws {
        let ports: [UInt16] = [50_000, 50_100, 50_200]
        for port in ports {
            try audio.start(ConnectionRequest(endpoint: stage, audioPort: port), musicianName: "Ana")
            await desk.open()
            audio.stop()
            try receivers.only().finishStopping()
            await settle()
        }

        let receiver = try receivers.only()
        #expect(receiver.launches == ports)
        #expect(receiver.stops == 3)
        #expect(transport.opened.count == 3)
        #expect(transport.closes == 3)
        #expect(!receiver.isAlive)
        #expect(!session.mixSession.isSessionRunning)
    }

    @Test("starts the receiver for a blank name too, as Android connects whatever the field holds")
    func aBlankNameStartsTheReceiver() throws {
        try audio.start(toStage, musicianName: "   ")

        #expect(try receivers.only().launches == [toStage.audioPort])
        #expect(transport.opened == [stage])
        #expect(audio.isRunning)
        #expect(session.mixSession.isSessionRunning)
    }

    @Test("stops the receiver when the session is stopped without it, and its report ends nothing more")
    func aSessionStoppedDirectlyStopsTheReceiver() async throws {
        try await connected()
        let receiver = try receivers.only()

        session.stop()

        #expect(receiver.stops == 1)
        #expect(transport.closes == 1)

        try receiver.finishStopping()
        await settle()
        audio.stop()

        #expect(receiver.stops == 1)
        #expect(transport.closes == 1)
        #expect(!receiver.isAlive)
    }

    /// The receiver is free as soon as its loop has reported, but the report reaches the main actor
    /// later. A session started in between is the next launch, so the report, counted as the
    /// previous one's, cannot end it.
    @Test("a stop report still on its way when the next session starts ends nothing")
    func aStopReportOnItsWayEndsNotTheNextSession() async throws {
        try await connected()
        let receiver = try receivers.only()
        audio.stop()

        try receiver.finishStopping()
        try audio.start(toStage, musicianName: "Ana")
        await settle()

        #expect(receiver.launches.count == 2)
        #expect(receiver.isAlive)
        #expect(receiver.stops == 1)
        #expect(transport.closes == 1)
        #expect(session.mixSession.isSessionRunning)
        #expect(session.connection == .connecting)
    }

    /// Two sessions started and stopped while the main actor is busy leave both their stop reports
    /// on the way. The first report then arrives with a newer session running and a newer one ended
    /// since: it belongs to neither, so it ends nothing.
    @Test("stop reports of two earlier sessions, still on their way, end neither the next session")
    func lateReportsOfEarlierSessionsEndNothing() async throws {
        let receiver = try receivers.only()
        for _ in 0..<2 {
            try audio.start(toStage, musicianName: "Ana")
            audio.stop()
            try receiver.finishStopping()
        }
        try audio.start(toStage, musicianName: "Ana")

        await settle()

        #expect(receiver.launches.count == 3)
        #expect(receiver.isAlive)
        #expect(receiver.stops == 2)
        #expect(transport.closes == 2)
        #expect(session.mixSession.isSessionRunning)
        #expect(session.connection == .connecting)

        // The third session still ends with its own receiver.
        try receiver.endOnItsOwn()
        await settle()

        #expect(transport.closes == 3)
        #expect(!session.mixSession.isSessionRunning)
    }

    // MARK: What the screens read

    @Test("keeps the latest stats, link report and error, and sends the report as the desk reads it")
    func keepsTheLatestReports() async throws {
        try await connected()
        let receiver = try receivers.only()
        let later = LinkStatsReport(
            intervalMilliseconds: 1_000, packetsReceived: 90, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0)

        receiver.listener.onStats(Self.stats)
        receiver.listener.onLinkStats(Self.report)
        receiver.listener.onLinkStats(later)
        receiver.listener.onError(ReceiveError.tooManyChannels(40))
        receiver.listener.onError(ReceiveError.sampleRateChanged(from: 48_000, to: 44_100))
        await settle()

        #expect(audio.receiverStats == Self.stats)
        #expect(audio.linkReport == later)
        #expect(
            audio.receiverError as? ReceiveError
                == .sampleRateChanged(from: 48_000, to: 44_100))
        let sent = try transport.sent(ofType: "stats")
        try #require(sent.count == 2)
        let expected = try wireObject(.stats(LinkStats(Self.report)))
        #expect(NSDictionary(dictionary: sent[0]) == NSDictionary(dictionary: expected))
    }

    @Test("keeps a link report that comes before the connection opens, and sends nothing")
    func aReportWhileConnectingIsKeptNotSent() async throws {
        try audio.start(toStage, musicianName: "Ana")
        try #require(session.connection == .connecting)

        try receivers.only().listener.onLinkStats(Self.report)
        await settle()

        #expect(audio.linkReport == Self.report)
        #expect(try transport.sent(ofType: "stats").isEmpty)
    }

    @Test("a new session starts the diagnostics again; an ended one no longer knows its link")
    func diagnosticsBelongToOneSession() async throws {
        try await connected()
        let receiver = try receivers.only()
        receiver.listener.onStats(Self.stats)
        receiver.listener.onLinkStats(Self.report)
        receiver.listener.onError(ReceiveError.tooManyChannels(40))
        await settle()

        audio.stop()

        #expect(audio.linkReport == nil)
        #expect(audio.receiverStats == Self.stats)
        #expect(audio.receiverError as? ReceiveError == .tooManyChannels(40))
        // Android keeps the counters of an ended session on show until the next one starts
        // (`MonitorReducer.kt:78-81` resets only the link health).
        #expect(audio.underrunTotal == Self.report.underruns)

        try receiver.finishStopping()
        await settle()
        try audio.start(toStage, musicianName: "Ana")

        #expect(audio.receiverStats == nil)
        #expect(audio.linkReport == nil)
        #expect(audio.receiverError == nil)
        #expect(audio.underrunTotal == 0)
    }

    /// The T8c verifier's A2: the screens read the figures inside ``MonitorAudio/stateDidChange``,
    /// so the change the session reports as it starts must already show none of the last session's.
    @Test("every change a start reports shows none of the last session's figures")
    func aStartShowsNoFigureOfTheLastSession() async throws {
        try await connected()
        let receiver = try receivers.only()
        receiver.listener.onStats(Self.stats)
        receiver.listener.onLinkStats(Self.report)
        receiver.listener.onError(ReceiveError.tooManyChannels(40))
        await settle()
        audio.stop()
        try receiver.finishStopping()
        await settle()
        var stale: [String] = []
        audio.stateDidChange = { [weak audio] in
            guard let audio, audio.isRunning else { return }
            if audio.receiverStats != nil { stale.append("stats") }
            if audio.underrunTotal != 0 { stale.append("underruns") }
            if audio.receiverError != nil { stale.append("error") }
        }

        try audio.start(toStage, musicianName: "Ana")

        #expect(stale.isEmpty, "\(stale)")
    }

    /// Android keeps the last session's diagnostics on show after a connect it refused, since only
    /// `SessionStarted` starts them again (`MonitorReducer.kt:70-72`).
    @Test("a start the session refuses keeps the last session's figures")
    func aRefusedStartKeepsTheLastSessionsFigures() async throws {
        try await connected()
        let receiver = try receivers.only()
        receiver.listener.onStats(Self.stats)
        receiver.listener.onLinkStats(Self.report)
        receiver.listener.onError(ReceiveError.tooManyChannels(40))
        await settle()
        audio.stop()
        try receiver.finishStopping()
        await settle()

        #expect(throws: ClientMessageError.udpPortMustNotBeZero) {
            try audio.start(ConnectionRequest(endpoint: stage, audioPort: 0), musicianName: "Ana")
        }

        #expect(audio.receiverStats == Self.stats)
        #expect(audio.underrunTotal == Self.report.underruns)
        #expect(audio.receiverError as? ReceiveError == .tooManyChannels(40))
    }

    // MARK: Whether a session runs

    /// The screens choose Connect or the tabs by it. It agrees with
    /// ``BandEarMix/MixSession/isSessionRunning`` at every change the coordinator reports, its start
    /// included, so a screen reading either never sees one session in two states.
    @Test("says whether a session runs, agreeing with the mix session at every change it reports")
    func isRunningAgreesWithTheMixSession() async throws {
        var heard: [Bool] = []
        var disagreements: [String] = []
        audio.stateDidChange = { [weak audio, session] in
            guard let audio else { return }
            heard.append(audio.isRunning)
            if audio.isRunning != session.mixSession.isSessionRunning {
                disagreements.append("running \(audio.isRunning) after change \(heard.count)")
            }
        }
        let receiver = try receivers.only()
        #expect(!audio.isRunning)

        try audio.start(toStage, musicianName: "Ana")
        #expect(audio.isRunning)
        await desk.open()
        audio.stop()
        #expect(!audio.isRunning)
        try receiver.finishStopping()
        await settle()

        try audio.start(toStage, musicianName: "Ana")
        try receiver.endOnItsOwn()
        await settle()
        #expect(!audio.isRunning)

        try audio.start(toStage, musicianName: "Ana")
        session.stop()
        #expect(!audio.isRunning)

        #expect(disagreements.isEmpty)
        #expect(heard.contains(true) && heard.contains(false))
    }

    @Test("a start refused while the previous receiver stops leaves no session running")
    func aRefusedStartRunsNothing() async throws {
        try await connected()
        audio.stop()

        #expect(throws: MonitorAudioError.previousReceiverStopping) {
            try audio.start(toStage, musicianName: "Ana")
        }

        #expect(!audio.isRunning)
    }

    @Test("a start the session refuses leaves no session running and launches nothing")
    func aStartTheSessionRefusesRunsNothing() throws {
        #expect(throws: ClientMessageError.udpPortMustNotBeZero) {
            try audio.start(ConnectionRequest(endpoint: stage, audioPort: 0), musicianName: "Ana")
        }

        #expect(!audio.isRunning)
        #expect(try receivers.only().launches.isEmpty)
        #expect(transport.opened.isEmpty)

        // Nothing was left half started: the next start runs as usual.
        try audio.start(toStage, musicianName: "Ana")

        #expect(audio.isRunning)
        #expect(try receivers.only().launches == [toStage.audioPort])
    }

    // MARK: Reports that belong to no running session (the T7f verifier's AD1 and AD2)

    /// AD1. The musician stops and connects again within one turn of the main queue, while the last
    /// launch's reports are still on their way to it: each report says which launch made it, and the
    /// new session's diagnostics take only its own.
    @Test("reports of the last launch, still queued when the next session starts, touch none of it")
    func reportsOfThePreviousLaunchAreDropped() async throws {
        try await connected()
        let receiver = try receivers.only()
        let stats = Self.stats
        let report = Self.report
        #expect(
            onReceiveThread(receiver.listener) { listener in
                listener.onStats(stats)
                listener.onLinkStats(report)
                listener.onError(ReceiveError.tooManyChannels(40))
            }, "the receive thread hung")
        audio.stop()
        try receiver.finishStopping()

        try audio.start(toStage, musicianName: "Ana")
        await settle()

        #expect(audio.receiverStats == nil)
        #expect(audio.linkReport == nil)
        #expect(audio.receiverError == nil)
        #expect(audio.underrunTotal == 0)
        #expect(audio.isRunning)
        #expect(session.mixSession.isSessionRunning)

        // The new launch's own reports still count.
        receiver.listener.onStats(stats)
        receiver.listener.onLinkStats(report)
        await settle()

        #expect(audio.receiverStats == stats)
        #expect(audio.linkReport == report)
        #expect(audio.underrunTotal == report.underruns)
    }

    /// AD2. The loop's last report can reach the main actor after Stop. Android sets its link health
    /// from it all the same (`MonitorReducer.kt:177`); here it describes a link no longer known.
    @Test("a link report that arrives after stop is neither kept, nor counted, nor sent")
    func aLinkReportAfterStopIsDropped() async throws {
        try await connected()
        let receiver = try receivers.only()
        let report = Self.report
        let sent = try transport.sent(ofType: "stats").count

        audio.stop()
        #expect(onReceiveThread(receiver.listener) { $0.onLinkStats(report) }, "the receive thread hung")
        await settle()

        #expect(audio.linkReport == nil)
        #expect(audio.underrunTotal == 0)
        #expect(try transport.sent(ofType: "stats").count == sent)
    }

    @Test("a link report that arrives after the session stopped directly is dropped too")
    func aLinkReportAfterTheSessionStoppedIsDropped() async throws {
        try await connected()
        let receiver = try receivers.only()

        receiver.listener.onLinkStats(Self.report)
        session.stop()
        await settle()

        #expect(audio.linkReport == nil)
        #expect(audio.underrunTotal == 0)
    }

    // MARK: The listener's owner (the T7f verifier's AD3)

    /// AD3. The listener looks its coordinator up inside the block the main queue runs, never on
    /// the receive thread that hands the report over, so the last reference to the coordinator is
    /// never let go off the main actor. Seen here through a report handed over before the
    /// coordinator attached itself: looked up on handing over, it would find no one.
    @Test("looks its coordinator up when the main actor handles a report, not when it is handed over")
    func readsItsOwnerOnTheMainActor() async throws {
        let session = ControlSession(
            transport: FakeTransport(), scheduler: ManualScheduler(), store: FakeMusicianStore())
        let audio = MonitorAudio(
            session: session, sinkFactory: FakeSinkFactory(),
            makeReceiver: { sinkFactory, controls, listener in
                listener.onError(ReceiveError.tooManyChannels(40))
                return FakeReceiver(sinkFactory, controls, listener)
            })

        await settle()

        #expect(audio.receiverError as? ReceiveError == .tooManyChannels(40))
    }

    @Test("tells its observer after every change of the session and every report")
    func notifiesItsObserver() async throws {
        var changes = 0
        audio.stateDidChange = { changes += 1 }

        try audio.start(toStage, musicianName: "Ana")
        let started = changes
        #expect(started >= 1)
        await desk.open()
        #expect(changes == started + 1)
        session.apply(.masterVolumeChanged(level: 40))
        #expect(changes == started + 2)
        try receivers.only().listener.onStats(Self.stats)
        await settle()
        #expect(changes == started + 3)
        audio.stop()
        #expect(changes >= started + 4)
    }

    // MARK: The hop to the main actor

    @Test("handles the receiver's reports on the main actor, never on the receive thread")
    func hopsToTheMainActor() async throws {
        try await connected()
        let receiver = try receivers.only()
        var changesOnMain: [Bool] = []
        audio.stateDidChange = { changesOnMain.append(Thread.isMainThread) }
        let stats = Self.stats
        let report = Self.report

        // The main thread waits here for the receive thread, so a listener that waited for the main
        // thread would never return.
        #expect(
            onReceiveThread(receiver.listener) { listener in
                listener.onStats(stats)
                listener.onLinkStats(report)
                listener.onError(ReceiveError.tooManyChannels(40))
            }, "the receive thread hung")

        // Handed over, not handled yet.
        #expect(audio.receiverStats == nil)
        #expect(audio.linkReport == nil)
        #expect(audio.receiverError == nil)
        #expect(try transport.sent(ofType: "stats").isEmpty)

        await settle()

        #expect(audio.receiverStats == stats)
        #expect(audio.linkReport == report)
        #expect(audio.receiverError as? ReceiveError == .tooManyChannels(40))
        #expect(try transport.sent(ofType: "stats").count == 1)
        #expect(changesOnMain == [true, true, true])

        #expect(onReceiveThread(receiver.listener) { $0.onStopped() }, "the receive thread hung")
        #expect(session.mixSession.isSessionRunning)

        await settle()

        #expect(!session.mixSession.isSessionRunning)
        #expect(transport.closes == 1)
        #expect(changesOnMain.allSatisfy { $0 })
    }
}

/// `message` as the desk reads it.
private func wireObject(_ message: ClientMessage) throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: message.encodedJSON()) as? [String: Any])
}

/// Calls `report` with `listener` from a thread of its own, as the receive thread calls it, and
/// waits for that thread to finish: false when it does not within 5 s. Synchronous, so the main
/// thread is blocked meanwhile, and a listener that waited for it would hang until the timeout.
private func onReceiveThread(
    _ listener: any ReceiverListener, _ report: @escaping @Sendable (any ReceiverListener) -> Void
) -> Bool {
    let handed = HandedToThread(listener)
    let returned = DispatchSemaphore(value: 0)
    Thread {
        report(handed.value)
        returned.signal()
    }.start()
    return returned.wait(timeout: .now() + 5) == .success
}

/// The listener a receive thread is handed. The coordinator's is made for that: every call hands
/// its value to the main queue and returns.
private struct HandedToThread: @unchecked Sendable {
    let value: any ReceiverListener

    init(_ value: any ReceiverListener) {
        self.value = value
    }
}
