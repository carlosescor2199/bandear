import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// What the parity suites reach only indirectly: the order of what goes out on open, what is
/// dropped while the connection is down, how ``ControlSession/stop()`` differs from a socket
/// closing, and the hop to the main actor.
@Suite("Control session")
@MainActor
struct ControlSessionTests {
    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let store = FakeMusicianStore()
    private let session: ControlSession
    private let desk: FakeDesk

    init() {
        session = ControlSession(transport: transport, scheduler: scheduler, store: store)
        desk = FakeDesk(transport)
    }

    /// The session started for Ana, its socket open and the desk's `channels` announced.
    private func connected(channels: Int = 2) async throws {
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
    }

    // MARK: Opening

    @Test("registers first, with no capabilities, and then sends the opening mix")
    func registersThenSendsTheOpeningMix() async throws {
        store.clientID = "phone-1"
        try session.start(toStage, musicianName: " Ana ")
        #expect(session.connection == .connecting)

        await desk.open()

        #expect(session.connection == .connected)
        #expect(try transport.sentTypes() == ["register", "mix"])
        let sent = try transport.sentObjects()
        try #require(sent.count == 2)
        #expect(
            NSDictionary(dictionary: sent[0])
                == NSDictionary(dictionary: [
                    "type": "register", "name": "Ana", "udp_port": 50_000, "client_id": "phone-1",
                ]))
        #expect(sent[1]["seq"] as? Int == 0)
    }

    @Test("starts nothing for a name the desk would store as no name")
    func aBlankNameStartsNothing() {
        #expect(throws: ClientMessageError.nameIsBlank) {
            try session.start(toStage, musicianName: "   ")
        }
        #expect(transport.opened.isEmpty)
        #expect(session.connection == .idle)
        #expect(!session.mixSession.isSessionRunning)
    }

    // MARK: Sending only while open

    @Test("drops a change made while connecting, and the open sends the newest mix instead")
    func dropsAChangeWhileConnecting() async throws {
        try session.start(toStage, musicianName: "Ana")

        session.apply(.masterVolumeChanged(level: 40))

        #expect(transport.sent.isEmpty)
        #expect(session.mixSession.sync.latest == 1)

        await desk.open()

        #expect(try transport.sentTypes() == ["register", "mix"])
        let mix = try desk.lastMix()
        #expect(mix["seq"] as? Int == 1)
        #expect(mix["volume_percent"] as? Int == 40)
    }

    @Test("drops changes and stats once the desk closed the socket, and the session goes on")
    func dropsWhatIsSentAfterTheDeskCloses() async throws {
        try await connected()
        let sent = transport.sent.count

        await desk.closes()
        session.apply(.masterVolumeChanged(level: 40))
        session.send(
            stats: try LinkStats(
                intervalMilliseconds: 1_000, packetsReceived: 1, packetsLost: 0, underruns: 0,
                receiveTimeouts: 0))

        #expect(session.connection == .reconnecting)
        #expect(transport.sent.count == sent)
        #expect(transport.closes == 0)
        #expect(session.mixSession.isSessionRunning)
        #expect(session.mixSession.mix.masterVolume == 40)
    }

    @Test("sends nothing for stats before the socket opens")
    func dropsStatsWhileConnecting() throws {
        try session.start(toStage, musicianName: "Ana")
        try #require(session.connection == .connecting)

        session.send(
            stats: try LinkStats(
                intervalMilliseconds: 1_000, packetsReceived: 1, packetsLost: 0, underruns: 0,
                receiveTimeouts: 0))

        #expect(transport.sent.isEmpty)
    }

    @Test("keeps the reason a connection failed, and the session goes on")
    func aFailureKeepsItsReason() async throws {
        try await connected()

        desk.post(.failed("connection refused"))
        await settle()

        #expect(session.connection == .reconnecting)
        #expect(session.connectionError == "connection refused")
        #expect(session.mixSession.isSessionRunning)
    }

    @Test("ignores what a connection reports after it ended")
    func ignoresAConnectionAfterItEnded() async throws {
        try await connected()
        await desk.closes()
        try #require(session.connection == .reconnecting)
        try #require(scheduler.pending.count == 1)

        await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
        desk.post(.failed("late failure"))
        await settle()

        #expect(session.redundancyHoldBlocks == 0)
        #expect(session.connection == .reconnecting)
        #expect(session.connectionError == nil)
        #expect(scheduler.pending.count == 1)
    }

    // MARK: Stopping

    @Test("stop ends the session, closes the socket and ignores what arrives after")
    func stopEndsTheSession() async throws {
        try await connected()
        try #require(session.connection == .connected)

        // Both already on their way when the musician stops.
        desk.post(.received(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#))
        desk.post(.closed)
        session.stop()
        await settle()

        #expect(session.connection == .idle)
        #expect(!session.mixSession.isSessionRunning)
        #expect(transport.closes == 1)
        #expect(session.playbackBuffer == .auto)
    }

    @Test("a second start replaces the first: the first connection is no longer heard")
    func aSecondStartReplacesTheFirst() async throws {
        try await connected()
        let first = try #require(transport.sink)
        let other = DeskEndpoint.manual(host: "192.168.1.20", controlPort: 50_001)

        try session.start(ConnectionRequest(endpoint: other, audioPort: 50_000), musicianName: "Ana")
        first(.received(#"{"type":"redundancy","delay_blocks":2}"#))
        first(.closed)
        await settle()

        #expect(transport.opened == [stage, other])
        #expect(session.connection == .connecting)
        #expect(session.redundancyHoldBlocks == 0)

        await desk.open()

        #expect(try transport.sentTypes() == ["register", "mix", "register", "mix"])
    }

    @Test("a new session forgets what the last desk set and keeps the mix and its numbers")
    func startForgetsTheLastDesk() async throws {
        try await connected()
        session.apply(.channelLevelChanged(channel: 0, level: 30))
        try await desk.answerMixes()
        await desk.receive(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
        await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
        await desk.receive(#"{"type":"error","message":"no UDP target configured for client IP"}"#)
        try #require(session.mixHeldByDesk && session.lastError != nil)
        let sync = session.mixSession.sync

        session.stop()
        try session.start(toStage, musicianName: "Ana")

        #expect(!session.mixHeldByDesk)
        #expect(session.playbackBuffer == .auto)
        #expect(session.redundancyHoldBlocks == 0)
        #expect(session.lastError == nil)
        #expect(session.mixSession.isSessionRunning)
        #expect(session.mixSession.sync == sync)
        #expect(session.mixSession.mix.levels == [30, 100])
    }

    // MARK: The hop to the main actor

    @Test("handles an event from the transport's thread on the main actor, not on that thread")
    func hopsToTheMainActor() async throws {
        try session.start(toStage, musicianName: "Ana")
        let sink = try #require(transport.sink)
        var changesOnMain: [Bool] = []
        session.stateDidChange = { changesOnMain.append(Thread.isMainThread) }

        #expect(deliver(.opened, to: sink), "the transport's thread hung")

        #expect(session.connection == .connecting)
        #expect(transport.sent.isEmpty)

        await settle()

        #expect(session.connection == .connected)
        #expect(changesOnMain == [true])
    }

    @Test("handles an event delivered on the main thread after the ones before it, never at once")
    func keepsTheTransportsOrder() async throws {
        try session.start(toStage, musicianName: "Ana")

        desk.post(.opened)
        desk.post(.received(configMessage(channels: 2)))

        #expect(session.connection == .connecting)

        await settle()

        #expect(session.connection == .connected)
        #expect(session.mixSession.mix.levels == [100, 100])
    }

    // MARK: Routing

    @Test("sends a mix when a desk message numbers one: a config that turns More of me off")
    func aDeskMessageThatNumbersAChangeSendsIt() async throws {
        try await connected(channels: 3)
        session.apply(.musicianChannelChosen(2))
        session.apply(.moreOfMeToggled(on: true))
        try await desk.answerMixes()
        let mixes = try transport.sent(ofType: "mix").count

        await desk.receive(configMessage(channels: 2))

        #expect(!session.mixSession.moreOfMe.isEnabled)
        #expect(try transport.sent(ofType: "mix").count == mixes + 1)
        #expect(ints(try desk.lastMix(), "channels") == [100, 100])
    }

    /// The report the reducer numbers itself: after a link turned More of me off, the link's own
    /// `mix_state` still carries the lift, and the phone answers it with the musician's own levels.
    @Test("sends a mix when a desk report numbers one: the musician's own levels after a link")
    func aDeskReportThatNumbersAChangeSendsIt() async throws {
        let unlinked = #"{"name":"A","link":false},{"name":"B","link":false},{"name":"C","link":false}"#
        let linked = #"{"name":"A","link":true},{"name":"B","link":false},{"name":"C","link":false}"#
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(#"{"type":"config","source_channels":3,"groups":[],"channels":[\#(unlinked)]}"#)
        session.apply(.musicianChannelChosen(1))
        session.apply(.moreOfMeToggled(on: true))
        try await desk.answerMixes()
        await desk.receive(#"{"type":"config","source_channels":3,"groups":[],"channels":[\#(linked)]}"#)
        try #require(session.mixSession.moreOfMe.notice == .channelPaired)
        let mixes = try transport.sent(ofType: "mix").count

        try await desk.receive(mixState(levels: [50, 100, 50]))

        #expect(try transport.sent(ofType: "mix").count == mixes + 1)
        #expect(ints(try desk.lastMix(), "channels") == [100, 100, 100])
    }

    @Test("ignores text that is not a message")
    func ignoresWhatItCannotRead() async throws {
        try await connected()
        try #require(session.mixSession.mix.levels == [100, 100])
        let before = session.mixSession
        let sent = transport.sent.count

        await desk.receive("not json")

        #expect(session.mixSession == before)
        #expect(transport.sent.count == sent)
        #expect(session.lastError == nil)
        #expect(session.connection == .connected)
    }

    @Test("keeps the desk's latest error")
    func keepsTheLatestError() async throws {
        try await connected()

        await desk.receive(#"{"type":"error","message":"first"}"#)
        await desk.receive(#"{"type":"error","message":"second"}"#)

        #expect(session.lastError == DeskError(message: "second", sequence: nil))
    }

    @Test("says the desk holds the mix only once it acknowledged one")
    func anAckAloneSaysTheDeskHoldsTheMix() async throws {
        try await connected()
        try #require(!session.mixHeldByDesk)

        try await desk.receive(mixState(levels: [80, 70], type: "mix_ack"))

        #expect(session.mixHeldByDesk)
        #expect(session.mixSession.mix.levels == [100, 100])
    }

    @Test("tells its observer after every change")
    func notifiesItsObserver() async throws {
        var changes = 0
        session.stateDidChange = { changes += 1 }

        try session.start(toStage, musicianName: "Ana")
        #expect(changes == 1)
        await desk.open()
        #expect(changes == 2)
        await desk.receive(configMessage(channels: 2))
        #expect(changes == 3)
        session.apply(.masterVolumeChanged(level: 40))
        #expect(changes == 4)
        session.stop()
        #expect(changes == 5)
    }
}

/// Reports `event` to `sink` from a thread of its own, as the transport's queue does, and waits for
/// that thread to finish: false when it does not within 5 s. Synchronous, so the waiting stays out
/// of the test's asynchronous context; it never waits on the main queue, so it cannot deadlock.
private func deliver(_ event: TransportEvent, to sink: @escaping @Sendable (TransportEvent) -> Void)
    -> Bool
{
    let delivered = DispatchSemaphore(value: 0)
    Thread {
        sink(event)
        delivered.signal()
    }.start()
    return delivered.wait(timeout: .now() + 5) == .success
}
