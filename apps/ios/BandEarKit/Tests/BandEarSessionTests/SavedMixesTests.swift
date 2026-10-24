import BandEarDiscovery
import BandEarMix
import BandEarPlayback
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// The saved mixes in memory, counting how often the list was written.
private final class CountingMixBankStore: MixBankStore {
    private(set) var writes = 0
    var savedMixes: [SavedMix] = [] {
        didSet { writes += 1 }
    }

    /// Holding `mixes`, as an earlier launch left them; setting them here is no write.
    init(_ mixes: [SavedMix] = []) {
        savedMixes = mixes
    }
}

/// A default mix saved as `name`.
private func saved(_ name: String, _ snapshot: MixSnapshot = MixSnapshot()) -> SavedMix {
    SavedMix(name: name, snapshot: snapshot)
}

/// The desk's push of a mix that follows `leader`'s, every channel at 70.
private func following(_ leader: String) -> [String: Any] {
    var object = mixState(levels: [Int](repeating: 70, count: 32))
    object["following"] = leader
    return object
}

/// A throwaway `UserDefaults` suite for `body`, whose every call of `launch` is one launch of the
/// app: a fresh `UserDefaults` over the same suite, as a new process opens it.
///
/// The suite is named by a path in a new temporary directory, which keeps its file there: a suite
/// named like an app keeps an empty file in `~/Library/Preferences` even after its domain is
/// removed. The domain and the directory are removed afterwards, so nothing outlives the test.
private func withThrowawaySuite(
    _ body: (_ launch: () throws -> UserDefaults) throws -> Void
) throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("bandear-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let suite = directory.appendingPathComponent("saved-mixes").path
    defer {
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: directory)
    }
    try body { try #require(UserDefaults(suiteName: suite)) }
}

/// What the iOS saved mixes add to Android's rules, which `SavedMixesParityTests` ports: when the
/// store is written, who hears a change, what a recall sends, and what survives a relaunch.
@Suite("Saved mixes")
@MainActor
struct SavedMixesTests {
    private let transport = FakeTransport()
    private let receivers = FakeReceivers()
    private let session: ControlSession
    private let desk: FakeDesk
    private let audio: MonitorAudio

    init() {
        session = ControlSession(
            transport: transport, scheduler: ManualScheduler(), store: FakeMusicianStore())
        desk = FakeDesk(transport)
        audio = MonitorAudio(
            session: session, sinkFactory: FakeSinkFactory(), makeReceiver: receivers.make)
    }

    /// The session starts, the socket opens, and the desk announces `channels` channels.
    private func connected(channels: Int = 2) async throws {
        try audio.start(toStage, musicianName: "Ana")
        await desk.open()
        await desk.receive(configMessage(channels: channels))
    }

    @Test("a new launch lists what the store holds, none active and nothing said")
    func aNewLaunchListsTheStoredMixes() {
        let store = CountingMixBankStore([saved("Verse"), saved("Chorus")])

        let mixes = SavedMixes(session: session, store: store)

        #expect(mixes.mixes == [saved("Verse"), saved("Chorus")])
        #expect(mixes.activeName == nil)
        #expect(mixes.notice == nil)
        #expect(store.writes == 0)
    }

    @Test("the saved mixes survive a relaunch, with none active")
    func theSavedMixesSurviveARelaunch() throws {
        try withThrowawaySuite { launch in
            let first = SavedMixes(session: session, store: UserDefaultsMixBankStore(try launch()))
            session.apply(.masterVolumeChanged(level: 40))
            first.save(as: "Verse")
            first.save(as: "Chorus")
            first.delete("Chorus")

            let next = SavedMixes(session: session, store: UserDefaultsMixBankStore(try launch()))

            #expect(next.mixes == [saved("Verse", session.mixSession.mix.snapshot)])
            #expect(next.mixes.first?.snapshot.volumePercent == 40)
            #expect(next.activeName == nil)
            #expect(next.notice == nil)
        }
    }

    @Test("the store is written once per change of the list, and never by a refusal")
    func theStoreIsWrittenOnlyWhenTheListChanges() {
        let store = CountingMixBankStore([saved("Verse"), saved("Chorus")])
        let mixes = SavedMixes(session: session, store: store)

        mixes.save(as: "  ")
        mixes.rename("Verse", to: " ")
        mixes.rename("Gone", to: "Bridge")
        mixes.rename("Verse", to: "Chorus")
        mixes.rename("Verse", to: " Verse ")
        mixes.recall("Verse")
        mixes.recall("Gone")
        #expect(store.writes == 0)
        #expect(store.savedMixes.map(\.name) == ["Verse", "Chorus"])

        mixes.save(as: "Bridge")
        #expect(store.writes == 1)
        mixes.rename("Bridge", to: "Outro")
        #expect(store.writes == 2)
        mixes.delete("Verse")
        #expect(store.writes == 3)
        #expect(store.savedMixes.map(\.name) == ["Chorus", "Outro"])
        #expect(mixes.mixes == store.savedMixes)
    }

    @Test("a rename to the name the mix already has says nothing new")
    func anUnchangedRenameKeepsTheLastNotice() {
        let mixes = SavedMixes(session: session, store: CountingMixBankStore())
        mixes.save(as: "Verse")
        let number = mixes.noticeNumber

        mixes.rename("Verse", to: " Verse ")

        #expect(mixes.notice == .bank(.saved(name: "Verse")))
        #expect(mixes.noticeNumber == number)
        #expect(mixes.activeName == "Verse")
    }

    @Test("an action that says what the last one said is still a notice of its own")
    func aRepeatedOutcomeIsANewNotice() {
        let mixes = SavedMixes(
            session: session, store: CountingMixBankStore([saved("Verse"), saved("Chorus")]))
        let actions: [(label: String, run: () -> Void)] = [
            ("a blank save", { mixes.save(as: " ") }),
            ("a save under a name already saved", { mixes.save(as: "Verse") }),
            ("a recall", { mixes.recall("Verse") }),
            ("a recall of a mix no longer saved", { mixes.recall("Gone") }),
            ("a rename to a name already taken", { mixes.rename("Verse", to: "Chorus") }),
            ("a blank rename", { mixes.rename("Verse", to: " ") }),
            ("a delete of a mix no longer saved", { mixes.delete("Gone") }),
        ]

        for action in actions {
            action.run()
            let first = (notice: mixes.notice, number: mixes.noticeNumber)

            action.run()

            #expect(mixes.notice == first.notice, "\(action.label)")
            #expect(mixes.noticeNumber != first.number, "\(action.label)")
        }
    }

    @Test("a second refused recall is still a notice of its own")
    func aRepeatedRefusedRecallIsANewNotice() async throws {
        let mixes = SavedMixes(session: session, store: CountingMixBankStore([saved("Verse")]))
        try await connected(channels: 2)
        try await desk.answerMixes()
        try await desk.receive(following("Ana"))
        mixes.recall("Verse")
        try #require(mixes.notice == .notRecalled(name: "Verse", because: .following(leader: "Ana")))
        let first = mixes.noticeNumber

        mixes.recall("Verse")

        #expect(mixes.notice == .notRecalled(name: "Verse", because: .following(leader: "Ana")))
        #expect(mixes.noticeNumber != first)
    }

    @Test("observers hear every action, refused or not")
    func observersHearEveryAction() {
        let mixes = SavedMixes(session: session, store: CountingMixBankStore([saved("Verse")]))
        var heard = 0
        mixes.stateDidChange = { heard += 1 }

        mixes.save(as: " ")
        #expect(heard == 1)
        mixes.save(as: "Chorus")
        #expect(heard == 2)
        mixes.recall("Verse")
        #expect(heard == 3)
        mixes.recall("Gone")
        #expect(heard == 4)
        mixes.rename("Verse", to: "Bridge")
        #expect(heard == 5)
        mixes.delete("Bridge")
        #expect(heard == 6)
    }

    @Test("a recall reaches the screens through the audio coordinator, as an edit does")
    func aRecallIsReportedLikeAnEdit() async throws {
        let mixes = SavedMixes(
            session: session,
            store: CountingMixBankStore([saved("Verse", MixSnapshot(channelGains: [30, 60]))]))
        try await connected(channels: 2)
        var reported = 0
        audio.stateDidChange = { reported += 1 }

        mixes.recall("Verse")

        #expect(reported >= 1)
        #expect(session.mixSession.mix.levels == [30, 60])
    }

    @Test("a recall is sent once, as the newest mix, carrying the saved one")
    func aRecallIsSentOnceAsTheNewestMix() async throws {
        let snapshot = MixSnapshot(
            channelGains: [30, 60], channelPans: [20, 80], channelMutes: [true, false],
            channelSolos: [false, true], volumePercent: 70, maxLevelPercent: 90, muted: true)
        let mixes = SavedMixes(
            session: session, store: CountingMixBankStore([saved("Verse", snapshot)]))
        try await connected(channels: 2)
        try await desk.answerMixes()
        let sent = try transport.sent(ofType: "mix").count

        mixes.recall("Verse")

        let after = try transport.sent(ofType: "mix")
        try #require(after.count == sent + 1)
        let recalled = after[sent]
        #expect(recalled["seq"] as? Int == Int(session.mixSession.sync.latest))
        #expect(ints(recalled, "channels") == [30, 60])
        #expect(ints(recalled, "pans") == [20, 80])
        #expect(recalled["mutes"] as? [Bool] == [true, false])
        #expect(recalled["solos"] as? [Bool] == [false, true])
        #expect(recalled["volume_percent"] as? Int == 70)
        #expect(recalled["max_level_percent"] as? Int == 90)
        #expect(recalled["muted"] as? Bool == true)
        #expect(mixes.activeName == "Verse")
    }

    @Test("a save while More of me lifts keeps the musician's own levels")
    func aSaveWhileMoreOfMeLiftsKeepsTheOwnLevels() async throws {
        let store = CountingMixBankStore()
        let mixes = SavedMixes(session: session, store: store)
        try await connected(channels: 2)
        session.apply(.channelLevelChanged(channel: 0, level: 30))
        session.apply(.musicianChannelChosen(0))
        session.apply(.moreOfMeToggled(on: true))
        try #require(session.mixSession.moreOfMe.isEnabled)
        try #require(ints(try desk.lastMix(), "channels") != [30, 100])

        mixes.save(as: "Verse")

        try #require(store.savedMixes.count == 1)
        #expect(store.savedMixes[0].snapshot.channelGains == [30, 100])
    }

    @Test("a recall of the active mix after a rename finds it by its new name")
    func aRenamedMixIsRecalledByItsNewName() async throws {
        let mixes = SavedMixes(
            session: session,
            store: CountingMixBankStore([saved("Verse", MixSnapshot(channelGains: [30, 60]))]))
        try await connected(channels: 2)
        mixes.rename("Verse", to: "Bridge")

        mixes.recall("Verse")
        #expect(mixes.notice == .bank(.missing(name: "Verse")))
        #expect(session.mixSession.mix.levels == [100, 100])

        mixes.recall("Bridge")
        #expect(mixes.notice == .recalled(name: "Bridge"))
        #expect(mixes.activeName == "Bridge")
        #expect(session.mixSession.mix.levels == [30, 60])
    }
}
