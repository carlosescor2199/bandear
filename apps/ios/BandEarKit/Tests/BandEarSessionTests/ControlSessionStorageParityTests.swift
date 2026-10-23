import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import BandEarSession
import Foundation
import Testing

/// The Android client's tests of what its ViewModel remembers on the phone, from
/// `MonitorViewModelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/ui/state/`),
/// driven through the session with Android's in-memory store (``FakeMusicianStore`` for its
/// `FakeStore(name = "Ana")`).
///
/// Five of them are only the store's half: T5e ported the rest (`OutgoingMixParityTests`) and left
/// `store.channel` to the session, which is what writes it here. Android's saved mixes are T9's,
/// and its listening for desks is the app's discovery model.
@Suite("Control session: Android storage parity")
@MainActor
struct ControlSessionStorageParityTests {
    /// Android's `CLIENT_ID` (`MonitorViewModelTest.kt:1193`).
    private static let clientID = "6f1c2a7e-0d4b-4c55-9a3e-2b8f6c1d9e70"

    /// Android's `announcePair` (`MonitorViewModelTest.kt:445-452`): channels 0 and 1 become one
    /// stereo pair.
    private static let pairConfig = #"""
        {"type":"config","source_channels":3,"groups":[],"channels":[
            {"name":"Keys","kind":"keys","hidden":false,"link":true},
            {"name":"","kind":"keys","hidden":false,"link":false},
            {"name":"Vox","kind":"voice","hidden":false,"link":false}]}
        """#

    private let transport = FakeTransport()
    private let scheduler = ManualScheduler()
    private let store = FakeMusicianStore(musicianName: "Ana")
    private let desk: FakeDesk

    init() {
        desk = FakeDesk(transport)
    }

    /// Android's `viewModel by lazy`: the app launches when a test first needs it, after the test
    /// set what an earlier launch left in the store.
    private func launched() -> ControlSession {
        ControlSession(transport: transport, scheduler: scheduler, store: store)
    }

    /// Android's `connected(channels)`: connects as Ana, opens, and the desk announces `channels`.
    private func connected(channels: Int = 2) async throws -> ControlSession {
        let session = launched()
        try session.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
        return session
    }

    /// The name and the channel only. The saved mixes are T9's, and listening for desks is the
    /// app's discovery model, which starts on its own.
    @Test func startingRestoresTheNameTheChannelAndTheSavedMixesAndListensForServers() {
        store.musicianChannel = 1

        let session = launched()

        #expect(session.musicianName == "Ana")
        #expect(session.mixSession.moreOfMe.musicianChannel == 1)
    }

    @Test func theRegistrationCarriesTheStoredClientIdAndTheName() async throws {
        store.clientID = Self.clientID
        let session = launched()
        try session.start(toStage, musicianName: "Ana")

        await desk.open()

        let registers = try transport.sent(ofType: "register")
        try #require(registers.count == 1)
        #expect(registers[0]["client_id"] as? String == Self.clientID)
        #expect(registers[0]["name"] as? String == "Ana")
        #expect(registers[0]["udp_port"] as? Int == Int(ConnectionForm.defaultAudioPort))
        #expect(try transport.sentTypes() == ["register", "mix"])
    }

    // MonitorViewModelTest.kt:1104. Android's fake store trims what it is given, as its real one
    // does (`MixBankStore.kt:61-63`); here the session hands the store the trimmed name itself.
    @Test func aNameChangeIsStoredAndAnnouncedAfterThePause() async throws {
        let session = try await connected()

        session.setMusicianName("Bea ")

        let socket = try #require(transport.attempts.last)
        #expect(session.musicianName == "Bea ")
        #expect(store.musicianName == "Bea")
        #expect(try socket.sent(ofType: "register").count == 1)
        #expect(scheduler.pendingDelays == [.milliseconds(400)])

        try scheduler.runNextDue()

        #expect(try socket.sent(ofType: "register").last?["name"] as? String == "Bea")
    }

    // MonitorReducerTest.kt:1406, `theNameIsKeptAsTyped`: Android's reducer keeps the name exactly
    // as typed, which is the name the session shows.
    @Test func theNameIsKeptAsTyped() {
        let session = launched()

        session.setMusicianName(" Ana ")

        #expect(session.musicianName == " Ana ")
    }

    @Test func aChannelTheServerDidNotAnnounceIsNotStored() async throws {
        let session = try await connected(channels: 2)

        session.apply(.musicianChannelChosen(5))

        #expect(store.musicianChannel == nil)
        #expect(session.mixSession.moreOfMe.musicianChannel == nil)
    }

    @Test func aConfigWithoutTheRememberedChannelForgetsIt() async throws {
        store.musicianChannel = 3

        let session = try await connected(channels: 2)

        #expect(session.mixSession.moreOfMe.musicianChannel == nil)
        #expect(store.musicianChannel == nil)
    }

    // MARK: The store's half of tests T5e ported

    @Test func moreOfMeLiftsTheMusiciansChannelOnlyInWhatIsSent() async throws {
        let session = try await connected(channels: 2)

        session.apply(.musicianChannelChosen(1))
        session.apply(.moreOfMeToggled(on: true))

        #expect(store.musicianChannel == 1)
    }

    @Test func choosingNoChannelTurnsMoreOfMeOffAndResendsTheMix() async throws {
        let session = try await connected(channels: 2)
        session.apply(.musicianChannelChosen(0))
        session.apply(.moreOfMeToggled(on: true))
        // Not in Android's test: the channel chosen first is remembered, so the nil below is a
        // channel forgotten rather than one never stored.
        try #require(store.musicianChannel == 0)

        session.apply(.musicianChannelChosen(nil))

        #expect(store.musicianChannel == nil)
        #expect(!session.mixSession.moreOfMe.isEnabled)
    }

    @Test func aConfigThatPairsTheMusiciansChannelRemembersThePairAndSendsNothing() async throws {
        let session = try await connected(channels: 3)
        session.apply(.musicianChannelChosen(1))
        let sent = try transport.sent(ofType: "mix").count

        await desk.receive(Self.pairConfig)

        #expect(session.mixSession.moreOfMe.musicianChannel == 0)
        #expect(store.musicianChannel == 0)
        #expect(try transport.sent(ofType: "mix").count == sent)
    }

    @Test func aConfigThatPairsTheMusiciansChannelWithMoreOfMeOnDropsTheLiftOnceTheLinksMixArrives()
        async throws
    {
        let session = try await connected(channels: 3)
        session.apply(.musicianChannelChosen(1))
        session.apply(.moreOfMeToggled(on: true))
        try await desk.answerMixes()

        await desk.receive(Self.pairConfig)

        #expect(session.mixSession.moreOfMe.musicianChannel == 0)
        #expect(store.musicianChannel == 0)
        #expect(!session.mixSession.moreOfMe.isEnabled)
        #expect(session.mixSession.moreOfMe.notice == .channelPaired)
    }

    @Test func aPushDuringMoreOfMeTurnsItOffWithoutSendingAndTheNextMoveSendsThePushedMix()
        async throws
    {
        let session = try await connected(channels: 2)
        session.apply(.musicianChannelChosen(1))
        session.apply(.moreOfMeToggled(on: true))
        try await desk.answerMixes()

        try await desk.receive(mixState(levels: [80, 70]))

        #expect(!session.mixSession.moreOfMe.isEnabled)
        #expect(session.mixSession.moreOfMe.notice == .mixChangedElsewhere)
        #expect(store.musicianChannel == 1)
    }
}
