import BandEarDiscovery
import SwiftUI

/// The musician's name and the desks found on the local network, with one line saying what
/// discovery is doing. Choosing a desk connects to it and opens its session.
///
/// Provisional copy: the connect screen's final words belong to T8.
struct DeskListView: View {
    @StateObject private var discovery = DeskDiscoveryModel()
    @StateObject private var session = SessionModel()

    var body: some View {
        NavigationStack {
            List {
                Section("Your name") {
                    TextField(session.deviceName, text: $session.musicianName)
                }
                Section("Desks") {
                    if let statusLine {
                        Text(statusLine)
                            .foregroundStyle(.secondary)
                    }
                    if let refusal = session.refusal {
                        Text(refusal)
                            .foregroundStyle(.red)
                    }
                    ForEach(discovery.desks) { desk in
                        Button {
                            session.connect(to: desk)
                        } label: {
                            DeskRow(desk: desk)
                        }
                    }
                }
            }
            .navigationTitle("BandEar")
            .navigationDestination(isPresented: inSession) {
                SessionView(session: session)
            }
        }
        .onAppear { discovery.start() }
        .onDisappear { discovery.stop() }
    }

    /// True while a session runs. Going back from it ends the session, as Disconnect does.
    private var inSession: Binding<Bool> {
        Binding(
            get: { session.desk != nil },
            set: { presented in
                if !presented { session.disconnect() }
            })
    }

    /// What discovery is doing, or nil when the list speaks for itself.
    private var statusLine: String? {
        switch discovery.status {
        case .idle:
            nil
        case .browsing:
            discovery.desks.isEmpty ? "Searching for desks…" : nil
        case .localNetworkDenied:
            "BandEar needs Local Network access to find the desk. Allow it in Settings, Privacy & Security, Local Network."
        case .unavailable(let reason):
            reason
        }
    }
}

private struct DeskRow: View {
    let desk: DiscoveredDesk

    var body: some View {
        VStack(alignment: .leading) {
            Text(desk.name)
                .foregroundStyle(.primary)
            if let sampleRate = desk.sampleRate {
                Text("\(sampleRate) Hz")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
