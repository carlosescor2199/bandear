import BandEarScreens
import Testing

/// The Android client's `SavedMixesTextTest` (`apps/android/app/src/test/java/com/bandear/android/
/// ui/screens/SavedMixesTextTest.kt`), case for case.
@Suite("Saved mixes words: Android parity")
struct SavedMixesWordsParityTests {

    /// `SavedMixesTextTest.kt:10-14`
    @Test func theListTitleCountsTheMixes() {
        #expect(SavedMixesWords.listTitle(count: 3) == "YOUR MIXES · 3")
        #expect(SavedMixesWords.listTitle(count: 0) == "YOUR MIXES · 0")
    }

    /// `SavedMixesTextTest.kt:16-20`
    @Test func onlyTheActiveMixHasADetailLine() {
        #expect(SavedMixesWords.detail(active: true) == "Active now")
        #expect(SavedMixesWords.detail(active: false) == nil)
    }

    /// `SavedMixesTextTest.kt:22-27`
    @Test func aRenameNeedsANewNonBlankName() {
        #expect(SavedMixesWords.canRename(from: "Soundcheck", to: "Main set"))
        #expect(!SavedMixesWords.canRename(from: "Soundcheck", to: "   "))
        #expect(!SavedMixesWords.canRename(from: "Soundcheck", to: " Soundcheck "))
    }

    /// `SavedMixesTextTest.kt:29-32`
    @Test func aSavedMixClearsTheField() {
        #expect(SavedMixesWords.nameAfterSave(typed: "Ballads", saved: true) == "")
    }

    /// `SavedMixesTextTest.kt:34-38`
    @Test func aRefusedSaveKeepsWhatWasTyped() {
        #expect(SavedMixesWords.nameAfterSave(typed: "Ballads", saved: false) == "Ballads")
        #expect(SavedMixesWords.nameAfterSave(typed: "   ", saved: false) == "   ")
    }

    /// `SavedMixesTextTest.kt:40-44`
    @Test func aRenameConfirmsTheTrimmedName() {
        #expect(SavedMixesWords.renameTarget(from: "Soundcheck", to: "  Main set ") == "Main set")
        #expect(SavedMixesWords.renameTarget(from: "Soundcheck", to: "Main set") == "Main set")
    }

    /// `SavedMixesTextTest.kt:46-50`
    @Test func aBlankOrUnchangedRenameConfirmsNothing() {
        #expect(SavedMixesWords.renameTarget(from: "Soundcheck", to: "   ") == nil)
        #expect(SavedMixesWords.renameTarget(from: "Soundcheck", to: " Soundcheck ") == nil)
    }
}
