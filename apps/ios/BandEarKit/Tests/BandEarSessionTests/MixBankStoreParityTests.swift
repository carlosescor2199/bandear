import BandEarMix
import Testing

@testable import BandEarSession

/// The Android client's tests of how its saved mixes are written down, ported case for case: the
/// JSON tests of `MixSnapshotTest.kt` (its `MixSnapshotTest` and `MixBankStoreJsonTest`) and of
/// `GroupSnapshotTest.kt` (`apps/android/app/src/test/java/com/bandear/android/`).
///
/// Android's `MixSnapshot.toJson` and `fromJson` and its `serializeBanks` and `parseBanks` are the
/// four functions of ``SavedMixesJSON``. Its `fromJson` throws for text that is no snapshot; here
/// that is nil. Its More of me tests were ported with the lift (`MoreOfMeParityTests`).
@Suite("Mix bank store: Android parity")
struct MixBankStoreParityTests {

    // MARK: MixSnapshotTest

    @Test func roundTripsThroughJsonWithEveryFieldPreserved() throws {
        let original = sample()

        let restored = try #require(SavedMixesJSON.snapshot(fromJSON: SavedMixesJSON.json(of: original)))

        #expect(restored.channelGains == original.channelGains)
        #expect(restored.channelPans == original.channelPans)
        #expect(restored.channelMutes == original.channelMutes)
        #expect(restored.channelSolos == original.channelSolos)
        #expect(restored.volumePercent == 70)
        #expect(restored.maxLevelPercent == 90)
        #expect(restored.muted)
    }

    // MARK: MixBankStoreJsonTest

    @Test func roundTripsNamedBanksThroughJson() throws {
        let banks = [
            SavedMix(name: "Rehearsal", snapshot: snapshot(60)),
            SavedMix(name: "Show \"A\"", snapshot: snapshot(90)),
        ]

        let restored = SavedMixesJSON.mixes(fromJSON: SavedMixesJSON.json(of: banks))

        try #require(restored.count == 2)
        #expect(restored[0].name == "Rehearsal")
        #expect(restored[0].snapshot.channelGains.first == 60)
        #expect(restored[1].name == "Show \"A\"")
        #expect(restored[1].snapshot.channelGains.first == 90)
    }

    @Test func fallsBackToAnEmptyListForMalformedJson() {
        #expect(SavedMixesJSON.mixes(fromJSON: "{not json") == [])
    }

    @Test func fallsBackToAnEmptyListForMissingJson() {
        #expect(SavedMixesJSON.mixes(fromJSON: nil) == [])
    }

    @Test func skipsBanksWhoseSnapshotIsMalformed() {
        #expect(SavedMixesJSON.mixes(fromJSON: #"[{"name":"Broken","snapshot":"nope"}]"#) == [])
    }

    // MARK: GroupSnapshotTest

    @Test func roundTripsGroupLevelsThroughJson() throws {
        let original = MixSnapshot(channelGains: [100, 100], groupLevels: [80, 40])

        let restored = try #require(SavedMixesJSON.snapshot(fromJSON: SavedMixesJSON.json(of: original)))

        #expect(restored.groupLevels == [80, 40])
    }

    @Test func aSnapshotWithoutGroupLevelsSerializesAnEmptyList() throws {
        let snapshot = MixSnapshot(channelGains: [100])

        let restored = try #require(SavedMixesJSON.snapshot(fromJSON: SavedMixesJSON.json(of: snapshot)))

        #expect(restored.groupLevels.count == 0)
    }

    @Test func anOlderBankWithNoGroupLevelsReadsAsEmpty() throws {
        let legacy = #"{"channelGains":[100],"volumePercent":70}"#

        let restored = try #require(SavedMixesJSON.snapshot(fromJSON: legacy))

        #expect(restored.channelGains == [100])
        #expect(restored.groupLevels.count == 0)
    }
}

/// Android's `MixSnapshotTest.sample()`.
private func sample() -> MixSnapshot {
    MixSnapshot(
        channelGains: [80, 100, 40], channelPans: [0, 50, 100], channelMutes: [false, true, false],
        channelSolos: [true, false, false], volumePercent: 70, maxLevelPercent: 90, muted: true)
}

/// Android's `MixBankStoreJsonTest.snapshot(gain)`.
private func snapshot(_ gain: Int) -> MixSnapshot {
    MixSnapshot(channelGains: [gain], volumePercent: 100)
}
