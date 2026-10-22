import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// The Android client's `ControlChannelReconnectTest` (`apps/android/app/src/test/java/com/bandear/
/// android/ControlChannelReconnectTest.kt`), driven as Android drives it: a transport whose
/// connections the test opens, fails and closes by hand, and a scheduler that runs a retry only when
/// the test says so. Each of Android's `FakeSocket`s is one of ``FakeTransport/attempts``.
///
/// Where Android's listener counts errors, the session says why the connection was lost in
/// ``ControlSession/connectionError``, once per outage; ``ConnectionErrorReports`` counts each time
/// it says so. Two of the file's tests were ported with the opening ones in T6b
/// (`aSessionOpensOneConnectionToTheGivenServer`, `stopClosesASocketThatIsStillOpening`, in
/// `ControlSessionParityTests`); `theClientPingsOftenEnoughForTheServerButNotAggressively` is about
/// the transport and is ported with it (`WebSocketTransportTests`).
@Suite("Control session: Android reconnect parity")
@MainActor
struct ControlSessionReconnectParityTests {
    private static let config = configMessage(channels: 2)

    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let session: ControlSession
    private let desk: FakeDesk

    init() {
        session = ControlSession(transport: transport, scheduler: scheduler)
        desk = FakeDesk(transport)
    }

    /// Android's `startSession`.
    private func startSession() throws {
        try session.start(toStage, musicianName: "Ana")
    }

    /// Android's `openAndProve`: the newest connection opens and the desk proves it with its first
    /// message.
    private func openAndProve() async {
        await desk.open()
        await desk.receive(Self.config)
    }

    @Test func everyOpenRegistersFirstAndThenSendsTheMix() async throws {
        try startSession()
        await openAndProve()
        await desk.fails("wifi gone")
        try scheduler.runNextDue()

        try #require(transport.attempts.count == 2)
        await desk.open()

        #expect(try transport.attempts[0].sentTypes() == ["register", "mix"])
        #expect(try transport.attempts[1].sentTypes() == ["register", "mix"])
        #expect(transport.attempts[1].endpoint == stage)
    }

    @Test func aFailureWhileRunningSchedulesARetryAfterThePolicyDelay() async throws {
        try startSession()
        await openAndProve()

        await desk.fails("connection reset")

        #expect(session.connection == .reconnecting)
        #expect(scheduler.pendingDelays == [.milliseconds(500)])
        #expect(transport.attempts.count == 1)

        try scheduler.runNextDue()

        #expect(transport.attempts.count == 2)
    }

    /// Android sees a close as `onClosing` and then `onClosed`, and answers the first with a close of
    /// its own. ``WebSocketTransport`` reports the desk's close once and lets the connection go; the
    /// second report stands in for Android's `onClosed`, which belongs to a connection that ended.
    /// The answer is the transport's: the session never closes a connection the desk closed.
    @Test func aServerClosingTheConnectionIsAnsweredAndRetried() async throws {
        try startSession()
        await openAndProve()

        desk.post(.closed)
        desk.post(.closed)
        await settle()

        #expect(session.connection == .reconnecting)
        #expect(scheduler.pendingDelays == [.milliseconds(500)])
    }

    @Test func stopCancelsAPendingRetryAndNothingReconnectsAfterwards() async throws {
        try startSession()
        await openAndProve()
        await desk.fails("connection reset")
        try #require(scheduler.pending.count == 1)
        let pending = scheduler.pending[0]

        session.stop()

        #expect(pending.cancelled)
        #expect(scheduler.pending.isEmpty)
        // Even an attempt that was already due when Stop came does not connect.
        pending.run()
        await settle()
        #expect(transport.attempts.count == 1)
    }

    /// Android's late ack is `{"type":"mix_ack"}`, which Android counts as acknowledged before reading
    /// anything else; this decoder cannot read it (T5c), so it would be ignored from any connection.
    /// The late ack here is a readable one, which the current connection would have counted.
    @Test func aLateCallbackFromAnOlderAttemptDoesNotTouchTheCurrentConnection() async throws {
        try startSession()
        await openAndProve()
        await desk.fails("connection reset")
        try scheduler.runNextDue()
        try #require(transport.attempts.count == 2)
        await openAndProve()
        try #require(session.connectionError == nil && !session.mixHeldByDesk)
        let old = transport.attempts[0]

        old.sink(.failed("late failure"))
        old.sink(.closed)
        old.sink(.received(try text(mixState(levels: [80, 70], type: "mix_ack"))))
        session.apply(.masterVolumeChanged(level: 40))
        await settle()

        #expect(session.connection == .connected)
        #expect(scheduler.pending.isEmpty)
        #expect(!session.mixHeldByDesk)
        #expect(session.connectionError == nil)
        #expect(session.lastError == nil)
        #expect(try transport.attempts[1].sentTypes() == ["register", "mix", "mix"])
    }

    @Test func aServerThatClosesRightAfterOpenBacksOffToTheCap() async throws {
        try startSession()

        var delays: [Duration] = []
        for _ in 0..<6 {
            await desk.open()
            await desk.closes()
            try #require(scheduler.pending.count == 1)
            delays += scheduler.pendingDelays
            try scheduler.runNextDue()
        }

        #expect(
            delays == [
                .milliseconds(500), .seconds(1), .seconds(2), .seconds(4), .seconds(5), .seconds(5),
            ])
    }

    @Test func aConnectionThatHearsFromTheServerStartsTheBackoffAgain() async throws {
        try startSession()
        for _ in 0..<3 {
            await desk.fails("refused")
            try scheduler.runNextDue()
        }
        await openAndProve()

        await desk.fails("wifi gone")

        #expect(scheduler.pendingDelays == [.milliseconds(500)])
    }

    /// Android prefixes the reason with "WebSocket error: "; the session keeps the transport's own
    /// words, and the screen says them (T8).
    @Test func theErrorIsReportedOncePerOutage() async throws {
        let reports = ConnectionErrorReports(watching: session)
        try startSession()
        for _ in 0..<4 {
            await desk.fails("refused")
            #expect(session.connection == .reconnecting)
            try scheduler.runNextDue()
        }

        #expect(reports.all == ["refused"])

        await openAndProve()
        await desk.fails("wifi gone")
        try scheduler.runNextDue()
        await desk.fails("still gone")

        #expect(reports.all == ["refused", "wifi gone"])
        #expect(session.connection == .reconnecting)
    }

    /// Android's receiver clears the shared running flag when it ends on its own. Here the session
    /// ends that way when ``MixEvent/sessionEnded`` reaches it without ``ControlSession/stop()``, as
    /// the player (T7) may send it.
    @Test func theReceiverEndingStopsTheRetriesEvenBeforeTheChannelIsClosed() async throws {
        try startSession()
        await openAndProve()
        await desk.fails("connection reset")
        try #require(scheduler.pending.count == 1)

        session.apply(.sessionEnded)
        try scheduler.runNextDue()

        #expect(transport.attempts.count == 1)
    }
}

/// The two tests of what the Android ViewModel sends that need a reconnect, from
/// `MonitorViewModelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/ui/state/`).
/// Android's `socket()` is the newest connection, ``FakeTransport/attempts``' last.
@Suite("Control session: Android ViewModel reconnect parity")
@MainActor
struct ControlSessionViewModelReconnectParityTests {
    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let session: ControlSession
    private let desk: FakeDesk

    init() {
        session = ControlSession(transport: transport, scheduler: scheduler)
        desk = FakeDesk(transport)
    }

    /// Android's `connected(channels)`.
    private func connected(channels: Int) async throws {
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
    }

    /// The mixes sent on the newest connection: Android's `socket().sentOfType("mix")`.
    private func mixesOnTheNewestConnection() throws -> [[String: Any]] {
        try #require(transport.attempts.last).sent(ofType: "mix")
    }

    @Test func aReconnectStillSendsThePhonesOwnMix() async throws {
        try await connected(channels: 3)
        session.apply(.channelLevelChanged(channel: 1, level: 30))
        try await desk.answerMixes()

        await desk.fails("Wi-Fi dropped")
        try scheduler.runNextDue()
        try #require(transport.attempts.count == 2)
        await desk.open()

        let mixes = try mixesOnTheNewestConnection()
        try #require(mixes.count == 1)
        #expect(ints(mixes[0], "channels") == [100, 30, 100])
    }

    @Test func aReconnectSendsTheMixAgainUnderItsNewestNumberAndItsAnswerSettlesIt() async throws {
        try await connected(channels: 2)
        try await desk.answerMixes()
        session.apply(.channelLevelChanged(channel: 0, level: 40))
        let newest = try #require(try desk.lastMix()["seq"] as? Int)
        await desk.fails("wifi gone")
        try scheduler.runNextDue()
        try #require(transport.attempts.count == 2)

        await desk.open()

        let resent = try #require(try mixesOnTheNewestConnection().last)
        #expect(resent["seq"] as? Int == newest)
        #expect(ints(resent, "channels") == [40, 100])

        // Pushed before the desk answered the mix sent again: that answer is the latest word.
        try await desk.receive(mixState(levels: [80, 70]))

        #expect(session.mixSession.mix.levels == [40, 100])

        try await desk.answerMixes()
        try await desk.receive(mixState(levels: [80, 70]))

        #expect(session.mixSession.mix.levels == [80, 70])
    }
}

/// Each time the session said why its connection was lost: every new value of
/// ``ControlSession/connectionError``, seen through ``ControlSession/stateDidChange``. Android's
/// `RecordingListener.errors`, for the connection's errors.
@MainActor
final class ConnectionErrorReports {
    private(set) var all: [String] = []
    private var shown: String?

    init(watching session: ControlSession) {
        session.stateDidChange = { [weak self, unowned session] in
            guard let self else { return }
            if let error = session.connectionError, error != shown {
                all.append(error)
            }
            shown = session.connectionError
        }
    }
}
