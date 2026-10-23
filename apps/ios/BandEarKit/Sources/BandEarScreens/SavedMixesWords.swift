import Foundation

/// The Mixes tab's words that need no saved mix: its title and introduction, its empty state, and
/// the pure helpers its list and its rename use.
///
/// Ported from the Android client's `SavedMixesText.kt` and the copy of `SavedMixesScreen.kt`. The
/// words that report what the store did, and the store itself, are the saved mixes' own (T9).
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
}
