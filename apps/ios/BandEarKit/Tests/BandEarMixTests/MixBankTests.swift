import Testing

@testable import BandEarMix

/// The saved mixes' rules that the Android suite never reaches: its view model checks a rename and
/// its store deletes before either reaches a test (`MonitorViewModel.kt:458-472`,
/// `MixBankStore.kt:36-44`), and its reducer keeps the active mix one that is saved
/// (`MonitorReducer.kt:523-525`). Here all three are ``MixBank``'s.
@Suite("Saved mixes")
struct MixBankTests {

    /// Android checks a blank name first, then a mix that is gone, then the same name; only a taken
    /// name is in its suite (`renamingAndDeletingAMix`).
    @Test("a rename to a blank or the same name, or of a mix not saved, changes nothing")
    func renameRefusals() throws {
        var state = MixBank(saved("Chorus", "Verse"))
        state.activate("Verse")
        try #require(state.activeName == "Verse")
        let before = state

        #expect(state.rename("Verse", to: " \n ") == .nameNeeded)
        #expect(state.rename("Bridge", to: " ") == .nameNeeded)
        #expect(state.rename("Bridge", to: "Intro") == .missing(name: "Bridge"))
        #expect(state.rename("Verse", to: " Verse ") == .unchanged)
        #expect(state == before)
    }

    /// Android's store trims the name it deletes by, and its view model says "Deleted" whether or not
    /// a mix had that name (`MonitorViewModel.kt:475-478`).
    @Test("deletes by the name trimmed, and says so whether or not a mix had it")
    func deleteByTrimmedName() {
        var state = MixBank(saved("Verse", "Chorus"))

        #expect(state.delete(" Verse ") == .deleted(name: " Verse "))
        #expect(state.names == ["Chorus"])
        #expect(state.delete("Bridge") == .deleted(name: "Bridge"))
        #expect(state.names == ["Chorus"])
    }

    @Test("makes only a saved mix the active one")
    func activeIsSaved() throws {
        var state = MixBank(saved("Verse"))
        state.activate("Verse")
        try #require(state.activeName == "Verse")

        state.activate("Gone")

        #expect(state.activeName == nil)
    }
}

private func saved(_ names: String...) -> [SavedMix] {
    names.map { SavedMix(name: $0, snapshot: MixSnapshot()) }
}
