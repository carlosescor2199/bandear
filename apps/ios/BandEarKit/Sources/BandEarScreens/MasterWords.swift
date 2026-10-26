import BandEarMix

/// The Master card's words: the master level and its Mute key, the phone's level cap, and the
/// engineer's limit.
///
/// Ported from the Android client's `MasterCard` (`MixStrips.kt:324-416`) and the master words in
/// `MixScreenText.kt`. What the musician hears at most is ``MixState/effectiveCap``, so the limit's
/// words read it from the mix rather than work it out again.
public enum MasterWords {
    /// The card's small capitals (`MixStrips.kt:348`).
    public static let title = "MASTER"

    /// The master level in the card's corner: "85%", or "muted" in words (`masterValueText`,
    /// `MixScreenText.kt:98-99`).
    public static func valueText(volume: Int, muted: Bool) -> String {
        muted ? "muted" : "\(volume)%"
    }

    /// The master Mute key's words, which change with it so the state is not told by colour alone
    /// (`masterMuteText`, `MixScreenText.kt:101-102`).
    public static func muteText(muted: Bool) -> String {
        muted ? "Muted" : "Mute"
    }

    /// What a screen reader calls the master Mute key (`MixStrips.kt:401`).
    public static let muteDescription = "Mute master"

    /// What a screen reader calls the master fader (`MixStrips.kt:360`).
    public static func faderDescription(muted: Bool) -> String {
        muted ? "Master level, muted" : "Master level"
    }

    /// The level cap's line is three pieces, the number set apart from its words: "Level cap ",
    /// then ``levelCapValue(_:)``, then ``levelCapTrail`` (`MixStrips.kt:366-368`).
    public static let levelCapLead = "Level cap "

    /// The phone's own cap in the level cap's line: "90%" (`MixStrips.kt:367`).
    public static func levelCapValue(_ cap: Int) -> String {
        "\(cap)%"
    }

    /// The end of the level cap's line (`MixStrips.kt:368`).
    public static let levelCapTrail = " · protects your ears"

    /// What a screen reader calls the level cap control; it says when the engineer's limit applies
    /// too, and what the musician then hears at most (`levelCapDescription`,
    /// `MixScreenText.kt:85-89`).
    public static func levelCapDescription(for mix: MixState) -> String {
        let cap = "Level cap, the loudest this phone plays"
        guard mix.engineer.isLimited else { return cap }
        return cap
            + ". The engineer's limit of \(mix.engineer.limitPercent)% also applies: at most \(mix.effectiveCap)%"
    }

    /// The engineer's limit under the level cap, with what the musician hears at most; nil when the
    /// engineer set no limit (`engineerLimitText`, `MixScreenText.kt:78-83`).
    public static func engineerLimitText(for mix: MixState) -> String? {
        guard mix.engineer.isLimited else { return nil }
        return "Limited to \(mix.engineer.limitPercent)% by the engineer · at most \(mix.effectiveCap)%"
    }
}
