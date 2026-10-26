import Testing

@testable import BandEarMix

/// The Android client's tests of its saved mixes, ported case for case: the saved-mix tests of
/// `MonitorReducerTest.kt`, the bank's half of the saved-mix tests of `MonitorViewModelTest.kt`, and
/// `GroupSnapshotTest.theDefaultSnapshotStartsWithNoGroups`
/// (`apps/android/app/src/test/java/com/bandear/android/`).
///
/// Android's reducer takes the list its store hands over (`MixesLoaded`, `MixSaved` and the rest)
/// and words what happened in the same step. Here ``MixBank`` makes each change itself and returns
/// what it did, and the words are the screen's (T9b), so where Android checks a notice a test checks
/// the outcome. Android's `MixesLoaded(names)` is a bank holding those names (``bank(_:)``). A
/// recall's other half, applying the saved mix, is ``MixSession``'s, ported in
/// `MixReducerParityTests`.
@Suite("Saved mixes: Android parity")
struct MixBankParityTests {

    // MARK: MonitorReducerTest.kt

    @Test func loadedMixesAreListedInStoredOrder() {
        let state = bank("Verse", "Chorus")

        #expect(state.names == ["Verse", "Chorus"])
        #expect(state.activeName == nil)
        #expect(!state.isEmpty)
    }

    @Test func theEmptyStateShowsWhenNothingIsSaved() {
        #expect(bank().isEmpty)
    }

    /// Android's `MixNotice("Enter a name for the mix")` is what a refused save says
    /// (`MonitorViewModel.kt:434-437`), so the refused save stands for it; its `MixMissing("Gone",
    /// ["Verse"])` is the list read again once "Gone" was not found, so the reload stands for it.
    @Test func theEmptyStateNeverShowsWhileOneMixIsSaved() {
        let oneSaved = [
            bank("Verse"),
            bank().after { _ = $0.save(MixSnapshot(), as: "Verse") },
            bank("Verse", "Chorus").after { $0.delete("Chorus") },
            bank("Verse").after { _ = $0.save(MixSnapshot(), as: "  ") },
            bank("Old").after { _ = $0.rename("Old", to: "Verse") },
            bank("Gone", "Verse").after { $0.reload(saved("Verse")) },
            bank("Verse").after { $0.activate("Verse") },
        ]

        for state in oneSaved {
            #expect(!state.isEmpty, "the empty state shows next to a saved mix")
            #expect(state.names == ["Verse"])
        }
    }

    /// Android's notice, "Deleted \"Verse\"", is the screen's (T9b); the outcome is what it words.
    @Test func deletingTheLastMixShowsTheEmptyState() {
        var state = bank("Verse")

        let outcome = state.delete("Verse")

        #expect(state.isEmpty)
        #expect(outcome == .deleted(name: "Verse"))
    }

    @Test func aSavedMixBecomesTheActiveOne() {
        var state = bank("Chorus")

        let outcome = state.save(MixSnapshot(), as: "Verse")

        #expect(state.names == ["Chorus", "Verse"])
        #expect(state.activeName == "Verse")
        #expect(outcome == .saved(name: "Verse"))
    }

    /// Android's `MixRecalled("Verse", …)` makes the recalled mix the active one; here the phone does
    /// that once the session took the mix (``MixBank/activate(_:)``).
    @Test func deletingTheActiveMixLeavesNoneActive() throws {
        var state = bank("Verse", "Chorus").after { $0.activate("Verse") }
        // Not in Android's test: without an active mix first, there would be nothing to lose.
        try #require(state.activeName == "Verse")

        state.delete("Verse")

        #expect(state.activeName == nil)
        #expect(state.names == ["Chorus"])
    }

    @Test func renamingTheActiveMixKeepsItActive() throws {
        var state = bank("Verse", "Chorus").after { _ = $0.save(MixSnapshot(), as: "Verse") }
        // Android's `MixSaved("Verse", ["Chorus", "Verse"])`: saving again moves the mix to the end.
        try #require(state.names == ["Chorus", "Verse"])

        let outcome = state.rename("Verse", to: "Bridge")

        #expect(state.names == ["Chorus", "Bridge"])
        #expect(state.activeName == "Bridge")
        #expect(outcome == .renamed(from: "Verse", to: "Bridge"))
    }

    /// Android's `MixMissing("Verse", [])` comes from its view model, which looked for the mix in the
    /// store and did not find it (`MonitorViewModel.kt:449-453`): here that is the list read again and
    /// ``MixBank/mix(named:)`` finding nothing. Its notice is the screen's (T9b).
    @Test func aMissingMixIsReportedAndTheListRefreshed() throws {
        var state = bank("Verse").after { $0.activate("Verse") }
        try #require(state.activeName == "Verse")

        state.reload([])

        #expect(state.mix(named: "Verse") == nil)
        #expect(state.activeName == nil)
        #expect(state.isEmpty)
    }

    @Test func aReloadThatLosesTheActiveMixLeavesNoneActive() throws {
        var state = bank("Verse").after { _ = $0.save(MixSnapshot(), as: "Verse") }
        try #require(state.activeName == "Verse")

        state.reload(saved("Chorus"))

        #expect(state.activeName == nil)
    }

    /// Android dispatches the refused save's notice by hand; here the save is refused, and its notice,
    /// "Enter a name for the mix", is the screen's (T9b).
    @Test func aRefusedSaveSaysWhy() {
        var state = bank("Verse")

        let outcome = state.save(MixSnapshot(), as: "  ")

        #expect(outcome == .nameNeeded)
        #expect(state.names == ["Verse"])
    }

    // MARK: GroupSnapshotTest.kt

    @Test func theDefaultSnapshotStartsWithNoGroups() {
        #expect(MixSnapshot().groupLevels.isEmpty)
    }

    // MARK: MonitorViewModelTest.kt, the bank's half

    /// The bank's half of `MonitorViewModelTest.kt:1027-1045`: the name is saved trimmed and becomes
    /// the active one, holding the mix as the musician left it. What the phone then sends and plays
    /// is T9b's.
    @Test func savingAndRecallingAMix() {
        let session = withChannels(2).after(.channelLevelChanged(channel: 0, level: 30))
        var state = bank()

        let outcome = state.save(session.mix.snapshot, as: " Verse ")

        #expect(outcome == .saved(name: "Verse"))
        #expect(state.names == ["Verse"])
        #expect(state.activeName == "Verse")
        #expect(state.mix(named: "Verse")?.snapshot.channelGains == [30, 100])
    }

    /// The bank's half of `MonitorViewModelTest.kt:1047-1059`.
    @Test func savingUnderAnExistingNameReplacesThatMix() {
        let session = withChannels(2).after(.channelLevelChanged(channel: 0, level: 30))
        var state = bank().after { _ = $0.save(session.mix.snapshot, as: "Verse") }
        let changed = session.after(.channelLevelChanged(channel: 0, level: 70))

        // As on the old screen, saving under a name already used updates that mix; it is not refused.
        let outcome = state.save(changed.mix.snapshot, as: "Verse")

        #expect(outcome == .saved(name: "Verse"))
        #expect(state.names == ["Verse"])
        #expect(state.mixes.map(\.snapshot.channelGains) == [[70, 100]])
    }

    /// The bank's half of `MonitorViewModelTest.kt:1061-1068`: Android's `saveMix` answers false and
    /// says "Enter a name for the mix" (T9b).
    @Test func savingNeedsAName() {
        var state = bank()

        let outcome = state.save(MixSnapshot(), as: "   ")

        #expect(outcome == .nameNeeded)
        #expect(state.isEmpty)
    }

    /// The bank's half of `MonitorViewModelTest.kt:1082-1101`. Android checks its store's list and
    /// its screen's, which are one list here; its notice, "A mix named \"Chorus\" already exists", is
    /// the screen's (T9b).
    @Test func renamingAndDeletingAMix() {
        var state = MixBank([
            SavedMix(name: "Verse", snapshot: MixSnapshot(volumePercent: 70)),
            SavedMix(name: "Chorus", snapshot: MixSnapshot()),
        ])

        let taken = state.rename("Verse", to: "Chorus")

        #expect(taken == .nameTaken(name: "Chorus"))
        #expect(state.names == ["Verse", "Chorus"])

        let renamed = state.rename("Verse", to: " Bridge ")

        #expect(renamed == .renamed(from: "Verse", to: "Bridge"))
        #expect(state.names == ["Chorus", "Bridge"])
        #expect(state.mixes.filter { $0.name == "Bridge" }.map(\.snapshot.volumePercent) == [70])

        state.delete("Chorus")

        #expect(state.names == ["Bridge"])
    }
}

/// Android's `MixesLoaded(names)`: a bank holding a default mix under each of `names`, none active.
private func bank(_ names: String...) -> MixBank {
    MixBank(saved(names))
}

/// A default mix saved under each of `names`.
private func saved(_ names: String...) -> [SavedMix] {
    saved(names)
}

private func saved(_ names: [String]) -> [SavedMix] {
    names.map { SavedMix(name: $0, snapshot: MixSnapshot()) }
}

extension MixBank {
    /// The bank after `change`: Android's `after` helper for the saved mixes.
    fileprivate func after(_ change: (inout MixBank) -> Void) -> MixBank {
        var next = self
        change(&next)
        return next
    }
}
