import BandEarMix
import BandEarSession
import Foundation

/// What the Mixes tab says: its title and introduction, the save card, the list with its menu and
/// dialogs, the empty state, what the last save, recall, rename or delete did, and the pure helpers
/// its list and its rename use.
///
/// Ported from the Android client's `SavedMixesText.kt`, the copy of `SavedMixesScreen.kt`, and the
/// saved-mix notices its view model and reducer word (`MonitorViewModel.kt:433-478`,
/// `MonitorReducer.kt:132-162`), every string verbatim.
public enum SavedMixesWords {
    /// The tab's title (`SavedMixesScreen.kt:90`).
    public static let screenTitle = "Saved mixes"

    /// The line under the tab's title (`SavedMixesScreen.kt:92`).
    public static let screenIntro = "Store your mix for a song or a set and bring it back with one tap."

    /// What the tab shows in place of the list while nothing is saved (`MonitorUiState.kt:384`).
    public static let emptyTitle = "No saved mixes yet"

    /// The line under ``emptyTitle`` (`SavedMixesScreen.kt:357`).
    public static let emptyDetail = "Save the current mix above, then bring it back here with one tap."

    /// The small capitals over the list of saved mixes, with how many there are: "YOUR MIXES · 3"
    /// (`savedMixesTitle`, `SavedMixesText.kt:3-4`).
    public static func listTitle(count: Int) -> String {
        "YOUR MIXES · \(count)"
    }

    /// The line under a saved mix's name: only the active one has one, and it says so in words
    /// (`savedMixDetail`, `SavedMixesText.kt:6-7`).
    public static func detail(active: Bool) -> String? {
        active ? "Active now" : nil
    }

    /// What the save field keeps after Save: nothing once the mix was `saved`, and everything
    /// `typed` when the save was refused, so the name can be corrected instead of typed again
    /// (`nameAfterSave`, `SavedMixesText.kt:9-13`).
    public static func nameAfterSave(typed: String, saved: Bool) -> String {
        saved ? "" : typed
    }

    /// The name a rename confirms: the new name without the spaces around it, or nil while it is
    /// blank or the same as the old one (`renameTarget`, `SavedMixesText.kt:15-19`).
    public static func renameTarget(from: String, to: String) -> String? {
        let name = to.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty || name == from ? nil : name
    }

    /// A rename can be confirmed once the new name is not blank and differs from the old one
    /// (`canRename`, `SavedMixesText.kt:21-22`).
    public static func canRename(from: String, to: String) -> Bool {
        renameTarget(from: from, to: to) != nil
    }

    // MARK: The save card

    /// The small capitals over the save field (`SavedMixesScreen.kt:192`).
    public static let saveCaption = "SAVE THE CURRENT MIX"

    /// The save field's label (`SavedMixesScreen.kt:197`).
    public static let saveFieldLabel = "Name for the current mix"

    /// What the save field shows while it is empty (`SavedMixesScreen.kt:199`).
    public static let saveFieldPlaceholder = "Name, e.g. Ballads"

    /// The button that saves the current mix under the typed name (`SavedMixesScreen.kt:204`).
    public static let save = "Save"

    // MARK: The list

    /// A saved mix's button that brings it back (`SavedMixesScreen.kt:241`).
    public static let recall = "Recall"

    /// What a screen reader says for the Recall button of the mix saved as `name`
    /// (`SavedMixesScreen.kt:243`).
    public static func recallLabel(_ name: String) -> String {
        "Recall \(name)"
    }

    /// What a screen reader says for the ⋮ button of the mix saved as `name`
    /// (`SavedMixesScreen.kt:260`).
    public static func moreActionsLabel(_ name: String) -> String {
        "More actions for \(name)"
    }

    /// The menu item that opens the rename dialog, and the dialog's confirming button
    /// (`SavedMixesScreen.kt:273,305`).
    public static let rename = "Rename"

    /// The menu item that opens the delete dialog, and the dialog's confirming button
    /// (`SavedMixesScreen.kt:281,332`).
    public static let delete = "Delete"

    /// The line under the list (`SavedMixesScreen.kt:135`).
    public static let menuHint =
        "Rename and delete are under ⋮, so a stray tap on stage never deletes a mix."

    // MARK: The dialogs

    /// The rename dialog's title, for the mix saved as `from` (`SavedMixesScreen.kt:308`).
    public static func renameTitle(_ from: String) -> String {
        "Rename \"\(from)\""
    }

    /// The rename dialog's field label (`SavedMixesScreen.kt:313`).
    public static let renameFieldLabel = "New name"

    /// Both dialogs' button that closes them, changing nothing (`SavedMixesScreen.kt:307,333`).
    public static let cancel = "Cancel"

    /// The delete dialog's title, for the mix saved as `name` (`SavedMixesScreen.kt:334`).
    public static func deleteTitle(_ name: String) -> String {
        "Delete \"\(name)\"?"
    }

    /// What the delete dialog says under its title (`SavedMixesScreen.kt:335`).
    public static let deleteMessage = "The mix is removed from this phone. This cannot be undone."

    // MARK: What the last action did

    /// What the last save, recall, rename or delete did, or why it did nothing, as Android words it
    /// (`MonitorReducer.kt:132-162`, `MonitorViewModel.kt:436,466,529`); nil for a rename to the name
    /// the mix already has, which says nothing. Each name is quoted as given; the leader a mix
    /// follows is named as the following banner names them (``MixWords/leaderName(_:)``).
    public static func notice(_ notice: SavedMixesNotice) -> String? {
        switch notice {
        case .bank(.saved(let name)):
            "Saved \"\(name)\""
        case .recalled(let name):
            "Recalled \"\(name)\""
        case .notRecalled(let name, .following(let leader)):
            "\"\(name)\" was not recalled: your mix follows \(MixWords.leaderName(leader))'s"
        case .bank(.missing(let name)):
            "\"\(name)\" is no longer saved"
        case .bank(.deleted(let name)):
            "Deleted \"\(name)\""
        case .bank(.renamed(let from, let to)):
            "Renamed \"\(from)\" to \"\(to)\""
        case .bank(.nameNeeded):
            "Enter a name for the mix"
        case .bank(.nameTaken(let name)):
            "A mix named \"\(name)\" already exists"
        case .bank(.unchanged):
            nil
        }
    }
}
