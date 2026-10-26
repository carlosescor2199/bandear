import BandEarDiscovery
import BandEarScreens
import SwiftUI

/// The session's server with Disconnect, the musician's name, the manual connection form and the
/// diagnostics. With no session running it is the manual address form opened from Connect
/// (``ManualAddressView``), as the Android client's `SettingsScreen.kt` is both.
///
/// Laid out as the "Settings" boards: one column in portrait; in landscape the connection on the
/// left and the diagnostics on the right, level with the server card.
///
/// Every word comes from ``SettingsWords``, except the stopped output's line below.
///
/// It observes the session, which a receiver report leaves alone; the figures each report changes
/// are observed only by the parts that show them (``DiagnosticsModel``).
struct SettingsView: View {
    @ObservedObject var session: SessionModel
    /// Set when this is the manual address form: leads back to Connect's servers.
    var backToServers: (@MainActor () -> Void)?

    @Environment(\.screenOrientation) private var orientation
    @FocusState private var focusedField: Field?

    /// The text fields, so a tap anywhere on a field's row or box focuses it.
    private enum Field: Hashable {
        case name
        case host
        case audioPort
        case controlPort
    }

    var body: some View {
        let status = ConnectionStatus(
            connection: session.connection, sessionRunning: session.isRunning,
            desk: session.desk?.endpoint, mixHeldByDesk: session.mixHeldByDesk)
        let landscape = orientation.isLandscape
        // One layout or the other around the same two columns, so a focused field and the open
        // diagnostics stay as they are through a rotation.
        let layout =
            landscape
            ? AnyLayout(HStackLayout(alignment: .settingsColumns, spacing: 28))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        ScrollView {
            layout {
                VStack(alignment: .leading, spacing: landscape ? 22 : 28) {
                    VStack(alignment: .leading, spacing: 4) {
                        if let backToServers {
                            BackToServers(action: backToServers)
                        }
                        ScreenTitle(text: SettingsWords.title)
                    }
                    serverCard(status)
                        .alignmentGuide(.settingsColumns) { $0[.top] }
                    nameSection
                    manualConnectionSection(status)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                DiagnosticsSection(
                    session: session, diagnostics: session.diagnostics,
                    controlStatus: status.statusDetail
                )
                .frame(width: landscape ? 300 : nil)
                .alignmentGuide(.settingsColumns) { $0[.top] }
            }
            .padding(.horizontal, 20)
            .padding(.top, landscape ? 16 : 12)
            .padding(.bottom, 24)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(BandEarColors.ground.ignoresSafeArea())
        .navigationTitle(SettingsWords.title)
        .toolbar(.hidden, for: .navigationBar)
    }

    /// Where the phone is connected, in words beside the coloured dot, and the way out.
    private func serverCard(_ status: ConnectionStatus) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                ConnectionToneDot(tone: SettingsWords.serverCardTone(status))
                VStack(alignment: .leading, spacing: 2) {
                    Text(SettingsWords.serverCardTitle(status))
                        .bandEarFont(BandEarType.bodyLarge.with(size: 16, weight: .semibold))
                        .foregroundStyle(BandEarColors.text)
                    ServerCardDetail(text: SettingsWords.serverCardDetail(status))
                        .bandEarFont(BandEarType.bodySmall)
                        .foregroundStyle(BandEarColors.textSecondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
                if status.sessionRunning {
                    DangerButton(title: SettingsWords.disconnect) {
                        session.disconnect()
                    }
                }
            }
            OutputStoppedRow(
                diagnostics: session.diagnostics, sessionRunning: status.sessionRunning)
        }
        .padding(.vertical, 12)
        .padding(.leading, 16)
        .padding(.trailing, 12)
        .frame(minHeight: 68)
        .screenCard()
    }

    private var nameSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionCaption(text: SettingsWords.nameCaption)
            TextField(
                SettingsWords.nameLabel,
                text: Binding(get: { session.musicianName }, set: { session.setName($0) }),
                prompt: fieldPrompt(SettingsWords.nameLabel)
            )
            .textInputAutocapitalization(.words)
            .autocorrectionDisabled()
            .submitLabel(.done)
            .focused($focusedField, equals: .name)
            .boxedField { focusedField = .name }
        }
    }

    /// The address typed by hand. Its fields stay editable while a session runs, as on Android;
    /// only the button waits for a Disconnect.
    private func manualConnectionSection(_ status: ConnectionStatus) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionCaption(text: SettingsWords.manualConnectionTitle)
            VStack(spacing: 0) {
                fieldRow(.host, label: SettingsWords.serverAddressLabel) {
                    TextField(
                        SettingsWords.serverAddressLabel, text: $session.form.host,
                        prompt: fieldPrompt(SettingsWords.serverAddressPlaceholder)
                    )
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                }
                CardDivider()
                fieldRow(.audioPort, label: SettingsWords.audioPortLabel) {
                    TextField(
                        SettingsWords.audioPortLabel, text: $session.form.audioPort,
                        prompt: fieldPrompt(SettingsWords.audioPortLabel)
                    )
                    .keyboardType(.numberPad)
                }
                CardDivider()
                fieldRow(.controlPort, label: SettingsWords.controlPortLabel) {
                    TextField(
                        SettingsWords.controlPortLabel, text: $session.form.controlPort,
                        prompt: fieldPrompt(SettingsWords.controlPortLabel)
                    )
                    .keyboardType(.numberPad)
                }
            }
            .screenCard()
            if let banner = SettingsWords.manualProblemBanner(
                session.refusal, sessionRunning: status.sessionRunning)
            {
                ConnectionBannerCard(banner: banner)
            }
            PrimaryButton(title: SettingsWords.connectToThisAddress, fillsWidth: true) {
                session.connectManually()
            }
            .disabled(status.sessionRunning)
            if status.sessionRunning {
                SectionHint(text: SettingsWords.disconnectFirst)
            }
        }
    }

    /// One field of the manual address: its label, and the field with its number at the trailing
    /// side. VoiceOver hears the field by the same label, so the visible one is not read twice.
    private func fieldRow(_ field: Field, label: String, @ViewBuilder input: () -> some View)
        -> some View
    {
        HStack(spacing: 12) {
            Text(label)
                .bandEarFont(BandEarType.bodyLarge)
                .foregroundStyle(BandEarColors.text)
                .accessibilityHidden(true)
            input()
                .bandEarFont(BandEarType.number.with(size: 15, weight: .medium))
                .foregroundStyle(BandEarColors.text)
                .tint(BandEarColors.accent)
                .multilineTextAlignment(.trailing)
                .focused($focusedField, equals: field)
        }
        .padding(.horizontal, 16)
        .frame(minHeight: orientation.isLandscape ? 48 : 52)
        .contentShape(Rectangle())
        .onTapGesture { focusedField = field }
    }
}

extension VerticalAlignment {
    /// The line Settings' two landscape columns share: the top of the server card on the left, and
    /// of the diagnostics on the right.
    private enum SettingsColumns: AlignmentID {
        nonisolated static func defaultValue(in context: ViewDimensions) -> CGFloat {
            context[.top]
        }
    }

    fileprivate static let settingsColumns = VerticalAlignment(SettingsColumns.self)
}

/// The server card's line under its title (``SettingsWords/serverCardDetail(_:)``). While a session
/// runs it is where, then the connection's state after a " · ": where that does not fit on one line,
/// the place is cut short in its middle and the state stays whole, so it never wraps onto a line of
/// its own (the landscape card is narrow). VoiceOver reads the line in full. Otherwise it is advice,
/// which wraps.
private struct ServerCardDetail: View {
    let text: String

    var body: some View {
        if let state = text.range(of: " · ", options: .backwards) {
            HStack(spacing: 0) {
                Text(text[..<state.lowerBound])
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(text[state.lowerBound...])
                    .fixedSize()
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(text)
        } else {
            Text(text)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// The way back from the manual address form to Connect's servers, above the title (Android's
/// `BackToServers`).
private struct BackToServers: View {
    let action: @MainActor () -> Void

    var body: some View {
        Button {
            action()
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chevron.left")
                    .font(.system(size: 17, weight: .semibold))
                    .accessibilityHidden(true)
                Text(SettingsWords.backToServers)
                    .bandEarFont(BandEarType.bodyLarge.with(weight: .semibold))
            }
            .foregroundStyle(BandEarColors.accent)
            .padding(.trailing, 12)
            .frame(minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(PressedDimStyle())
    }
}

/// The line under the server card while the system holds the audio output stopped: packets keep
/// arriving then, so the card's "connected" alone would hide the silence. Nothing while the output
/// plays or no session runs.
///
/// A view of its own, so a receiver report rebuilds this line and not the whole of Settings.
private struct OutputStoppedRow: View {
    @ObservedObject var diagnostics: DiagnosticsModel
    let sessionRunning: Bool

    /// Verbatim from the provisional session screen, kept here until `BandEarScreens` holds it.
    private let outputStoppedLine =
        "The audio output stopped. It resumes when the system allows, or when you return to the app."

    var body: some View {
        if sessionRunning, !diagnostics.outputPlaying {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: "speaker.slash.fill")
                    .foregroundStyle(BandEarColors.solid(.amber))
                    .accessibilityHidden(true)
                Text(outputStoppedLine)
                    .bandEarFont(BandEarType.bodySmall)
                    .foregroundStyle(BandEarColors.text)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .combine)
        }
    }
}
