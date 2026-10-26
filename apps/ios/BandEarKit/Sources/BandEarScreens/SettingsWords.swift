/// What the Settings screen says: the session's desk with Disconnect, the musician's name and the
/// manual connection form. Its diagnostics card has words of its own (``Diagnostics``,
/// ``DiagnosticsWords``).
///
/// Ported from the Android client's `SettingsScreen.kt` and the server card's text in
/// `SettingsText.kt:62-71`. Android writes "server" for the desk; so do these words.
public enum SettingsWords {
    /// The screen's title.
    public static let title = "Settings"
    /// The back button of the manual address form opened from Connect, which returns to the list
    /// of servers.
    public static let backToServers = "Servers"
    /// The small capitals over the name field.
    public static let nameCaption = "YOUR NAME"
    /// The name field's label and placeholder.
    public static let nameLabel = "Your name"
    /// The server card's button while a session runs.
    public static let disconnect = "Disconnect"
    /// The title of the manual connection card.
    public static let manualConnectionTitle = "MANUAL CONNECTION"
    /// The address field's label.
    public static let serverAddressLabel = "Server address"
    /// The address field's placeholder.
    public static let serverAddressPlaceholder = "192.168.1.34"
    /// The label of the field for this phone's own UDP port.
    public static let audioPortLabel = "Audio port"
    /// The label of the field for the desk's WebSocket port.
    public static let controlPortLabel = "Control port"
    /// The manual connection card's button, enabled only while no session runs.
    public static let connectToThisAddress = "Connect to this address"
    /// The line under that button while a session runs.
    public static let disconnectFirst = "Disconnect first to connect to another server."

    /// The server card's title: the server while a session runs, otherwise that there is none
    /// (`serverCardTitle`, `SettingsText.kt:62-64`).
    public static func serverCardTitle(_ status: ConnectionStatus) -> String {
        status.sessionRunning ? "BandEar server" : "Not connected"
    }

    /// The line under the server card's title: where, and the pill's state, or what to do instead
    /// (`serverCardDetail`, `SettingsText.kt:66-71`).
    public static func serverCardDetail(_ status: ConnectionStatus) -> String {
        guard status.sessionRunning, let desk = status.desk else {
            return "Pick a server, or enter its address below."
        }
        return "\(ConnectionWords.deskLabel(desk)) · \(status.pill.words.lowercased())"
    }

    /// The server card's dot: the pill's colour while a session runs, grey otherwise
    /// (`SettingsScreen.kt:137`).
    public static func serverCardTone(_ status: ConnectionStatus) -> ConnectionTone {
        status.sessionRunning ? status.pill.tone : .neutral
    }

    /// The "Could not connect" banner under the manual form, for a connect refused with `problem`;
    /// nil when nothing was refused, or while a session runs (`SettingsScreen.kt:219-226`).
    public static func manualProblemBanner(_ problem: String?, sessionRunning: Bool)
        -> ConnectionBanner?
    {
        sessionRunning ? nil : ConnectionBanner.couldNotConnect(problem)
    }
}
