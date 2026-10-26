import BandEarDiscovery
import BandEarScreens
import SwiftUI

/// The first screen, shown whenever no session runs: the brand, the musician's name, the servers
/// found on this network, each joined with one tap, and the way to type an address instead.
///
/// Follows the Android client's `ConnectScreen.kt` in the look of the "Connect" boards: one column
/// in portrait; in landscape the brand and the name on the left, the servers on the right with the
/// manual address at their foot. Every word comes from ``ConnectWords``.
struct ConnectView: View {
    @ObservedObject var session: SessionModel
    @ObservedObject var discovery: DeskDiscoveryModel
    @Environment(\.screenOrientation) private var orientation
    @FocusState private var nameFocused: Bool

    var body: some View {
        let landscape = orientation.isLandscape
        // One layout or the other around the same two columns, so the name field keeps its focus
        // through a rotation.
        let layout =
            landscape
            ? AnyLayout(HStackLayout(alignment: .top, spacing: 40))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 36))
        GeometryReader { proxy in
            ScrollView {
                layout {
                    VStack(alignment: .leading, spacing: landscape ? 28 : 36) {
                        BrandHeader()
                        nameSection
                    }
                    .frame(width: landscape ? 300 : nil)
                    .frame(maxWidth: landscape ? nil : .infinity, alignment: .leading)
                    VStack(alignment: .leading, spacing: 0) {
                        serversSection
                        // In landscape the manual address sits at the foot of the screen.
                        Spacer(minLength: landscape ? 16 : 36)
                        ManualAddressLink { session.manualAddressOpen = true }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                }
                .padding(.horizontal, 20)
                .padding(.top, landscape ? 20 : 24)
                .padding(.bottom, landscape ? 16 : 32)
                .frame(minHeight: landscape ? proxy.size.height : nil, alignment: .top)
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .background(BandEarColors.ground.ignoresSafeArea())
        .toolbar(.hidden, for: .navigationBar)
    }

    private var nameSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionCaption(text: ConnectWords.nameCaption)
            TextField(
                ConnectWords.nameLabel,
                text: Binding(get: { session.musicianName }, set: { session.setName($0) }),
                prompt: fieldPrompt(ConnectWords.nameLabel)
            )
            .textInputAutocapitalization(.words)
            .autocorrectionDisabled()
            .submitLabel(.done)
            .focused($nameFocused)
            .boxedField { nameFocused = true }
            SectionHint(text: ConnectWords.nameHint)
        }
    }

    private var serversSection: some View {
        // Discovery keeps listening while this screen shows, found servers or not, unless it
        // cannot: then the reason shows in place of searching.
        let problem = ConnectWords.discoveryProblem(discovery.status)
        return VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                SectionCaption(text: ConnectWords.serversTitle)
                    .lineLimit(1)
                    .layoutPriority(1)
                Spacer(minLength: 8)
                if problem == nil {
                    Searching()
                }
            }
            .frame(minHeight: 20)
            if let banner = ConnectionBanner.couldNotConnect(session.refusal) {
                ConnectionBannerCard(banner: banner)
            }
            if let problem {
                DiscoveryProblem(text: problem)
            }
            if !discovery.desks.isEmpty {
                ForEach(discovery.desks) { desk in
                    ServerCard(desk: desk) { session.connect(to: desk) }
                }
            } else if problem == nil {
                NoServersYet()
            }
            SectionHint(text: ConnectWords.serversHint)
        }
    }
}

/// The app's icon and name with the small capitals under it.
private struct BrandHeader: View {
    var body: some View {
        HStack(spacing: 12) {
            ConnectBrandMark()
            VStack(alignment: .leading, spacing: 0) {
                Text(ConnectWords.brand)
                    .bandEarFont(BandEarType.titleLarge)
                    .foregroundStyle(BandEarColors.text)
                Text(ConnectWords.brandTag)
                    .bandEarFont(BandEarType.caps)
                    .foregroundStyle(BandEarColors.textSecondary)
            }
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
                .tint(BandEarColors.blue)
            Text(ConnectWords.searching)
                .bandEarFont(BandEarType.bodySmall)
                .foregroundStyle(BandEarColors.textSecondary)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(ConnectWords.searching)
    }
}

/// Why discovery cannot look for servers, in place of searching.
private struct DiscoveryProblem: View {
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: "wifi.exclamationmark")
                .foregroundStyle(BandEarColors.solid(.amber))
                .accessibilityHidden(true)
            Text(text)
                .bandEarFont(BandEarType.bodyMedium)
                .foregroundStyle(BandEarColors.text)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .screenCard()
        .accessibilityElement(children: .combine)
    }
}

/// One found server on its card: the whole card is its single Connect action.
private struct ServerCard: View {
    let desk: DiscoveredDesk
    let connect: () -> Void

    var body: some View {
        Button(action: connect) {
            HStack(spacing: 14) {
                Image(systemName: "server.rack")
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(BandEarColors.text)
                    .frame(width: 40, height: 40)
                    .background(BandEarColors.surfaceRaised, in: BandEarShapes.rounded(BandEarShapes.control))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(ConnectWords.serverCardTitle)
                        .bandEarFont(BandEarType.bodyLarge.with(size: 16, weight: .semibold))
                        .foregroundStyle(BandEarColors.text)
                    Text(ConnectWords.serverCardDetail(desk))
                        .bandEarFont(BandEarType.bodySmall)
                        .foregroundStyle(BandEarColors.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                // Drawn as the primary button, but the card is the one button.
                Text(ConnectWords.connect)
                    .bandEarFont(BandEarType.labelLarge)
                    .foregroundStyle(BandEarColors.onAccent)
                    .padding(.horizontal, 18)
                    .frame(minHeight: 44)
                    .background(BandEarColors.accent, in: BandEarShapes.rounded(BandEarShapes.control))
            }
            .padding(.vertical, 12)
            .padding(.leading, 16)
            .padding(.trailing, 12)
            .screenCard()
            .contentShape(BandEarShapes.rounded(BandEarShapes.panel))
        }
        .buttonStyle(PressedDimStyle())
    }
}

/// The empty list, saying what to check.
private struct NoServersYet: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(ConnectWords.noServersTitle)
                .bandEarFont(BandEarType.bodyLarge.with(size: 16, weight: .semibold))
                .foregroundStyle(BandEarColors.text)
            Text(ConnectWords.noServersDetail)
                .bandEarFont(BandEarType.bodySmall)
                .foregroundStyle(BandEarColors.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 14)
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .screenCard()
        .accessibilityElement(children: .combine)
    }
}

/// The row that opens the manual address form, between two rules.
private struct ManualAddressLink: View {
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 8) {
                Text(ConnectWords.manualAddress)
                    .bandEarFont(BandEarType.bodyLarge.with(weight: .medium))
                    .foregroundStyle(BandEarColors.text)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 8)
                Image(systemName: "chevron.right")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(BandEarColors.neutral)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, 4)
            .frame(minHeight: 52)
            .overlay(alignment: .top) { rule }
            .overlay(alignment: .bottom) { rule }
            .contentShape(Rectangle())
        }
        .buttonStyle(PressedDimStyle())
    }

    private var rule: some View {
        Rectangle()
            .fill(BandEarColors.border)
            .frame(height: 1)
    }
}
