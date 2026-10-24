import BandEarMix
import BandEarSession
import Foundation
import Testing

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
    let suite = directory.appendingPathComponent("mixes").path
    defer {
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: directory)
    }
    try body { try #require(UserDefaults(suiteName: suite)) }
}

/// Where the adapter keeps the saved mixes: Android's `banks` key, under this app's prefix.
private let key = "bandear.banks"

/// The JSON object `text` holds, or nil when it holds none.
private func object(_ text: String) -> Any? {
    try? JSONSerialization.jsonObject(with: Data(text.utf8))
}

/// The `UserDefaults` adapter, on a throwaway suite: what one launch saves, the next one reads, in
/// the JSON Android writes, and a damaged entry costs only itself.
@Suite("Mix bank store: UserDefaults")
struct UserDefaultsMixBankStoreTests {

    @Test("holds no saved mix on the first launch, nor when what it holds is no list")
    func emptyStore() throws {
        try withThrowawaySuite { launch in
            let defaults = try launch()
            let store = UserDefaultsMixBankStore(defaults)

            #expect(store.savedMixes == [])

            let unreadable: [Any] = ["", "   ", #"{"name":"Verse"}"#, 42, Data([1, 2]), ["Verse"]]
            for value in unreadable {
                defaults.set(value, forKey: key)
                #expect(store.savedMixes == [], "\(value) is no list of mixes")
            }

            store.savedMixes = []
            #expect(UserDefaultsMixBankStore(try launch()).savedMixes == [])
        }
    }

    @Test("what one launch saves, the next one reads, written as Android writes it")
    func roundTrip() throws {
        let mixes = [
            SavedMix(
                name: "Verse",
                snapshot: MixSnapshot(
                    channelGains: [30, 60], channelPans: [0, 100], channelMutes: [true, false],
                    channelSolos: [false, true], groupLevels: [40], volumePercent: 70,
                    maxLevelPercent: 90, muted: true)),
            SavedMix(name: "Show \"A\" / ñ", snapshot: MixSnapshot()),
        ]
        try withThrowawaySuite { launch in
            UserDefaultsMixBankStore(try launch()).savedMixes = mixes

            let next = try launch()

            #expect(UserDefaultsMixBankStore(next).savedMixes == mixes)
            // Android's shape (`MixBankStore.kt:74-88`, `MixSnapshot.kt:26-35`): a string holding a
            // list of `name` and `snapshot` objects, each snapshot a string of its own JSON object.
            let stored = try #require(next.string(forKey: key))
            let list = try #require(object(stored) as? [[String: Any]])
            try #require(list.count == 2)
            #expect(Set(list[0].keys) == ["name", "snapshot"])
            #expect(list[0]["name"] as? String == "Verse")
            let snapshot = try #require(list[0]["snapshot"] as? String)
            let fields = try #require(object(snapshot) as? [String: Any])
            #expect(
                Set(fields.keys) == [
                    "channelGains", "channelPans", "channelMutes", "channelSolos", "groupLevels",
                    "volumePercent", "maxLevelPercent", "muted",
                ])
            #expect(fields["channelGains"] as? [Int] == [30, 60])
            #expect(fields["channelMutes"] as? [Bool] == [true, false])
            #expect(fields["volumePercent"] as? Int == 70)
            #expect(fields["muted"] as? Bool == true)
        }
    }

    /// Android's `serializeBanks` output for one mix, as its `org.json` writes it.
    @Test("reads the saved mixes Android writes")
    func readsAndroidsJSON() throws {
        let android = #"""
            [{"name":"Rehearsal","snapshot":"{\"channelGains\":[60,40],\"channelPans\":[0,100],\"channelMutes\":[false,true],\"channelSolos\":[true,false],\"groupLevels\":[80],\"volumePercent\":70,\"maxLevelPercent\":90,\"muted\":true}"}]
            """#
        try withThrowawaySuite { launch in
            let defaults = try launch()
            defaults.set(android, forKey: key)

            let mixes = UserDefaultsMixBankStore(defaults).savedMixes

            #expect(
                mixes == [
                    SavedMix(
                        name: "Rehearsal",
                        snapshot: MixSnapshot(
                            channelGains: [60, 40], channelPans: [0, 100], channelMutes: [false, true],
                            channelSolos: [true, false], groupLevels: [80], volumePercent: 70,
                            maxLevelPercent: 90, muted: true))
                ])
        }
    }

    /// Android skips an entry that is no object, has no text name, or has no snapshot it can read,
    /// and keeps every other (`MixBankStore.kt:94-114`).
    @Test("a damaged entry loses only itself")
    func aDamagedEntryLosesOnlyItself() throws {
        let stored = #"""
            [{"name":"Verse","snapshot":"{\"channelGains\":[30,60]}"},
             7,
             null,
             {"name":5,"snapshot":"{}"},
             {"name":"No snapshot"},
             {"name":"Nested","snapshot":{"channelGains":[10]}},
             {"name":"Broken","snapshot":"nope"},
             {"name":"A list","snapshot":"[1,2]"},
             {"name":"Chorus","snapshot":"{\"volumePercent\":70}"}]
            """#
        try withThrowawaySuite { launch in
            let defaults = try launch()
            defaults.set(stored, forKey: key)

            let mixes = UserDefaultsMixBankStore(defaults).savedMixes

            #expect(mixes.map(\.name) == ["Verse", "Chorus"])
            #expect(mixes.first?.snapshot == MixSnapshot(channelGains: [30, 60]))
            #expect(mixes.last?.snapshot == MixSnapshot(volumePercent: 70))
        }
    }

    /// Android reads each value with `optInt` and `optBoolean`: a number is read as its whole part
    /// and anything else as the default, so a list keeps its length and every channel its place.
    /// Android also reads a number or a `true` written as text; this reads text as the default, as
    /// the `config` decoder does (T2c), since neither client writes one.
    @Test("reads a damaged value as the default, keeping every channel in its place")
    func aDamagedValueReadsAsTheDefault() throws {
        let stored = #"""
            [{"name":"Odd","snapshot":"{\"channelGains\":[2.7,-3.9,true,\"5\",null,[1]],\"channelPans\":7,\"channelMutes\":[true,1,\"true\",null],\"channelSolos\":null,\"groupLevels\":[40],\"volumePercent\":\"loud\",\"maxLevelPercent\":55.5,\"muted\":1}"}]
            """#
        try withThrowawaySuite { launch in
            let defaults = try launch()
            defaults.set(stored, forKey: key)

            let mixes = UserDefaultsMixBankStore(defaults).savedMixes

            #expect(
                mixes.first?.snapshot
                    == MixSnapshot(
                        channelGains: [2, -3, 0, 0, 0, 0], channelPans: [],
                        channelMutes: [true, false, false, false], channelSolos: [], groupLevels: [40],
                        volumePercent: 100, maxLevelPercent: 55, muted: false))
        }
    }
}
