import BandEarMix

/// The words of a channel's sheet, opened from its strip's label: which strip it shows, the label
/// button's spoken name and pan, the pan and level captions, and its keys.
///
/// Ported from the Android client's `ChannelSheetText.kt`, with the words of `ChannelSheet.kt`.
/// The sheet's level, Mute and Solo read as the strip's do (``StripWords``), and a held channel's
/// note is ``lockedNote`` or ``MixWords/followingTitle(_:)``.
public enum ChannelSheetWords {
    /// The strip whose sheet is open among `strips`, found by the channel a strip reported, not by
    /// its position in the list; nil when no sheet is open or the desk no longer announces that
    /// channel, which closes the sheet (`sheetChannelFor`, `ChannelSheetText.kt:7-13`).
    public static func sheetStrip(in strips: [ChannelStrip], open channel: Int?) -> ChannelStrip? {
        guard let channel else { return nil }
        return strips.first { $0.index == channel }
    }

    /// What a screen reader calls `strip`'s label button, which opens its sheet: the channel by its
    /// name ("Channel 3" when it has none), whether it is a stereo pair, whether the engineer locked
    /// it and whose mix it follows (`channelSettingsLabel`, `ChannelSheetText.kt:15-24`).
    public static func settingsLabel(for strip: ChannelStrip) -> String {
        let name =
            strip.label == ChannelLayout.defaultName(channel: strip.index)
            ? "Channel \(strip.index + 1)" : strip.label
        let settings = strip.isStereo ? "\(name) stereo pair settings" : "\(name) settings"
        return StripWords.lockedLabel(settings, locked: strip.isLocked, following: strip.following)
    }

    /// The pan a strip's label button reports to a screen reader, as it shows it under the label:
    /// "Pan left 30", or a stereo pair's "Balance centre" (`channelPanState`,
    /// `ChannelSheetText.kt:26-31`).
    public static func panState(pan: Int, stereo: Bool = false) -> String {
        "\(stereo ? "Balance" : "Pan") \(PanScale.spokenText(pan).lowercased())"
    }

    /// The caption over the sheet's pan control; a stereo pair's is its balance (`panTitle`,
    /// `ChannelSheetText.kt:33-34`).
    public static func panTitle(stereo: Bool) -> String {
        stereo ? "BALANCE" : "PAN"
    }

    /// What a screen reader calls the sheet's pan control: "CH 3 pan", "Keys balance"
    /// (`channelPanDescription`, `ChannelSheetText.kt:36-37`).
    public static func panDescription(label: String, stereo: Bool = false) -> String {
        "\(label) \(stereo ? "balance" : "pan")"
    }

    /// ``panDescription(label:stereo:)`` for `strip`, held as the strip is held
    /// (`ChannelSheet.kt:147`).
    public static func panControlDescription(for strip: ChannelStrip) -> String {
        StripWords.lockedLabel(
            panDescription(label: strip.label, stereo: strip.isStereo), locked: strip.isLocked,
            following: strip.following)
    }

    /// The caption over the sheet's level fader (`ChannelSheet.kt:153`).
    public static let levelTitle = "LEVEL"

    /// The words that mark a stereo pair in its sheet, so it is not told by an icon alone
    /// (`STEREO_TAG`, `ChannelSheetText.kt:39-40`).
    public static let stereoTag = "Stereo pair"

    /// The words a locked channel's sheet shows over its disabled controls (`LOCKED_NOTE`,
    /// `ChannelSheetText.kt:42-43`).
    public static let lockedNote = "Locked by the engineer"

    /// The words that mark the musician's own channel in its sheet (`ChannelSheet.kt:223`).
    public static let yourChannel = "Your channel"

    /// What the sheet shows when no group lists the channel, as the strip does (`ChannelSheet.kt:214`).
    public static let noGroup = StripWords.noGroup

    /// The button that closes the sheet (`ChannelSheet.kt:183`).
    public static let done = "Done"

    /// The words on the sheet's Mute key, which change with it so its state is not told by colour
    /// alone (`muteKeyText`, `ChannelSheetText.kt:45-46`).
    public static func muteKeyText(muted: Bool) -> String {
        muted ? "Muted" : "Mute"
    }

    /// The words on the sheet's Solo key (`soloKeyText`, `ChannelSheetText.kt:48-49`).
    public static func soloKeyText(soloed: Bool) -> String {
        soloed ? "Soloed" : "Solo"
    }
}
