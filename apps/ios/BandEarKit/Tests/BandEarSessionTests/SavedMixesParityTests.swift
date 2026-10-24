import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// The saved mixes in memory, as Android's `FakeStore` keeps its banks (`MonitorFakes.kt:25-55`): a
/// test sets what an earlier launch left and reads what the saved mixes wrote.
private final class MemoryMixBankStore: MixBankStore {
    var savedMixes: [SavedMix] = []
}

/// One row of the list as Android's `SavedMixUiState` holds it: the name, and whether it is active.
private struct Row: Equatable {
    let name: String
    let active: Bool
}

/// The list as Android's `savedMixes.mixes` shows it.
@MainActor
private func rows(_ mixes: SavedMixes) -> [Row] {
    mixes.mixes.map { Row(name: $0.name, active: $0.name == mixes.activeName) }
}

/// A default mix saved as `name`.
private func saved(_ name: String, _ snapshot: MixSnapshot = MixSnapshot()) -> SavedMix {
    SavedMix(name: name, snapshot: snapshot)
}

/// Android's `followingState(name, level)` (`MonitorViewModelTest.kt:610-611`): a push of every
/// channel at `level`, the mix following `name`'s.
private func followingState(_ name: String?, level: Int = 100) -> [String: Any] {
    var object = mixState(levels: [Int](repeating: level, count: 32))
    object["following"] = name ?? NSNull()
    return object
}

/// The Android client's tests of its saved mixes as its view model runs them, from
/// `MonitorViewModelTest.kt`, and the saved-mix cases of `MonitorReducerTest.kt` that word a notice
/// (`apps/android/app/src/test/java/com/bandear/android/ui/state/`): what a save, a recall, a rename
/// and a delete do to the store, the list, the active mix, the mix, the player and the desk.
///
/// Driven as Android drives them: the real session over a ``FakeTransport``, its audio coordinator
/// with a ``FakeReceiver``, and the saved mixes in memory. Android builds its view model when a test
/// first touches it, which reads the store; ``launched()`` is that moment.
/// - Where Android checks a notice's words, these check the notice; its words are the screen's, and
///   `SavedMixesWordsParityTests` checks them against the same strings.
/// - Where Android checks `emptyMessage`, these check that nothing is listed, which is when the
///   screen shows it.
/// - Android's reducer cases build the list with `MixesLoaded(names)`; here that is what the store
///   holds when the saved mixes are made. Their halves about the bank alone are in
///   `MixBankParityTests`, and about the mix alone in `MixReducerParityTests`.
@Suite("Saved mixes: Android parity")
@MainActor
struct SavedMixesParityTests {
    private let transport = FakeTransport()
    private let receivers = FakeReceivers()
    private let store = MemoryMixBankStore()
    private let session: ControlSession
    private let desk: FakeDesk
    private let audio: MonitorAudio

    init() {
        // Android's `FakeStore(name = "Ana")`.
        session = ControlSession(
            transport: transport, scheduler: ManualScheduler(),
            store: FakeMusicianStore(musicianName: "Ana"))
        desk = FakeDesk(transport)
        audio = MonitorAudio(
            session: session, sinkFactory: FakeSinkFactory(), makeReceiver: receivers.make)
    }

    /// Android's view model, built now: the saved mixes as the store holds them, recalled into the
    /// session.
    private func launched() -> SavedMixes {
        SavedMixes(session: session, store: store)
    }

    /// Android's `connected(channels)`: the session starts for Ana, the socket opens, and the desk
    /// announces `channels` channels.
    private func connected(channels: Int = 2) async throws {
        try audio.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
    }

    // MARK: MonitorViewModelTest

    // MonitorViewModelTest.kt:1027-1045
    @Test func savingAndRecallingAMix() async throws {
        let mixes = launched()
        try await connected(channels: 2)
        session.apply(.channelLevelChanged(channel: 0, level: 30))

        #expect(mixes.save(as: " Verse "))

        #expect(store.savedMixes.map(\.name) == ["Verse"])
        #expect(rows(mixes) == [Row(name: "Verse", active: true)])

        session.apply(.channelLevelChanged(channel: 0, level: 90))
        session.apply(.masterVolumeChanged(level: 50))
        mixes.recall("Verse")

        #expect(session.mixSession.mix.levels == [30, 100])
        #expect(ints(try desk.lastMix(), "channels") == [30, 100])
        #expect(try receivers.only().controls.snapshot.volumePercent == 100)
        #expect(mixes.notice == .recalled(name: "Verse"))
    }

    // MonitorViewModelTest.kt:1047-1059
    @Test func savingUnderAnExistingNameReplacesThatMix() async throws {
        let mixes = launched()
        try await connected(channels: 2)
        session.apply(.channelLevelChanged(channel: 0, level: 30))
        mixes.save(as: "Verse")
        session.apply(.channelLevelChanged(channel: 0, level: 70))

        // As on the old screen, saving under a name already used updates that mix; it is not refused.
        #expect(mixes.save(as: "Verse"))

        #expect(store.savedMixes.map(\.name) == ["Verse"])
        try #require(store.savedMixes.count == 1)
        #expect(store.savedMixes[0].snapshot.channelGains == [70, 100])
    }

    // MonitorViewModelTest.kt:1061-1068
    @Test func savingNeedsAName() {
        let mixes = launched()

        #expect(!mixes.save(as: "   "))

        #expect(store.savedMixes.isEmpty)
        #expect(mixes.notice == .bank(.nameNeeded))
        #expect(mixes.mixes.isEmpty)
    }

    // MonitorViewModelTest.kt:1070-1080
    @Test func recallingAMixThatIsGoneRefreshesTheList() {
        store.savedMixes.append(saved("Verse"))
        let mixes = launched()
        store.savedMixes.removeAll()

        mixes.recall("Verse")

        #expect(mixes.notice == .bank(.missing(name: "Verse")))
        #expect(mixes.mixes.isEmpty)
    }

    // MonitorViewModelTest.kt:1082-1101
    @Test func renamingAndDeletingAMix() throws {
        store.savedMixes.append(saved("Verse", MixSnapshot(volumePercent: 70)))
        store.savedMixes.append(saved("Chorus"))
        let mixes = launched()

        mixes.rename("Verse", to: "Chorus")

        #expect(mixes.notice == .bank(.nameTaken(name: "Chorus")))
        #expect(store.savedMixes.map(\.name) == ["Verse", "Chorus"])

        mixes.rename("Verse", to: " Bridge ")

        #expect(mixes.mixes.map(\.name) == ["Chorus", "Bridge"])
        let bridge = store.savedMixes.filter { $0.name == "Bridge" }
        try #require(bridge.count == 1)
        #expect(bridge[0].snapshot.volumePercent == 70)

        mixes.delete("Chorus")

        #expect(store.savedMixes.map(\.name) == ["Bridge"])
        #expect(mixes.mixes.map(\.name) == ["Bridge"])
    }

    // MonitorViewModelTest.kt:657-670
    @Test func recallingWhileFollowingSendsNothingAndSaysWhy() async throws {
        store.savedMixes.append(saved("Verse", MixSnapshot(channelGains: [10, 20, 30])))
        let mixes = launched()
        try await connected(channels: 3)
        try await desk.answerMixes()
        try await desk.receive(followingState("Ana", level: 70))
        let sent = try transport.sent(ofType: "mix").count

        mixes.recall("Verse")

        #expect(try transport.sent(ofType: "mix").count == sent)
        #expect(mixes.notice == .notRecalled(name: "Verse", because: .following(leader: "Ana")))
        #expect(session.mixSession.mix.levels == [70, 70, 70])
    }

    /// The saved mixes' half; the name and the channel are in `ControlSessionStorageParityTests`, and
    /// listening for desks is the app's discovery model.
    // MonitorViewModelTest.kt:117-127
    @Test func startingRestoresTheNameTheChannelAndTheSavedMixesAndListensForServers() {
        store.savedMixes.append(saved("Verse"))

        let mixes = launched()

        #expect(mixes.mixes.map(\.name) == ["Verse"])
        #expect(!mixes.mixes.isEmpty)
    }

    // MonitorViewModelTest.kt:867-881
    @Test func aPushedMixIsTheCurrentMixAndLeavesTheSavedMixesAlone() async throws {
        let mixes = launched()
        try await connected(channels: 2)
        session.apply(.channelLevelChanged(channel: 0, level: 30))
        mixes.save(as: "Verse")
        try await desk.answerMixes()

        try await desk.receive(mixState(levels: [80, 70]))

        try #require(store.savedMixes.count == 1)
        #expect(store.savedMixes[0].snapshot.channelGains == [30, 100])

        mixes.save(as: "Chorus")

        let chorus = store.savedMixes.filter { $0.name == "Chorus" }
        try #require(chorus.count == 1)
        #expect(chorus[0].snapshot.channelGains == [80, 70])
    }

    // MARK: MonitorReducerTest, the saved mixes' notices

    /// Android reduces the recall without numbering it, so the push that follows is shown; here the
    /// desk answers the recall first, which leaves the push to show too.
    // MonitorReducerTest.kt:816-824
    @Test func aPushedMixLeavesTheSavedMixesAsTheyWere() async throws {
        store.savedMixes.append(saved("Verse", MixSnapshot(channelGains: [30, 30])))
        let mixes = launched()
        try await connected(channels: 2)
        mixes.recall("Verse")
        try await desk.answerMixes()
        let recalled = (rows(mixes), mixes.notice)

        try await desk.receive(mixState(levels: [80, 70]))

        #expect(session.mixSession.mix.levels == [80, 70])
        #expect(rows(mixes) == recalled.0)
        #expect(mixes.notice == recalled.1)
    }

    /// Android's `lockedBy(following("Ana"))` is the desk's push saying so.
    // MonitorReducerTest.kt:1135-1147
    @Test func recallingASavedMixWhileFollowingIsRefusedAndSaysWhy() async throws {
        store.savedMixes.append(
            saved("Verse", MixSnapshot(channelGains: [10, 20, 30], volumePercent: 40)))
        let mixes = launched()
        try await connected(channels: 3)
        try await desk.answerMixes()
        try await desk.receive(followingState("Ana", level: 70))
        let before = session.mixSession.mix

        mixes.recall("Verse")

        #expect(session.mixSession.mix == before)
        #expect(mixes.activeName == nil)
        #expect(mixes.notice == .notRecalled(name: "Verse", because: .following(leader: "Ana")))
        // A leader with no name is still named in words, by the screen.
        try await desk.receive(followingState("", level: 70))
        mixes.recall("Verse")
        #expect(mixes.notice == .notRecalled(name: "Verse", because: .following(leader: "")))
    }

    // MonitorReducerTest.kt:1234-1240
    @Test func deletingTheLastMixShowsTheEmptyState() {
        store.savedMixes.append(saved("Verse"))
        let mixes = launched()

        mixes.delete("Verse")

        #expect(mixes.mixes.isEmpty)
        #expect(mixes.notice == .bank(.deleted(name: "Verse")))
    }

    // MonitorReducerTest.kt:1242-1248
    @Test func aSavedMixBecomesTheActiveOne() {
        store.savedMixes.append(saved("Chorus"))
        let mixes = launched()

        mixes.save(as: "Verse")

        #expect(rows(mixes) == [Row(name: "Chorus", active: false), Row(name: "Verse", active: true)])
        #expect(mixes.notice == .bank(.saved(name: "Verse")))
    }

    /// The saved mixes' half: the recalled mix is the active one, and the notice says so. The values
    /// applied are `MixReducerParityTests`'.
    // MonitorReducerTest.kt:1250-1280
    @Test func recallingAMixAppliesItsValuesAndKeepsWhatTheServerAnnounced() async throws {
        store.savedMixes.append(saved("Verse", MixSnapshot(channelGains: [30, 60], volumePercent: 70)))
        let mixes = launched()
        try await connected(channels: 2)

        mixes.recall("Verse")

        #expect(session.mixSession.mix.levels == [30, 60])
        #expect(session.mixSession.mix.masterVolume == 70)
        #expect(rows(mixes) == [Row(name: "Verse", active: true)])
        #expect(mixes.notice == .recalled(name: "Verse"))
    }

    // MonitorReducerTest.kt:1320-1330
    @Test func renamingTheActiveMixKeepsItActive() {
        store.savedMixes.append(saved("Verse"))
        store.savedMixes.append(saved("Chorus"))
        let mixes = launched()
        mixes.save(as: "Verse")

        mixes.rename("Verse", to: "Bridge")

        #expect(rows(mixes) == [Row(name: "Chorus", active: false), Row(name: "Bridge", active: true)])
        #expect(mixes.notice == .bank(.renamed(from: "Verse", to: "Bridge")))
    }

    // MonitorReducerTest.kt:1332-1343
    @Test func aMissingMixIsReportedAndTheListRefreshed() {
        store.savedMixes.append(saved("Verse"))
        let mixes = launched()
        mixes.recall("Verse")
        store.savedMixes.removeAll()

        mixes.recall("Verse")

        #expect(mixes.notice == .bank(.missing(name: "Verse")))
        #expect(mixes.activeName == nil)
        #expect(mixes.mixes.isEmpty)
    }

    /// Android dispatches the refused save's notice by hand; here the save is refused.
    // MonitorReducerTest.kt:1352-1358
    @Test func aRefusedSaveSaysWhy() {
        store.savedMixes.append(saved("Verse"))
        let mixes = launched()

        mixes.save(as: "   ")

        #expect(mixes.notice == .bank(.nameNeeded))
        #expect(mixes.mixes.map(\.name) == ["Verse"])
    }
}
