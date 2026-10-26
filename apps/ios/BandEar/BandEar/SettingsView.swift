import BandEarDiscovery
import BandEarScreens
import SwiftUI

/// The session's server with Disconnect, the musician's name, the manual connection form and the
/// diagnostics. With no session running it is the manual address form opened from Connect
/// (``ManualAddressView``), as the Android client's `SettingsScreen.kt` is both.
///
/// Every word comes from ``SettingsWords``, except the stopped output's line below.
///
/// It observes the session, which a receiver report leaves alone; the figures each report changes
/// are observed only by the parts that show them (``DiagnosticsModel``).
struct SettingsView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        let status = ConnectionStatus(
            connection: session.connection, sessionRunning: session.isRunning,
            desk: session.desk?.endpoint, mixHeldByDesk: session.mixHeldByDesk)
        Form {
            serverSection(status)
            nameSection
            manualConnectionSection(status)
            DiagnosticsSection(
                session: session, diagnostics: session.diagnostics,
                controlStatus: status.statusDetail)
        }
        .scrollDismissesKeyboard(.interactively)
        .navigationTitle(SettingsWords.title)
    }

    /// Where the phone is connected, in words beside the coloured dot, and the way out.
    private func serverSection(_ status: ConnectionStatus) -> some View {
        Section {
            HStack(spacing: 12) {
                ConnectionToneDot(tone: SettingsWords.serverCardTone(status))
                VStack(alignment: .leading, spacing: 2) {
                    Text(SettingsWords.serverCardTitle(status))
                        .font(.body.weight(.semibold))
                    Text(SettingsWords.serverCardDetail(status))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .accessibilityElement(children: .combine)
                if status.sessionRunning {
                    Spacer(minLength: 8)
                    Button(SettingsWords.disconnect, role: .destructive) {
                        session.disconnect()
                    }
                    .buttonStyle(.bordered)
                }
            }
            OutputStoppedRow(
                diagnostics: session.diagnostics, sessionRunning: status.sessionRunning)
        }
    }

    private var nameSection: some View {
        Section {
            TextField(
                SettingsWords.nameLabel,
                text: Binding(get: { session.musicianName }, set: { session.setName($0) })
            )
            .textInputAutocapitalization(.words)
            .autocorrectionDisabled()
            .submitLabel(.done)
        } header: {
            Text(SettingsWords.nameCaption)
                .textCase(nil)
        }
    }

    /// The address typed by hand. Its fields stay editable while a session runs, as on Android;
    /// only the button waits for a Disconnect.
    private func manualConnectionSection(_ status: ConnectionStatus) -> some View {
        Section {
            LabeledContent(SettingsWords.serverAddressLabel) {
                TextField(
                    SettingsWords.serverAddressLabel, text: $session.form.host,
                    prompt: Text(SettingsWords.serverAddressPlaceholder)
                )
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .multilineTextAlignment(.trailing)
            }
            LabeledContent(SettingsWords.audioPortLabel) {
                TextField(SettingsWords.audioPortLabel, text: $session.form.audioPort)
                    .keyboardType(.numberPad)
                    .multilineTextAlignment(.trailing)
            }
            LabeledContent(SettingsWords.controlPortLabel) {
                TextField(SettingsWords.controlPortLabel, text: $session.form.controlPort)
                    .keyboardType(.numberPad)
                    .multilineTextAlignment(.trailing)
            }
            if let banner = SettingsWords.manualProblemBanner(
                session.refusal, sessionRunning: status.sessionRunning)
            {
                ConnectionBannerRow(banner: banner)
            }
            Button(SettingsWords.connectToThisAddress) {
                session.connectManually()
            }
            .disabled(status.sessionRunning)
        } header: {
            Text(SettingsWords.manualConnectionTitle)
                .textCase(nil)
        } footer: {
            if status.sessionRunning {
                Text(SettingsWords.disconnectFirst)
            }
        }
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
            Label {
                Text(outputStoppedLine)
            } icon: {
                Image(systemName: "speaker.slash.fill")
                    .foregroundStyle(.orange)
            }
            .font(.subheadline)
        }
    }
}
