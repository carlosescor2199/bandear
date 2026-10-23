import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// What the name parity tests reach only indirectly: the pause typing waits for, when a renamed
/// session announces nothing, the length a name is cut to, and the name a start takes.
///
/// Android's rules: `setName` keeps the name as typed, the store keeps it trimmed
/// (`MonitorViewModel.kt:488-492`, `MixBankStore.kt:61-63`), and the open control channel
/// re-announces it 400 ms after the last change (`ControlChannel.kt:15,342-348`). The name fields
/// stop at 64 UTF-16 units, never splitting a character made of two (`BandEarTextField.kt:67-85`).
@Suite("Control session: the musician's name")
@MainActor
struct ControlSessionNameTests {
    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let store = FakeMusicianStore(clientID: "phone-1", musicianName: "Ana")
    private let session: ControlSession
    private let desk: FakeDesk

    init() {
        session = ControlSession(transport: transport, scheduler: scheduler, store: store)
        desk = FakeDesk(transport)
    }

    /// The session started for Ana, its socket open and the desk's `config` received.
    private func connected() async throws {
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: 2))
    }

    /// The `name` of every registration sent, on any connection, in order.
    private func registeredNames() throws -> [String?] {
        try transport.sent(ofType: "register").map { $0["name"] as? String }
    }

    // MARK: The pause

    @Test("typing again within the pause announces only the last name, once")
    func onlyTheLastNameOfAPauseIsAnnounced() async throws {
        try await connected()

        session.setMusicianName("B")
        session.setMusicianName("Be")
        session.setMusicianName("Bea")

        #expect(scheduler.pendingDelays == [.milliseconds(400)])
        #expect(try registeredNames() == ["Ana"])

        try scheduler.runNextDue()

        #expect(try registeredNames() == ["Ana", "Bea"])
        #expect(scheduler.pending.isEmpty)
    }

    @Test("the announcement is the whole registration, under the same id and port")
    func theAnnouncementIsTheWholeRegistration() async throws {
        try await connected()

        session.setMusicianName(" Bea ")
        try scheduler.runNextDue()

        let registers = try transport.sent(ofType: "register")
        try #require(registers.count == 2)
        #expect(
            NSDictionary(dictionary: registers[1])
                == NSDictionary(dictionary: [
                    "type": "register", "name": "Bea", "udp_port": 50_000, "client_id": "phone-1",
                    "capabilities": [
                        "stats.v2", "playback.buffer", "redundancy.recovered", "redundancy.hold",
                    ],
                ]))
    }

    @Test("a renamed session announces the name only: no reconnect and no mix")
    func aRenameSendsOnlyTheRegistration() async throws {
        try await connected()
        let sent = transport.sent.count

        session.setMusicianName("Bea")
        try scheduler.runNextDue()

        #expect(try transport.sentTypes().dropFirst(sent) == ["register"])
        #expect(transport.opened.count == 1)
        #expect(transport.closes == 0)
        #expect(session.connection == .connected)
    }

    // MARK: When nothing is announced

    @Test("a name typed while no session runs is stored and announces nothing")
    func aNameTypedWhileIdleAnnouncesNothing() {
        session.setMusicianName("Bea")

        #expect(session.musicianName == "Bea")
        #expect(store.musicianName == "Bea")
        #expect(scheduler.scheduled.isEmpty)
        #expect(transport.sent.isEmpty)
    }

    /// Android's `updateRegistrationName` returns at once while no session runs.
    @Test("a name typed after the session stopped is stored and waits for no pause")
    func aNameTypedAfterAStopAnnouncesNothing() async throws {
        try await connected()
        session.stop()

        session.setMusicianName("Bea")

        #expect(store.musicianName == "Bea")
        #expect(scheduler.pending.isEmpty)
        #expect(try registeredNames() == ["Ana"])
    }

    @Test("a pause that ends while the connection is down announces nothing; the next open does")
    func aPauseEndingWhileDisconnectedAnnouncesNothing() async throws {
        try await connected()
        await desk.fails("wifi gone")
        try #require(session.connection == .reconnecting)

        session.setMusicianName("Bea")
        try #require(scheduler.pendingDelays.contains(.milliseconds(400)))
        // The pause is shorter than the first retry's wait, so it ends first.
        try scheduler.runNextDue()

        #expect(try registeredNames() == ["Ana"])

        try scheduler.runNextDue()
        await desk.open()

        #expect(try registeredNames() == ["Ana", "Bea"])
        #expect(try #require(transport.attempts.last).sentTypes() == ["register", "mix"])
    }

    @Test("a name typed while connecting is the one the open registers")
    func theOpenRegistersTheNameTypedWhileConnecting() async throws {
        try session.start(toStage, musicianName: "Ana")

        session.setMusicianName("Bea")
        await desk.open()

        #expect(try registeredNames() == ["Bea"])
    }

    @Test("stop calls off an announcement still waiting for its pause")
    func stopCallsOffTheAnnouncement() async throws {
        try await connected()
        session.setMusicianName("Bea")

        session.stop()

        #expect(scheduler.pending.isEmpty)
        #expect(try registeredNames() == ["Ana"])
    }

    @Test("a new start calls off the last session's announcement and registers with its own name")
    func aNewStartCallsOffTheAnnouncement() async throws {
        try await connected()
        session.setMusicianName("Bea")

        try session.start(toStage, musicianName: "Cleo")
        await desk.open()

        #expect(scheduler.pending.isEmpty)
        #expect(try registeredNames() == ["Ana", "Cleo"])
    }

    // MARK: The name as typed

    @Test("a rename tells the observer")
    func aRenameTellsTheObserver() {
        var changes = 0
        session.stateDidChange = { changes += 1 }

        session.setMusicianName("Bea")

        #expect(changes == 1)
    }

    /// `BandEarTextFieldTest.aNameStopsAtTheLengthTheServerKeeps`, for a whole name.
    @Test("a name is cut at the 64 characters the desk keeps, as typed, stored and announced")
    func aNameStopsAtTheLengthTheServerKeeps() async throws {
        try await connected()

        session.setMusicianName(String(repeating: "a", count: 80))
        try scheduler.runNextDue()

        #expect(session.musicianName == String(repeating: "a", count: 64))
        #expect(store.musicianName == String(repeating: "a", count: 64))
        #expect(try registeredNames().last == String(repeating: "a", count: 64))
    }

    /// `BandEarTextFieldTest.theCutNeverSplitsATwoUnitCharacter`, at the session's limit: Android
    /// counts a name in UTF-16 units, and the cut is one unit shorter rather than split an emoji.
    @Test("the cut counts UTF-16 units and never splits a two-unit character")
    func theCutNeverSplitsATwoUnitCharacter() {
        let fits = String(repeating: "a", count: 62) + "🎸!"
        let splits = String(repeating: "a", count: 63) + "🎸!"

        session.setMusicianName(fits)
        #expect(session.musicianName == String(repeating: "a", count: 62) + "🎸")

        session.setMusicianName(splits)
        #expect(session.musicianName == String(repeating: "a", count: 63))
    }

    @Test("a name is cut before it is trimmed, as the field cuts what is typed")
    func theCutComesBeforeTheTrim() {
        session.setMusicianName("   " + String(repeating: "a", count: 70))

        #expect(session.musicianName == "   " + String(repeating: "a", count: 61))
        #expect(store.musicianName == String(repeating: "a", count: 61))
    }

    @Test("a start takes its name as typed and cut, as a rename does")
    func aStartCutsTheName() async throws {
        try session.start(toStage, musicianName: String(repeating: "b", count: 70))
        await desk.open()

        #expect(session.musicianName == String(repeating: "b", count: 64))
        #expect(store.musicianName == String(repeating: "b", count: 64))
        #expect(try registeredNames() == [String(repeating: "b", count: 64)])
    }
}
