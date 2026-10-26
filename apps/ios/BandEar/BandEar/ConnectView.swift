import BandEarDiscovery
import BandEarScreens
import SwiftUI

/// The first screen, shown whenever no session runs: the brand, the musician's name, the servers
/// found on this network, each joined with one tap, and the way to type an address instead.
///
/// Follows the Android client's `ConnectScreen.kt`; every word comes from ``ConnectWords``.
struct ConnectView: View {
    @ObservedObject var session: SessionModel
    @ObservedObject var discovery: DeskDiscoveryModel

    var body: some View {
        List {
            Section {
                BrandHeader()
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 12, leading: 4, bottom: 0, trailing: 4))
            }
            nameSection
            serversSection
            Section {
                Button {
                    session.manualAddressOpen = true
                } label: {
                    HStack {
                        Label(ConnectWords.manualAddress, systemImage: "keyboard")
                        Spacer(minLength: 8)
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(Color.secondary.opacity(0.6))
                            .accessibilityHidden(true)
                    }
                }
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .toolbar(.hidden, for: .navigationBar)
    }

    private var nameSection: some View {
        Section {
            TextField(
                ConnectWords.nameLabel,
                text: Binding(get: { session.musicianName }, set: { session.setName($0) })
            )
            .textInputAutocapitalization(.words)
            .autocorrectionDisabled()
            .submitLabel(.done)
        } header: {
            Text(ConnectWords.nameCaption)
                .textCase(nil)
        } footer: {
            Text(ConnectWords.nameHint)
        }
    }

    private var serversSection: some View {
        // Discovery keeps listening while this screen shows, found servers or not, unless it
        // cannot: then the reason shows in place of searching.
        let problem = ConnectWords.discoveryProblem(discovery.status)
        return Section {
            if let banner = ConnectionBanner.couldNotConnect(session.refusal) {
                ConnectionBannerRow(banner: banner)
            }
            if let problem {
                Label {
                    Text(problem)
                } icon: {
                    Image(systemName: "wifi.exclamationmark")
                        .foregroundStyle(.orange)
                }
            }
            if !discovery.desks.isEmpty {
                ForEach(discovery.desks) { desk in
                    ServerRow(desk: desk) { session.connect(to: desk) }
                }
            } else if problem == nil {
                NoServersYet()
            }
        } header: {
            HStack {
                Text(ConnectWords.serversTitle)
                    .lineLimit(1)
                    .layoutPriority(1)
                Spacer(minLength: 8)
                if problem == nil {
                    Searching()
                }
            }
            .textCase(nil)
        } footer: {
            Text(ConnectWords.serversHint)
        }
    }
}

/// The app's name with the small capitals after it.
private struct BrandHeader: View {
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(ConnectWords.brand)
                .font(.largeTitle.bold())
            Text(ConnectWords.brandTag)
                .font(.caption.weight(.semibold))
                .tracking(2)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// Said beside the servers' title while discovery listens.
private struct Searching: View {
    var body: some View {
        HStack(spacing: 6) {
            ProgressView()
                .controlSize(.small)
            Text(ConnectWords.searching)
        }
        .font(.footnote)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(ConnectWords.searching)
    }
}

/// One found server: the whole row is its single Connect action.
private struct ServerRow: View {
    let desk: DiscoveredDesk
    let connect: () -> Void

    var body: some View {
        Button(action: connect) {
            HStack(spacing: 14) {
                Image(systemName: "server.rack")
                    .font(.title3)
                    .foregroundStyle(Color.accentColor)
                    .frame(width: 32)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(ConnectWords.serverCardTitle)
                        .font(.headline)
                        .foregroundStyle(.primary)
                    Text(ConnectWords.serverCardDetail(desk))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                Spacer(minLength: 8)
                Text(ConnectWords.connect)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Capsule().fill(Color.accentColor))
            }
            .padding(.vertical, 4)
        }
    }
}

/// The empty list, saying what to check.
private struct NoServersYet: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(ConnectWords.noServersTitle)
                .font(.headline)
            Text(ConnectWords.noServersDetail)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }
}
