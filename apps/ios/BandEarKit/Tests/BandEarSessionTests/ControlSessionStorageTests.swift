import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import BandEarSession
import Foundation
import Testing

/// What the storage parity suite reaches only indirectly: the name a session starts with, the
/// client id across launches, and the remembered channel before and after the desk's `config`.
@Suite("Control session: what the phone remembers")
@MainActor
struct ControlSessionStorageTests {
    private let store = FakeMusicianStore(musicianName: "Ana")

    /// One launch of the app over ``store``, with its own transport.
    private func launch() -> (ControlSession, FakeTransport) {
        let transport = FakeTransport()
        let session = ControlSession(
            transport: transport, scheduler: ManualScheduler(), store: store)
        return (session, transport)
    }

    @Test("a session started with a name keeps it as typed and remembers it trimmed")
    func remembersTheName() throws {
        let (session, _) = launch()

        try session.start(toStage, musicianName: " Bea ")

        #expect(session.musicianName == " Bea ")
        #expect(store.musicianName == "Bea")
    }

    @Test("a session started with a blank name keeps it as typed and remembers it empty")
    func aBlankNameIsRememberedEmpty() throws {
        let (session, _) = launch()

        try session.start(toStage, musicianName: "  ")

        #expect(session.musicianName == "  ")
        #expect(store.musicianName == "")
    }

    @Test("the next launch shows the name the last one remembered")
    func theNameSurvivesTheLaunch() {
        let (first, _) = launch()
        first.setMusicianName(" Bea ")

        let (next, _) = launch()

        #expect(next.musicianName == "Bea")
    }

    @Test("the first launch makes an id and registers with it, and the next launch sends the same")
    func theClientIDSurvivesTheLaunch() async throws {
        let (first, firstTransport) = launch()
        try first.start(toStage, musicianName: "Ana")
        await FakeDesk(firstTransport).open()
        let (next, nextTransport) = launch()
        try next.start(toStage, musicianName: "Ana")
        await FakeDesk(nextTransport).open()

        let firstRegisters = try firstTransport.sent(ofType: "register")
        let nextRegisters = try nextTransport.sent(ofType: "register")
        try #require(firstRegisters.count == 1 && nextRegisters.count == 1)
        let id = try #require(firstRegisters[0]["client_id"] as? String)
        #expect(ClientIdentity.validClientID(id) == id)
        #expect(store.clientID == id)
        #expect(nextRegisters[0]["client_id"] as? String == id)
    }

    @Test("the remembered channel is on the session before the first config, which keeps it")
    func theRememberedChannelMeetsTheFirstConfig() async throws {
        store.musicianChannel = 1
        let (session, transport) = launch()
        let desk = FakeDesk(transport)
        try session.start(toStage, musicianName: "Ana")
        await desk.open()

        #expect(session.mixSession.moreOfMe.musicianChannel == 1)

        await desk.receive(configMessage(channels: 2))

        #expect(session.mixSession.moreOfMe.musicianChannel == 1)
        #expect(store.musicianChannel == 1)
    }

    @Test("a channel the desk did not announce leaves the remembered one alone")
    func anUnannouncedChoiceKeepsTheRememberedChannel() async throws {
        store.musicianChannel = 1
        let (session, transport) = launch()
        let desk = FakeDesk(transport)
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: 2))

        session.apply(.musicianChannelChosen(5))

        #expect(session.mixSession.moreOfMe.musicianChannel == 1)
        #expect(store.musicianChannel == 1)
    }
}
