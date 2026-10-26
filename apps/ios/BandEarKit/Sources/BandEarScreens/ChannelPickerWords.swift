import BandEarMix

/// The words of the "My channel" picker beside More of me, and the channel it names.
///
/// Ported from the Android client's `MyChannelPicker` (`MixScreen.kt:285-374`) and the picker's
/// words in `MixScreenText.kt`. Its choices are ``MixState/musicianChoices``: one per strip, by name.
public enum ChannelPickerWords {
    /// The small capitals over the chosen channel (`MixScreen.kt:316`).
    public static let title = "MY CHANNEL"

    /// The menu's first entry, which chooses no channel (`MixScreen.kt:341`).
    public static let none = "None"

    /// The menu's disabled entry while the desk has announced no channel (`MixScreen.kt:344`).
    public static let waiting = "Waiting for the server's channels"

    /// What a screen reader is told the picker does (`MixScreen.kt:309`).
    public static let chooseHint = "Choose your channel"

    /// What a screen reader calls the tick beside the chosen entry (`MixScreen.kt:371`).
    public static let chosen = "Chosen"

    /// The channel on the picker, or "None" (`myChannelText`, `MixScreenText.kt:104-105`).
    public static func text(_ label: String?) -> String {
        label ?? none
    }

    /// What a screen reader says for the picker (`myChannelDescription`, `MixScreenText.kt:107-109`).
    public static func description(_ label: String?) -> String {
        guard let label else { return "My channel: none chosen. Tap to choose" }
        return "My channel: \(label). Tap to change"
    }

    /// The musician's channel as its strip names it, or nil when none is chosen. The picker and the
    /// More of me caption both name it so (`MixScreen.kt:147`).
    public static func chosenLabel(in session: MixSession) -> String? {
        session.moreOfMe.musicianChannel.map(session.mix.label(of:))
    }
}
