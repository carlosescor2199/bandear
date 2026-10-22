import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// What the reconnect parity suite reaches only indirectly: changes made while the connection is
/// down, a connection ending once, which reason an outage keeps and when it is forgotten, a retry
/// outlived by its session, and the order ``ControlSession/start(_:musicianName:clientID:)`` and
/// ``ControlSession/stop()`` change things in.
@Suite("Control session: reconnecting")
@MainActor
struct ControlSessionReconnectTests {
    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let session: ControlSession
    private let desk: FakeDesk

    init() {
        session = ControlSession(transport: transport, scheduler: scheduler)
        desk = FakeDesk(transport)
    }

    /// The session started for Ana, its socket open and proved by the desk's `config`.
    private func connected() async throws {
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: 2))
    }

    // MARK: While the connection is down

    @Test("keeps reconnecting until the next connection opens, and that open sends the newest mix")
    func aChangeWhileReconnectingGoesOutOnTheNextOpen() async throws {
        try await connected()
        await desk.fails("wifi gone")
        let sent = transport.sent.count

        session.apply(.masterVolumeChanged(level: 40))
        try scheduler.runNextDue()

        #expect(transport.sent.count == sent)
        #expect(session.connection == .reconnecting)
        try #require(transport.attempts.count == 2)

        await desk.open()

        #expect(session.connection == .connected)
        let mixes = try transport.attempts[1].sent(ofType: "mix")
        try #require(mixes.count == 1)
        #expect(mixes[0]["seq"] as? Int == Int(session.mixSession.sync.latest))
        #expect(mixes[0]["volume_percent"] as? Int == 40)
        #expect(session.mixSession.sync.latest == 1)
    }

    /// Android's `ConnectionGenerationsTest.theEndOfTheCurrentConnectionIsHandledExactlyOnce`,
    /// through the session: a close and a failure racing on one connection end it once.
    @Test("ends a connection once: a failure after its close schedules nothing and says nothing")
    func aConnectionEndsOnce() async throws {
        try await connected()

        desk.post(.closed)
        desk.post(.failed("reset after close"))
        await settle()

        #expect(scheduler.pending.count == 1)
        #expect(session.connectionError == nil)
    }

    // MARK: The reason an outage is reported with

    @Test("keeps the reason an outage was first reported with while its retries keep failing")
    func anOutageKeepsItsFirstReason() async throws {
        try await connected()

        await desk.fails("refused 1")
        try scheduler.runNextDue()
        await desk.fails("refused 2")

        #expect(session.connectionError == "refused 1")
    }

    @Test("reports a failure that follows a close in the same outage")
    func aFailureAfterACloseIsReported() async throws {
        try await connected()
        await desk.closes()
        try #require(session.connectionError == nil)
        try scheduler.runNextDue()

        await desk.fails("refused")

        #expect(session.connectionError == "refused")
    }

    @Test("keeps the reason through an open the desk never spoke on, and forgets it once it speaks")
    func theReasonLastsUntilTheDeskSpeaks() async throws {
        try await connected()
        await desk.fails("wifi gone")
        try scheduler.runNextDue()

        await desk.open()

        #expect(session.connection == .connected)
        #expect(session.connectionError == "wifi gone")

        await desk.receive(configMessage(channels: 2))

        #expect(session.connectionError == nil)
    }

    @Test("forgets the reason on stop and on a new start")
    func stopAndStartForgetTheReason() async throws {
        try await connected()
        await desk.fails("wifi gone")
        try #require(session.connectionError == "wifi gone")

        session.stop()
        #expect(session.connectionError == nil)

        try session.start(toStage, musicianName: "Ana")
        await desk.fails("refused")
        try #require(session.connectionError == "refused")
        try session.start(toStage, musicianName: "Ana")

        #expect(session.connectionError == nil)
    }

    // MARK: Retries and sessions

    @Test("stop while reconnecting cancels the retry and leaves the session idle")
    func stopWhileReconnecting() async throws {
        try await connected()
        await desk.fails("wifi gone")
        try #require(session.connection == .reconnecting)

        session.stop()

        #expect(session.connection == .idle)
        #expect(scheduler.pending.isEmpty)
        #expect(!session.mixSession.isSessionRunning)
    }

    @Test("a new start while reconnecting opens at once, and the old retry, run anyway, opens nothing")
    func aStartOutlivesTheRetry() async throws {
        try await connected()
        await desk.fails("wifi gone")
        try #require(scheduler.pending.count == 1)
        let retry = scheduler.pending[0]
        let other = DeskEndpoint.manual(host: "192.168.1.20", controlPort: 50_001)

        try session.start(ConnectionRequest(endpoint: other, audioPort: 50_000), musicianName: "Ana")

        #expect(retry.cancelled)
        #expect(transport.opened == [stage, other])
        #expect(session.connection == .connecting)

        retry.run()
        await settle()

        #expect(transport.opened == [stage, other])
    }

    @Test("a new session starts the backoff from its shortest wait")
    func aStartResetsTheBackoff() async throws {
        try session.start(toStage, musicianName: "Ana")
        for _ in 0..<3 {
            await desk.fails("refused")
            try scheduler.runNextDue()
        }

        try session.start(toStage, musicianName: "Ana")
        await desk.fails("refused")

        #expect(scheduler.pendingDelays == [.milliseconds(500)])
    }

    @Test("a retry whose session ended without a stop opens nothing and leaves the session idle")
    func aRetryAfterTheSessionEndedOpensNothing() async throws {
        try await connected()
        await desk.fails("wifi gone")
        session.apply(.sessionEnded)

        try scheduler.runNextDue()

        #expect(transport.attempts.count == 1)
        #expect(session.connection == .idle)
    }

    // MARK: The order start and stop change things in (T6b's review)

    /// Neither ``MixEvent/sessionStarted`` nor ``MixEvent/sessionEnded`` numbers a send today, so
    /// this pins what the transport sees rather than catching a send that cannot happen: the
    /// connection is set aside before the session's own event is reduced.
    @Test("a start that replaces a connected session sends nothing to the previous connection")
    func aReplacingStartSendsNothingToThePreviousConnection() async throws {
        try await connected()
        try #require(session.connection == .connected)
        let before = transport.calls.count
        let sentOnTheFirst = transport.attempts[0].sent.count
        let other = DeskEndpoint.manual(host: "192.168.1.20", controlPort: 50_001)

        try session.start(ConnectionRequest(endpoint: other, audioPort: 50_000), musicianName: "Ana")

        #expect(Array(transport.calls[before...]) == [.open(other)])
        #expect(transport.attempts[0].sent.count == sentOnTheFirst)
    }

    /// See ``aReplacingStartSendsNothingToThePreviousConnection()``.
    @Test("stop from connected closes the connection and sends nothing")
    func stopFromConnectedSendsNothing() async throws {
        try await connected()
        try #require(session.connection == .connected)
        let before = transport.calls.count

        session.stop()

        #expect(Array(transport.calls[before...]) == [.close])
    }
}

/// The scheduler the app uses, on the real main queue. Each test waits for its last piece of work
/// to run rather than for time to pass, so it never sleeps.
@Suite("Main queue scheduler")
@MainActor
struct MainQueueSchedulerTests {
    /// What ran, in order, and whether on the main thread.
    @MainActor
    private final class Log {
        var ran: [String] = []
        var onMain: [Bool] = []

        func record(_ name: String) {
            ran.append(name)
            onMain.append(Thread.isMainThread)
        }
    }

    @Test("runs work on the main actor after its delay, in the order of the delays, never at once")
    func runsInTheOrderOfTheDelays() async {
        let scheduler = MainQueueScheduler()
        let log = Log()

        let finished = await waitUntilDone { done in
            _ = scheduler.schedule(after: .milliseconds(80)) {
                log.record("later")
                done()
            }
            _ = scheduler.schedule(after: .milliseconds(10)) { log.record("sooner") }
            #expect(log.ran.isEmpty)
        }

        #expect(finished, "the later work never ran")
        #expect(log.ran == ["sooner", "later"])
        #expect(log.onMain == [true, true])
    }

    /// The cancelled work is due first, so had it been going to run, it would have run first.
    @Test("never runs work that was cancelled")
    func cancelledWorkNeverRuns() async {
        let scheduler = MainQueueScheduler()
        let log = Log()

        let finished = await waitUntilDone { done in
            scheduler.schedule(after: .milliseconds(10)) { log.record("cancelled") }.cancel()
            _ = scheduler.schedule(after: .milliseconds(60)) {
                log.record("kept")
                done()
            }
        }

        #expect(finished, "the kept work never ran")
        #expect(log.ran == ["kept"])
    }
}
