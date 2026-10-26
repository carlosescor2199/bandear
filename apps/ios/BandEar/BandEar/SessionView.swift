import BandEarDiscovery
import BandEarSession
import SwiftUI

/// The session with one desk: which desk, how the connection stands, and how many strips the desk
/// announced.
///
/// Provisional copy: the Mix screen and its words belong to T8.
struct SessionView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        List {
            Section {
                LabeledContent("Desk", value: session.desk?.name ?? "")
                LabeledContent("Connection", value: connectionLine)
                // The reason stays set on an open the desk has not spoken on yet; it is no current
                // error then.
                if session.connection != .connected, let reason = session.connectionError {
                    Text(reason)
                        .foregroundStyle(.secondary)
                }
                LabeledContent("Strips", value: "\(session.stripCount)")
            }
            Section {
                Button("Disconnect", role: .destructive) {
                    session.disconnect()
                }
            }
        }
        .navigationTitle("Session")
    }

    private var connectionLine: String {
        switch session.connection {
        case .idle: "Not connected"
        case .connecting: "Connecting…"
        case .connected: "Connected"
        case .reconnecting: "Reconnecting…"
        }
    }
}
