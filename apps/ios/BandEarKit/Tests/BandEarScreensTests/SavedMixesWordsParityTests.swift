import BandEarMix
import BandEarScreens
import BandEarSession
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

/// The Android client's saved-mix notices, from the tests that word one: the saved-mix cases of
/// `MonitorViewModelTest.kt` and `MonitorReducerTest.kt` (`apps/android/app/src/test/java/com/
/// bandear/android/ui/state/`).
///
/// Android's reducer words what its store did in the same step. Here the saved mixes say what they
/// did as a `SavedMixesNotice`, `SavedMixesParityTests` checks that they say it, and these check
/// the words. Where Android checks `emptyMessage`, these check the empty state's title, which the
/// screen shows while nothing is listed.
@Suite("Saved mixes notices: Android parity")
struct SavedMixesNoticeWordsParityTests {

    // MARK: MonitorViewModelTest

    /// `MonitorViewModelTest.kt:1027-1045`
    @Test func savingAndRecallingAMix() {
        #expect(SavedMixesWords.notice(.recalled(name: "Verse")) == "Recalled \"Verse\"")
    }

    /// `MonitorViewModelTest.kt:1061-1068`
    @Test func savingNeedsAName() {
        #expect(SavedMixesWords.notice(.bank(.nameNeeded)) == "Enter a name for the mix")
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
    }

    /// `MonitorViewModelTest.kt:1070-1080`
    @Test func recallingAMixThatIsGoneRefreshesTheList() {
        #expect(
            SavedMixesWords.notice(.bank(.missing(name: "Verse"))) == "\"Verse\" is no longer saved")
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
    }

    /// `MonitorViewModelTest.kt:1082-1101`
    @Test func renamingAndDeletingAMix() {
        #expect(
            SavedMixesWords.notice(.bank(.nameTaken(name: "Chorus")))
                == "A mix named \"Chorus\" already exists")
    }

    /// `MonitorViewModelTest.kt:657-670`
    @Test func recallingWhileFollowingSendsNothingAndSaysWhy() {
        #expect(
            SavedMixesWords.notice(.notRecalled(name: "Verse", because: .following(leader: "Ana")))
                == "\"Verse\" was not recalled: your mix follows Ana's")
    }

    // MARK: MonitorReducerTest

    /// `MonitorReducerTest.kt:1135-1147`
    @Test func recallingASavedMixWhileFollowingIsRefusedAndSaysWhy() {
        #expect(
            SavedMixesWords.notice(.notRecalled(name: "Verse", because: .following(leader: "Ana")))
                == "\"Verse\" was not recalled: your mix follows Ana's")
        // A leader with no name is still named in words.
        #expect(
            SavedMixesWords.notice(.notRecalled(name: "Verse", because: .following(leader: "")))
                == "\"Verse\" was not recalled: your mix follows another musician's")
    }

    /// `MonitorReducerTest.kt:1234-1240`
    @Test func deletingTheLastMixShowsTheEmptyState() {
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
        #expect(SavedMixesWords.notice(.bank(.deleted(name: "Verse"))) == "Deleted \"Verse\"")
    }

    /// `MonitorReducerTest.kt:1242-1248`
    @Test func aSavedMixBecomesTheActiveOne() {
        #expect(SavedMixesWords.notice(.bank(.saved(name: "Verse"))) == "Saved \"Verse\"")
    }

    /// `MonitorReducerTest.kt:1250-1280`
    @Test func recallingAMixAppliesItsValuesAndKeepsWhatTheServerAnnounced() {
        #expect(SavedMixesWords.notice(.recalled(name: "Verse")) == "Recalled \"Verse\"")
    }

    /// `MonitorReducerTest.kt:1320-1330`
    @Test func renamingTheActiveMixKeepsItActive() {
        #expect(
            SavedMixesWords.notice(.bank(.renamed(from: "Verse", to: "Bridge")))
                == "Renamed \"Verse\" to \"Bridge\"")
    }

    /// `MonitorReducerTest.kt:1332-1343`
    @Test func aMissingMixIsReportedAndTheListRefreshed() {
        #expect(
            SavedMixesWords.notice(.bank(.missing(name: "Verse"))) == "\"Verse\" is no longer saved")
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
    }

    /// `MonitorReducerTest.kt:1352-1358`
    @Test func aRefusedSaveSaysWhy() {
        #expect(SavedMixesWords.notice(.bank(.nameNeeded)) == "Enter a name for the mix")
    }
}
