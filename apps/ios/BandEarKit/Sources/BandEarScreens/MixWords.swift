import BandEarDiscovery
import Foundation

/// What the row of strips on the Mix screen shows: one strip per channel, or one per group.
///
/// Ported from the Android client's `MixView` (`MixScreenText.kt:9-13`). Named for the row, so it
/// does not read as a SwiftUI view in the app that lays it out.
public enum MixRow: CaseIterable, Sendable {
    case channels
    case groups

    /// The row's name on the switch between them, and in its section title.
    public var label: String {
        switch self {
        case .channels: "Channels"
        case .groups: "Groups"
        }
    }
}

/// The Mix screen's own words: its header, the title over the row of strips, what an empty row
/// says, and how a followed mix is named.
///
/// Ported from the Android client's `MixScreenText.kt` and `MixStrips.kt`. The words of the strips,
/// the Master card, the channel sheet and the channel picker live beside this, one area each. The
/// connection banner is the connection words' (`MixScreenText.kt:117-150`).
public enum MixWords {
    /// The musician's name at the top of the Mix screen, trimmed, or the app's name before one is
    /// set (`headerName`, `MixScreenText.kt:111-112`).
    public static func headerName(_ name: String) -> String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "BandEar" : trimmed
    }

    /// The line under the name: the session's desk, or "No server" while none runs
    /// (`headerServerText`, `MixScreenText.kt:114-115`).
    ///
    /// Android names its server by address and port, because a beacon carries no name
    /// (`MonitorUiState.kt:69-73`). A desk here is named as the list names it, which is what the
    /// session started with.
    public static func headerServerText(_ desk: DiscoveredDesk?) -> String {
        desk?.name ?? "No server"
    }

    /// The small capitals over the row of strips, with how many it holds: "CHANNELS · 8"
    /// (`sectionTitle`, `MixScreenText.kt:95-96`).
    public static func sectionTitle(_ row: MixRow, count: Int) -> String {
        "\(row.label.uppercased()) · \(count)"
    }

    /// What the channel row says before the desk announced any channel (`MixStrips.kt:95`).
    public static let waitingForChannels = "Waiting for the server to announce its channels."

    /// What the group row says when the desk announced no group (`MixStrips.kt:120`).
    public static let noGroups = "The server has no groups."

    /// The musician a mix follows, as the phone names them: `following` trimmed, or "another
    /// musician" when the desk sent no name (`leaderName`, `MonitorUiState.kt:426`).
    public static func leaderName(_ following: String) -> String {
        let trimmed = following.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "another musician" : trimmed
    }

    /// The title of the following banner over the Mix screen, and the channel sheet's note, while the
    /// mix follows `following`'s: "Following Ana's mix" (`followingTitle`, `MixScreenText.kt:60-61`).
    public static func followingTitle(_ following: String) -> String {
        "Following \(leaderName(following))'s mix"
    }

    /// What the following banner says under its title: what follows the leader and what stays the
    /// musician's (`FOLLOWING_DETAIL`, `MixScreenText.kt:63-64`).
    public static let followingDetail =
        "Your channels and groups follow that mix. Your master, level cap and mute are still yours."
}
