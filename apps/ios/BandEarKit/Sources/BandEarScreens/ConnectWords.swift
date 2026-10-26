import BandEarDiscovery
import BandEarSession

/// What the Connect screen says: the brand, the musician's name, the servers found on this network
/// and the way to type one, and why a connect was refused.
///
/// Ported from the Android client's `ui/screens/ConnectScreen.kt`, every string, and the refusals
/// its `MonitorViewModel.startSession` words (`ui/state/MonitorViewModel.kt:241-247`). Android writes
/// "server" for the desk; so do these words, except where the iOS app already says "desk" for what
/// Android has no words for.
public enum ConnectWords {
    /// The app's name beside its mark.
    public static let brand = "BandEar"
    /// The small capitals after the name.
    public static let brandTag = "MONITOR"
    /// The small capitals over the name field.
    public static let nameCaption = "YOUR NAME"
    /// The name field's label and placeholder.
    public static let nameLabel = "Your name"
    /// The line under the name field.
    public static let nameHint = "The engineer sees this name on the desktop."
    /// The small capitals over the list of servers.
    public static let serversTitle = "SERVERS ON THIS NETWORK"
    /// Beside ``serversTitle`` while discovery listens, found servers or not.
    public static let searching = "Searching"
    /// The line under the list.
    public static let serversHint =
        "Servers appear here on their own. Tap one to join; the phone stays connected and "
        + "reconnects on its own if the Wi-Fi drops."
    /// A found server's title on its card.
    public static let serverCardTitle = "BandEar server"
    /// A server card's single action, on its button and for a screen reader.
    public static let connect = "Connect"
    /// The empty list's title.
    public static let noServersTitle = "No servers found yet"
    /// The line under ``noServersTitle``.
    public static let noServersDetail = "Check that this phone is on the same Wi-Fi as the BandEar desktop."
    /// The button that opens the manual address form.
    public static let manualAddress = "Enter the server address manually"
    /// Shown instead of searching while the musician has not allowed local network access, so no
    /// desk can ever appear. iOS only, so Android has no words for it; these are the ones the app
    /// already shows.
    public static let localNetworkDenied =
        "BandEar needs Local Network access to find the desk. Allow it in Settings, Privacy & "
        + "Security, Local Network."

    /// The line under a found server's title: the desk's name (``ConnectionWords/deskLabel(_:)``).
    public static func serverCardDetail(_ desk: DiscoveredDesk) -> String {
        ConnectionWords.deskLabel(desk.endpoint)
    }

    /// Why discovery cannot find a desk, said instead of ``searching``; nil while it can, when the
    /// screen says ``searching``.
    ///
    /// Android's beacon listener has no such states, so these words are iOS's own:
    /// ``localNetworkDenied``, and for any other failure "Could not search for servers:" and the
    /// browser's reason, in the voice of Android's "Could not resolve server host:".
    public static func discoveryProblem(_ status: DiscoveryStatus) -> String? {
        switch status {
        case .idle, .browsing: nil
        case .localNetworkDenied: localNetworkDenied
        case .unavailable(let reason): "Could not search for servers: \(reason)"
        }
    }

    /// The words for a connect the phone refused, shown in the "Could not connect" banner
    /// (``ConnectionBanner/couldNotConnect(_:)``).
    ///
    /// - The form's mistakes (``ConnectionFormError``) and a receiver still stopping
    ///   (``MonitorAudioError/previousReceiverStopping``) get Android's words, checked in Android's
    ///   order by the form.
    /// - Anything else has no Android words, and is described as the error describes itself.
    public static func problem(for error: any Error) -> String {
        switch error {
        case ConnectionFormError.missingHost:
            "Enter the server IP address or host name"
        case ConnectionFormError.invalidAudioPort:
            "Enter a valid UDP port between 1 and 65535"
        case ConnectionFormError.invalidControlPort:
            "Enter a valid WebSocket control port between 1 and 65535"
        case MonitorAudioError.previousReceiverStopping:
            "The previous receiver is still stopping"
        default:
            String(describing: error)
        }
    }
}
