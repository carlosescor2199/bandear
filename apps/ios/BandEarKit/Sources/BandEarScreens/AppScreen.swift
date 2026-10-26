/// The tabs of a running session, in the order the bar shows them.
///
/// Ported from the Android client's `MainTab` (`ui/components/BottomTabBar.kt:35-39`).
public enum AppTab: CaseIterable, Equatable, Sendable {
    case mix
    case mixes
    case settings

    /// The tab's name under its icon.
    public var label: String {
        switch self {
        case .mix: "Mix"
        case .mixes: "Mixes"
        case .settings: "Settings"
        }
    }
}

/// The screens of the app. Only the three of a running session sit above the tab bar.
///
/// Ported from the Android client's `AppScreen` (`ui/screens/AppScreen.kt`). The channel sheet opens
/// over Mix and the diagnostics live inside Settings, so neither is a screen of its own.
public enum AppScreen: CaseIterable, Equatable, Sendable {
    /// Pick a found desk; shows whenever no session runs.
    case connect
    /// The Settings form opened from Connect, to type a desk's address.
    case manualAddress
    case mix
    case mixes
    case settings

    /// Whether the tab bar shows under this screen: only a running session's screens have it.
    public var showsTabs: Bool {
        switch self {
        case .connect, .manualAddress: false
        case .mix, .mixes, .settings: true
        }
    }

    /// The screen to show (Android's `appScreenFor`, `AppScreen.kt:17-30`).
    ///
    /// A running session shows its selected tab. Without one, Connect shows, or the manual address
    /// form when the musician opened it from Connect; the tab a finished session left selected
    /// never hides Connect.
    public init(sessionRunning: Bool, tab: AppTab, manualAddressOpen: Bool) {
        if sessionRunning {
            switch tab {
            case .mix: self = .mix
            case .mixes: self = .mixes
            case .settings: self = .settings
            }
        } else if manualAddressOpen {
            self = .manualAddress
        } else {
            self = .connect
        }
    }
}
