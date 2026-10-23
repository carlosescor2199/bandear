import BandEarMix
import Foundation

/// The words of the More of me switch: its name, the caption that says whether it is on and which
/// channel it lifts, and why it is not doing what the musician last asked.
///
/// Ported from the Android client's `MoreOfMeToggle.kt` and `MoreOfMeNotice.message`
/// (`MonitorUiState.kt:335-351`). The reasons are ``MoreOfMeNotice``'s; only their words live here.
public enum MoreOfMeWords {
    /// The switch's name (`MoreOfMeToggle.kt:63`).
    public static let title = "More of me"

    /// The line under the switch's name: "On" or "Off" in words, so the state is never told by the
    /// fill colour alone, and the channel it lifts, "your channel" when `channelLabel` is blank
    /// (`moreOfMeCaption`, `MoreOfMeToggle.kt:28-36`).
    public static func caption(on: Bool, channelLabel: String) -> String {
        guard on else { return "Off · tap to lift your channel" }
        let trimmed = channelLabel.trimmingCharacters(in: .whitespacesAndNewlines)
        return "On · \(trimmed.isEmpty ? "your channel" : trimmed) lifted"
    }

    /// ``caption(on:channelLabel:)`` for `session`: on as More of me is, naming the musician's
    /// channel as its strip does (`MixScreen.kt:147,250-252`).
    public static func caption(for session: MixSession) -> String {
        caption(
            on: session.moreOfMe.isEnabled,
            channelLabel: ChannelPickerWords.chosenLabel(in: session) ?? "")
    }

    /// The words under the switch for `notice` (`MoreOfMeNotice.message`, `MonitorUiState.kt:335-351`).
    public static func message(for notice: MoreOfMeNotice) -> String {
        switch notice {
        case .needsChannel: "Choose your channel first, then turn More of me on"
        case .noChannelSelected: "More of me is off: no channel is chosen"
        case .channelOutOfRange: "More of me is off: your channel is no longer in the mix"
        case .mixChangedElsewhere: "More of me is off: your mix was changed from the desktop"
        case .channelLocked: "More of me is off: the engineer locked your channel"
        case .following: "More of me is off while your mix follows another musician's"
        case .channelPaired: "More of me is off: your channel is now part of a stereo pair"
        }
    }
}
