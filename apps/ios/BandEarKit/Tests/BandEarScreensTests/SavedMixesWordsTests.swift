import BandEarMix
import BandEarScreens
import BandEarSession
import Testing

/// The Mixes tab's copy (`SavedMixesScreen.kt`, `MonitorUiState.kt:384`) and the notices no Android
/// test pins, word for word.
@Suite("Saved mixes words")
struct SavedMixesWordsTests {

    @Test func theMixesTabsCopyIsAndroids() {
        #expect(SavedMixesWords.screenTitle == "Saved mixes")
        #expect(
            SavedMixesWords.screenIntro
                == "Store your mix for a song or a set and bring it back with one tap.")
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
        #expect(
            SavedMixesWords.emptyDetail
                == "Save the current mix above, then bring it back here with one tap.")
    }

    /// `SavedMixesScreen.kt:192-204`
    @Test func theSaveCardsCopyIsAndroids() {
        #expect(SavedMixesWords.saveCaption == "SAVE THE CURRENT MIX")
        #expect(SavedMixesWords.saveFieldLabel == "Name for the current mix")
        #expect(SavedMixesWords.saveFieldPlaceholder == "Name, e.g. Ballads")
        #expect(SavedMixesWords.save == "Save")
    }

    /// `SavedMixesScreen.kt:135,241-243,260,273,281`
    @Test func theListsCopyIsAndroids() {
        #expect(SavedMixesWords.recall == "Recall")
        #expect(SavedMixesWords.recallLabel("Ballads") == "Recall Ballads")
        #expect(SavedMixesWords.moreActionsLabel("Ballads") == "More actions for Ballads")
        #expect(SavedMixesWords.rename == "Rename")
        #expect(SavedMixesWords.delete == "Delete")
        #expect(
            SavedMixesWords.menuHint
                == "Rename and delete are under ⋮, so a stray tap on stage never deletes a mix.")
    }

    /// `SavedMixesScreen.kt:305-335`
    @Test func theDialogsCopyIsAndroids() {
        #expect(SavedMixesWords.renameTitle("Soundcheck") == "Rename \"Soundcheck\"")
        #expect(SavedMixesWords.renameFieldLabel == "New name")
        #expect(SavedMixesWords.cancel == "Cancel")
        #expect(SavedMixesWords.deleteTitle("Soundcheck") == "Delete \"Soundcheck\"?")
        #expect(
            SavedMixesWords.deleteMessage
                == "The mix is removed from this phone. This cannot be undone.")
    }

    /// Names are quoted as they are given, spaces included: the saved mixes hand over the name that
    /// was asked for where Android does (`MonitorReducer.kt:150-160`).
    @Test func eachNoticeQuotesTheNamesItIsGiven() {
        #expect(SavedMixesWords.notice(.bank(.deleted(name: " Verse "))) == "Deleted \" Verse \"")
        #expect(
            SavedMixesWords.notice(.bank(.renamed(from: "Old", to: "New")))
                == "Renamed \"Old\" to \"New\"")
        #expect(SavedMixesWords.notice(.bank(.saved(name: "Ballads"))) == "Saved \"Ballads\"")
        #expect(SavedMixesWords.notice(.recalled(name: "Ballads")) == "Recalled \"Ballads\"")
    }

    /// The leader is named as the following banner names them (``MixWords/leaderName(_:)``).
    @Test func aRefusedRecallNamesTheLeaderAsTheBannerDoes() {
        #expect(
            SavedMixesWords.notice(.notRecalled(name: "Verse", because: .following(leader: "  Ana ")))
                == "\"Verse\" was not recalled: your mix follows Ana's")
        #expect(
            SavedMixesWords.notice(.notRecalled(name: "Verse", because: .following(leader: "   ")))
                == "\"Verse\" was not recalled: your mix follows another musician's")
    }

    /// A rename to the name the mix already has says nothing (`MonitorViewModel.kt:465`).
    @Test func anUnchangedRenameSaysNothing() {
        #expect(SavedMixesWords.notice(.bank(.unchanged)) == nil)
    }
}
