import BandEarDiscovery
import SwiftUI

/// The desks found on the local network, with one line saying what discovery is doing.
///
/// Provisional copy: the connect screen's final words belong to T8.
struct DeskListView: View {
    @StateObject private var discovery = DeskDiscoveryModel()

    var body: some View {
        NavigationStack {
            List {
                if let statusLine {
                    Text(statusLine)
                        .foregroundStyle(.secondary)
                }
                ForEach(discovery.desks) { desk in
                    DeskRow(desk: desk)
                }
            }
            .navigationTitle("Desks")
        }
        .onAppear { discovery.start() }
        .onDisappear { discovery.stop() }
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
            if let sampleRate = desk.sampleRate {
                Text("\(sampleRate) Hz")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
