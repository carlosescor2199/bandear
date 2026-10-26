import BandEarDiscovery
import BandEarSession
import Foundation

/// The colour role of a sign that says how the connection is doing: the status pill, a banner, the
/// link-health light, the dot on the server card. The app turns it into a colour; every sign says
/// the same thing in words too, so nothing is told by colour alone.
///
/// Ported from the Android client's `ColorToken` (`ui/theme/Color.kt:12`). Only the role is ported,
/// not Android's palette: the iOS app draws with the system's colours.
public enum ConnectionTone: CaseIterable, Equatable, Sendable {
    case green
    case amber
    case red
    case blue
    case neutral
}

/// What the status pill at the top of the Mix screen can say.
///
/// Ported from the Android client's `PillKind` and its words and colours
/// (`ui/components/ConnectionPill.kt`), and the pill's spoken description
/// (`ui/components/StatusPill.kt:45`).
public enum ConnectionPill: CaseIterable, Equatable, Sendable {
    case connected
    case reconnecting
    case connecting
    case notConnected

    /// The pill for a control connection in `state` (Android's `pillKindFor`,
    /// `ConnectionPill.kt:9-21`).
    ///
    /// Android reads two more states as not connected, and neither exists here: `UNAVAILABLE`, a
    /// channel that gave up, because this session never gives up (it reconnects until ``ControlSession/stop()``,
    /// ``ReconnectPolicy``); and `CLOSED`, a stopped channel, which here is ``ControlSession/ConnectionState/idle``.
    public init(_ state: ControlSession.ConnectionState) {
        switch state {
        case .connecting: self = .connecting
        case .connected: self = .connected
        case .reconnecting: self = .reconnecting
        case .idle: self = .notConnected
        }
    }

    /// The words on the pill. They always say the state, so it is never told by colour alone.
    public var words: String {
        switch self {
        case .connected: "Connected"
        case .reconnecting: "Reconnecting"
        case .connecting: "Connecting"
        case .notConnected: "Not connected"
        }
    }

    /// The colour of the pill's dot, tint and border.
    public var tone: ConnectionTone {
        switch self {
        case .connected: .green
        case .reconnecting: .amber
        case .connecting: .blue
        case .notConnected: .neutral
        }
    }

    /// What a screen reader says for the pill: the state in words, never the colour.
    public var spokenDescription: String {
        "Connection status: \(words)"
    }
}

/// The session and its control connection, as the pill, the banners and Settings show them.
///
/// The iOS counterpart of the Android client's `ConnectionUiState` (`ui/state/MonitorUiState.kt:87-109`).
/// Android reduces it from events; here it is read from what the session holds, so the pill and the
/// status detail are derived rather than stored. The form's refusal and the typed address are not
/// here: the connect screen keeps them.
public struct ConnectionStatus: Equatable, Sendable {
    /// Where the control connection stands (``ControlSession/connection``).
    public var connection: ControlSession.ConnectionState
    /// True from a start until a stop, or until the receiver ends on its own.
    public var sessionRunning: Bool
    /// The desk of the running session; nil when no session runs.
    public var desk: DeskEndpoint?
    /// True once the desk acknowledged a mix (``ControlSession/mixHeldByDesk``, Android's
    /// `mixHeldByServer`): the desk then applies the mix, and keeps it while the connection is away.
    /// False means this phone still mixes on its own.
    public var mixHeldByDesk: Bool

    public init(
        connection: ControlSession.ConnectionState = .idle, sessionRunning: Bool = false,
        desk: DeskEndpoint? = nil, mixHeldByDesk: Bool = false
    ) {
        self.connection = connection
        self.sessionRunning = sessionRunning
        self.desk = desk
        self.mixHeldByDesk = mixHeldByDesk
    }

    /// The pill for ``connection``.
    public var pill: ConnectionPill {
        ConnectionPill(connection)
    }

    /// True while the control connection retries on its own; the reconnecting banner shows then.
    public var reconnecting: Bool {
        pill == .reconnecting
    }

    /// The control connection's state in words, including who holds the mix
    /// (``ConnectionWords/statusDetail(_:mixHeldByDesk:)``).
    public var statusDetail: String {
        ConnectionWords.statusDetail(connection, mixHeldByDesk: mixHeldByDesk)
    }

    /// The banner for a session whose control connection is away, or nil when there is none to show
    /// (Android's `connectionBanner`, `ui/screens/MixScreenText.kt:117-150`).
    ///
    /// While the connection retries it says who holds the mix meanwhile. Once it is not connected at
    /// all, the musician has to connect again, so the banner says how; this session never gives up,
    /// so that one shows only for a running session whose connection reads idle. Connecting and
    /// connected need no banner: the pill says it.
    public var banner: ConnectionBanner? {
        guard sessionRunning else { return nil }
        switch pill {
        case .reconnecting:
            return ConnectionBanner(
                tone: .amber,
                title: "Reconnecting to \(desk.map(ConnectionWords.deskLabel) ?? "the server")",
                detail: mixHeldByDesk
                    ? "Your mix is held at the server. It retries on its own."
                    : "This phone keeps mixing until it is back. It retries on its own.",
                busy: true)
        case .notConnected:
            return ConnectionBanner(
                tone: .red,
                title: "Lost the server",
                detail: mixHeldByDesk
                    ? "Your mix stays at the server's last setting. Disconnect in Settings, then connect again."
                    : "This phone keeps mixing. Disconnect in Settings, then connect again.",
                busy: false)
        case .connected, .connecting:
            return nil
        }
    }
}

/// A banner under the Mix header (or over a refused connect), always in words, with its tone.
///
/// Ported from the Android client's `ConnectionBanner` (`ui/screens/MixScreenText.kt:18-19`) and the
/// two other `StatusBanner`s its screens build with fixed words: the following banner
/// (`ui/screens/MixScreen.kt:133-143`) and "Could not connect" (`ui/screens/ConnectScreen.kt:150-156`,
/// `ui/screens/SettingsScreen.kt:219-226`).
public struct ConnectionBanner: Equatable, Sendable {
    public let tone: ConnectionTone
    public let title: String
    public let detail: String
    /// Whether the banner shows it is working on it, as a spinner: only while the connection retries.
    public let busy: Bool

    public init(tone: ConnectionTone, title: String, detail: String, busy: Bool) {
        self.tone = tone
        self.title = title
        self.detail = detail
        self.busy = busy
    }

    /// The banner while the engineer has this mix follow `following`'s (``MixState/following``),
    /// saying whose and what stays the musician's; nil while the mix follows nobody's.
    ///
    /// Its words are the Mix screen's, kept once in ``MixWords``: Android's `followingTitle` and
    /// `FOLLOWING_DETAIL` (`MixScreenText.kt:60-64`), with the leader named by ``MixWords/leaderName(_:)``.
    public static func following(_ following: String?) -> ConnectionBanner? {
        guard let following else { return nil }
        return ConnectionBanner(
            tone: .blue, title: MixWords.followingTitle(following), detail: MixWords.followingDetail,
            busy: false)
    }

    /// The title over a connect the phone refused.
    public static let couldNotConnectTitle = "Could not connect"

    /// The banner over a connect the phone refused, with `problem` as the detail; nil when nothing
    /// was refused.
    public static func couldNotConnect(_ problem: String?) -> ConnectionBanner? {
        guard let problem else { return nil }
        return ConnectionBanner(tone: .red, title: couldNotConnectTitle, detail: problem, busy: false)
    }
}

/// The connection's words that are not a property of one value.
public enum ConnectionWords {
    /// The control connection's state in words, including who owns the mix (Android's
    /// `controlStatusLabel`, `ControlStatus.kt:5-34`).
    ///
    /// Once the desk acknowledged a mix it applies that mix for the rest of the session, and keeps
    /// applying it while the connection is away; until then this phone mixes on its own. Android's
    /// words for a channel that gave up or was stopped ("channel lost - mix held at the server's last
    /// setting", "unavailable - mixing on this device") have no state here to say them for: this
    /// session never gives up, and a stopped one is idle.
    public static func statusDetail(
        _ state: ControlSession.ConnectionState, mixHeldByDesk: Bool
    ) -> String {
        switch state {
        case .idle:
            "not connected"
        case .connecting:
            "connecting..."
        case .connected:
            mixHeldByDesk
                ? "connected - mix applied on the server"
                : "connected - waiting for the server to take over"
        // The connection dropped and the session retries on its own. Ownership is unchanged until it
        // reopens: a desk that took the mix keeps applying it, otherwise this phone still mixes.
        case .reconnecting:
            mixHeldByDesk
                ? "reconnecting - mix held at the server" : "reconnecting - mixing on this device"
        }
    }

    /// How a desk is named on screen (Android's `ServerEndpoint.label`,
    /// `ui/state/MonitorUiState.kt:68-73`).
    ///
    /// Android's beacons carry no name, so it names a server by `address:port`. A desk found over
    /// Bonjour has one, `BandEar on <host>` as the desk writes it, and its address is not known
    /// until the connection opens, so it is named by that; a typed desk is named as Android names
    /// every server.
    public static func deskLabel(_ desk: DeskEndpoint) -> String {
        switch desk {
        case .bonjour(let name, _, _): name
        case .manual(let host, let controlPort): "\(host):\(controlPort)"
        }
    }
}
