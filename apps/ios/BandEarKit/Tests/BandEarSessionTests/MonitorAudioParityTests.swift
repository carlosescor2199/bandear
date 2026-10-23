import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// The Android client's tests of its view model around the player, from `MonitorViewModelTest.kt`
/// (`apps/android/app/src/test/java/com/bandear/android/ui/state/`): what reaches the receive
/// thread, what reaches the desk, and how a session's receiver starts and stops with it. Driven as
/// Android drives them: a ``FakeReceiver`` the test ends by hand, and the real session over a
/// ``FakeTransport``.
///
/// Where Android reads its receiver's `ClientMixState`, these read the ``PlayerControls`` the fake
/// receiver was handed, as the receive loop reads them. Where Android's state shows a session
/// running, a closed socket or a diagnostic, these read the session and the coordinator. Android's
/// `connection.problem` for a refused Connect is the error ``MonitorAudio/start(_:musicianName:)``
/// throws, in the same words.
@Suite("Monitor audio: Android parity")
@MainActor
struct MonitorAudioParityTests {
    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let receivers = FakeReceivers()
    private let session: ControlSession
    private let desk: FakeDesk
    private let audio: MonitorAudio

    init() {
        session = ControlSession(
            transport: transport, scheduler: scheduler, store: FakeMusicianStore())
        desk = FakeDesk(transport)
        audio = MonitorAudio(
            session: session, sinkFactory: FakeSinkFactory(), makeReceiver: receivers.make)
    }

    /// Android's `connected(channels)`: the session starts for Ana, the socket opens, and the desk
    /// announces `channels` channels.
    private func connected(channels: Int = 2) async throws {
        try audio.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
    }

    /// Android's `engineerState(limit, locked)`: a push carrying what the engineer set.
    private func engineerState(limit: Int, locked: Int...) -> [String: Any] {
        var object = mixState(levels: [])
        object["engineer_limit_percent"] = limit
        object["locked"] = (0..<32).map { locked.contains($0) }
        return object
    }

    // MARK: Starting and stopping with the session

    // MonitorViewModelTest.kt:139
    @Test func connectingToAFoundServerOpensItsControlChannelAndStartsTheReceiver() throws {
        try audio.start(toStage, musicianName: "Ana")

        let receiver = try receivers.only()
        #expect(transport.opened == [stage])
        #expect(receiver.launches == [toStage.audioPort])
        #expect(receiver.isAlive)
        #expect(session.mixSession.isSessionRunning)
        #expect(session.connection == .connecting)
    }

    // MonitorViewModelTest.kt:165, the receiver's half: whether the desk owns the mix reaches the
    // receive thread, and a lost connection leaves both it and the receiver as they were.
    @Test func controlStateChangesMoveThePill() async throws {
        try audio.start(toStage, musicianName: "Ana")
        let receiver = try receivers.only()
        await desk.open()
        #expect(!receiver.controls.snapshot.serverOwnsMix)

        // Android's bare `{"type":"mix_ack"}`; this decoder needs a number or a mix to read one (see
        // Follow-ups, "T6 inputs from T5c"), so the desk answers with its mix and no number.
        try await desk.receive(mixState(levels: [], type: "mix_ack"))
        #expect(session.mixHeldByDesk)
        #expect(receiver.controls.snapshot.serverOwnsMix)

        await desk.fails("wifi gone")
        #expect(session.connection == .reconnecting)
        #expect(receiver.controls.snapshot.serverOwnsMix)
        #expect(scheduler.pendingDelays == [.milliseconds(500)])
        #expect(receiver.stops == 0)
        #expect(receiver.isAlive)
    }

    // MonitorViewModelTest.kt:185
    @Test func disconnectStopsTheReceiverClosesTheChannelAndListensAgain() async throws {
        try await connected()
        let receiver = try receivers.only()

        audio.stop()

        #expect(receiver.stops == 1)
        #expect(transport.closes == 1)
        #expect(!session.mixSession.isSessionRunning)
        #expect(session.connection == .idle)

        audio.stop()

        #expect(receiver.stops == 1)
        #expect(transport.closes == 1)
    }

    // MonitorViewModelTest.kt:203
    @Test func aReceiverThatEndsOnItsOwnEndsTheSession() async throws {
        try await connected()
        let receiver = try receivers.only()

        try receiver.endOnItsOwn()
        await settle()

        #expect(transport.closes == 1)
        #expect(!session.mixSession.isSessionRunning)
        #expect(session.connection == .idle)
        #expect(scheduler.pendingDelays.isEmpty)
    }

    // MonitorViewModelTest.kt:216
    @Test func theReceiversLateStopReportAfterADisconnectEndsNothingAgain() async throws {
        try await connected()
        let receiver = try receivers.only()
        audio.stop()
        let ended = session.mixSession

        try receiver.finishStopping()
        await settle()

        #expect(transport.closes == 1)
        #expect(session.connection == .idle)
        #expect(session.mixSession == ended)
    }

    // MonitorViewModelTest.kt:229
    @Test func aReceiverThatEndsOnItsOwnEndsTheSessionOnlyOnce() async throws {
        try await connected()
        let receiver = try receivers.only()
        try receiver.endOnItsOwn()
        await settle()

        receiver.listener.onStopped()
        await settle()
        audio.stop()

        #expect(transport.closes == 1)
        #expect(receiver.stops == 0)
        #expect(!session.mixSession.isSessionRunning)
    }

    // MonitorViewModelTest.kt:243
    @Test func theLastSessionsLateStopReportCannotEndTheNextSession() async throws {
        try await connected()
        let receiver = try receivers.only()
        audio.stop()
        // The musician connects again at once. The receiver counts as alive until the stopped
        // loop's report, so this start is refused rather than start a session the late report
        // could reach.
        #expect(throws: MonitorAudioError.previousReceiverStopping) {
            try audio.start(toStage, musicianName: "Ana")
        }
        #expect(transport.opened.count == 1)

        try receiver.finishStopping()
        await settle()
        try audio.start(toStage, musicianName: "Ana")

        #expect(transport.opened.count == 2)
        #expect(transport.closes == 1)
        #expect(receiver.launches.count == 2)
        #expect(receiver.isAlive)
        #expect(session.mixSession.isSessionRunning)

        // The next session ends only when its own receive loop does.
        try receiver.endOnItsOwn()
        await settle()

        #expect(transport.closes == 2)
        #expect(!session.mixSession.isSessionRunning)
        #expect(session.connection == .idle)
    }

    // MonitorViewModelTest.kt:272. Android's loop lowers the shared session flag before it reports,
    // so its Stop finds the flag down and does nothing, and the report ends the session. Here the
    // flag is the receiver's own and the coordinator hears of the end only from the report, so Stop
    // ends the session at once and the report then ends nothing: the same one session ends, once.
    @Test func aLoopEndingWhileTheMusicianStopsAndReconnectsEndsOnlyItsOwnSession() async throws {
        try await connected()
        let receiver = try receivers.only()
        var refusal: (any Error)?

        // The loop ends on its own, and before its report the musician taps Stop and Connect.
        try receiver.endOnItsOwn(beforeReport: {
            audio.stop()
            do {
                try audio.start(toStage, musicianName: "Ana")
            } catch {
                refusal = error
            }
        })
        await settle()

        // Connect was refused while the loop was still reporting.
        #expect(refusal as? MonitorAudioError == .previousReceiverStopping)
        #expect(transport.opened.count == 1)
        #expect(transport.closes == 1)
        #expect(!session.mixSession.isSessionRunning)
        #expect(!receiver.isAlive)

        // The second session's report is counted as the second: after Stop it ends nothing again.
        try audio.start(toStage, musicianName: "Ana")
        audio.stop()
        try receiver.finishStopping()
        await settle()

        #expect(transport.opened.count == 2)
        #expect(transport.closes == 2)
        #expect(!receiver.isAlive)
    }

    // MonitorViewModelTest.kt:372
    @Test func aSecondConnectWhileRunningIsIgnored() throws {
        try audio.start(toStage, musicianName: "Ana")

        try audio.start(
            ConnectionRequest(
                endpoint: .manual(host: "10.0.0.9", controlPort: 50_001), audioPort: 50_000),
            musicianName: "Ana")

        #expect(transport.opened == [stage])
        #expect(try receivers.only().launches.count == 1)
    }

    // MonitorViewModelTest.kt:382
    @Test func connectWaitsForThePreviousReceiverToEnd() async throws {
        try await connected()
        let receiver = try receivers.only()
        audio.stop()

        // The stopped loop has not reported yet, so its receiver still counts as alive.
        let refusal = #expect(throws: MonitorAudioError.self) {
            try audio.start(toStage, musicianName: "Ana")
        }

        #expect(refusal == .previousReceiverStopping)
        #expect(refusal.map(String.init(describing:)) == "The previous receiver is still stopping")
        #expect(transport.opened.count == 1)
        #expect(receiver.launches.count == 1)
        #expect(!session.mixSession.isSessionRunning)
    }

    // MARK: What reaches the receive thread

    // MonitorViewModelTest.kt:542
    @Test func theEngineersLimitHoldsThePhonesOwnPlaybackAndIsNeverSent() async throws {
        try await connected(channels: 3)
        try await desk.answerMixes()

        try await desk.receive(engineerState(limit: 60))
        session.apply(.levelCapChanged(level: 90))

        #expect(session.mixSession.mix.effectiveCap == 60)
        #expect(try receivers.only().controls.snapshot.maxLevelPercent == 60)
        let mix = try desk.lastMix()
        #expect(mix["max_level_percent"] as? Int == 90)
        #expect(mix["engineer_limit_percent"] == nil)
        #expect(mix["locked"] == nil)
    }

    // MonitorViewModelTest.kt:575
    @Test func aLimitOnlyReportMovesThePhonesOwnCeilingWithNoMasterEdit() async throws {
        try await connected(channels: 3)
        try await desk.answerMixes()
        let receiver = try receivers.only()
        #expect(receiver.controls.snapshot.maxLevelPercent == 100)

        try await desk.receive(engineerState(limit: 60))

        #expect(receiver.controls.snapshot.maxLevelPercent == 60)

        // An answer that only lowers the limit moves it too; the change it answers kept the cap.
        session.apply(.channelLevelChanged(channel: 0, level: 40))
        var answer = try desk.lastMix()
        answer["type"] = "mix_ack"
        answer["engineer_limit_percent"] = 45
        answer["locked"] = [Bool](repeating: false, count: 32)
        try await desk.receive(answer)

        #expect(receiver.controls.snapshot.maxLevelPercent == 45)
        #expect(session.mixSession.mix.levelCap == 100)
    }

    // MonitorViewModelTest.kt:739
    @Test func masterChangesReachTheAudioThreadAndTheServer() async throws {
        try await connected()

        session.apply(.masterVolumeChanged(level: 40))
        session.apply(.levelCapChanged(level: 80))
        session.apply(.masterMuteChanged(muted: true))

        let controls = try receivers.only().controls.snapshot
        #expect(controls.volumePercent == 40)
        #expect(controls.maxLevelPercent == 80)
        #expect(controls.muted)
        let mix = try desk.lastMix()
        #expect(mix["volume_percent"] as? Int == 40)
        #expect(mix["max_level_percent"] as? Int == 80)
        #expect(mix["muted"] as? Bool == true)
    }

    // MonitorViewModelTest.kt:1161
    @Test func theDesksBufferReachesTheReceiveThreadAndANewSessionStartsOnAuto() async throws {
        try await connected()
        let receiver = try receivers.only()
        #expect(receiver.controls.snapshot.playbackBuffer == .auto)

        await desk.receive(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
        #expect(receiver.controls.snapshot.playbackBuffer == .fixed(milliseconds: 40))

        audio.stop()
        try receiver.finishStopping()
        await settle()
        try audio.start(toStage, musicianName: "Ana")

        #expect(
            receiver.controls.snapshot.playbackBuffer == .auto,
            "the phone keeps nothing: the desk sends it again")
        #expect(receiver.settingsAtLaunch.map(\.playbackBuffer) == [.auto, .auto])
    }

    // MonitorViewModelTest.kt:1179
    @Test func theHoldForDelayedCopiesReachesTheReceiveThreadAndANewSessionHoldsNothing()
        async throws
    {
        try await connected()
        let receiver = try receivers.only()
        #expect(receiver.controls.snapshot.redundancyHoldPackets == 0)

        await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
        #expect(receiver.controls.snapshot.redundancyHoldPackets == 2)

        audio.stop()
        try receiver.finishStopping()
        await settle()
        try audio.start(toStage, musicianName: "Ana")

        #expect(
            receiver.controls.snapshot.redundancyHoldPackets == 0,
            "the phone keeps nothing: the desk sends it again")
        #expect(receiver.settingsAtLaunch.map(\.redundancyHoldPackets) == [0, 0])
    }

    // MARK: What the receiver reports

    // MonitorViewModelTest.kt:1120. Android also shows a link health from the report; the screens
    // derive it in T8, from the report kept here.
    @Test func receiverReportsReachTheDiagnosticsAndTheServer() async throws {
        try await connected()
        let receiver = try receivers.only()
        let stats = ReceiverStats(
            packets: 500, lost: 5, sampleRate: 48_000, packetsPerSecond: 100,
            interArrivalMilliseconds: 10, jitterMilliseconds: 0.3, discardedSamples: 0,
            packetsWithDiscardedSamples: 0, playbackBacklogMilliseconds: 15)
        let report = LinkStatsReport(
            intervalMilliseconds: 1_000, packetsReceived: 100, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0)

        receiver.listener.onStats(stats)
        receiver.listener.onLinkStats(report)
        receiver.listener.onError(AudioDatagramDecodingError.headerTooShort(byteCount: 3))
        await desk.receive(#"{"type":"error","message":"mix refused"}"#)

        #expect(audio.receiverStats?.packets == 500)
        #expect(audio.receiverStats?.playbackBacklogMilliseconds == 15)
        #expect(audio.linkReport == report)
        let sent = try transport.sent(ofType: "stats")
        try #require(sent.count == 1)
        #expect(sent[0]["packets_received"] as? Int == 100)
        #expect(
            audio.receiverError as? AudioDatagramDecodingError == .headerTooShort(byteCount: 3))
        #expect(session.lastError == DeskError(message: "mix refused", sequence: nil))
    }
}
