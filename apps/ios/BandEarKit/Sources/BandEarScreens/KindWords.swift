import BandEarProtocol

/// What each kind of channel is called: the spoken name of the icon beside a channel's name, which
/// adds a shape to find the channel by and is never the only way to tell.
///
/// Ported from the Android client's `ChannelKind.label` (`ClientMixState.kt:10-21`), which names its
/// icons (`KindIcons.kt`) and the channel sheet's (`ChannelSheet.kt:203`).
public enum KindWords {
    /// The name of `kind`: "Voice", "Guitar", and so on; "Other" for any other source.
    public static func label(_ kind: ChannelKind) -> String {
        switch kind {
        case .voice: "Voice"
        case .guitar: "Guitar"
        case .bass: "Bass"
        case .keys: "Keys"
        case .drums: "Drums"
        case .percussion: "Percussion"
        case .horns: "Horns"
        case .strings: "Strings"
        case .playback: "Playback"
        case .click: "Click"
        case .other: "Other"
        }
    }
}
