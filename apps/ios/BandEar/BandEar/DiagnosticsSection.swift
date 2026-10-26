import BandEarScreens
import BandEarSession
import SwiftUI

/// The diagnostics of Settings: the phone's own link health, the six readings that tell whether
/// the audio is healthy, and an expander with every counter and the recent errors.
///
/// Follows the Android client's `DiagnosticsCard` (`SettingsScreen.kt`); every word and number
/// comes from ``Diagnostics`` and ``DiagnosticsWords``.
struct DiagnosticsSection: View {
    /// The desk's reply and the connection's outage, which a receiver report leaves alone.
    @ObservedObject var session: SessionModel
    /// The receiver's figures, which change with almost every packet: each report rebuilds this
    /// section, and not the screens around it.
    @ObservedObject var diagnostics: DiagnosticsModel
    /// The control connection's state in words (``ConnectionStatus/statusDetail``), which leads
    /// the expanded counters.
    let controlStatus: String

    @State private var expanded = false

    var body: some View {
        let readings = self.readings
        Section {
            ForEach(readings.summary, id: \.label) { reading in
                ReadingRow(reading: reading)
            }
            Button {
                expanded.toggle()
            } label: {
                HStack {
                    Text(DiagnosticsWords.expanderText(expanded: expanded))
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.down")
                        .font(.footnote.weight(.semibold))
                        .rotationEffect(expanded ? .degrees(180) : .zero)
                        .accessibilityHidden(true)
                }
            }
            .accessibilityValue(DiagnosticsWords.expanderState(expanded: expanded))
            if expanded {
                // Every counter and the recent errors can be selected and copied, for a bug report.
                ForEach(readings.allReadings(controlStatus: controlStatus), id: \.label) { reading in
                    ReadingRow(reading: reading)
                        .textSelection(.enabled)
                }
                Text(DiagnosticsWords.recentErrorsTitle)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityAddTraits(.isHeader)
                let errors = readings.recentErrors
                if errors.isEmpty {
                    Text(DiagnosticsWords.noRecentErrors)
                        .font(.subheadline)
                } else {
                    ForEach(errors, id: \.self) { error in
                        Text(error)
                            .font(.subheadline)
                            .textSelection(.enabled)
                    }
                }
            }
        } header: {
            HStack {
                Text(DiagnosticsWords.title)
                Spacer(minLength: 8)
                LinkHealthLight(health: readings.linkHealth)
            }
            .textCase(nil)
        }
    }

    /// What the session holds, as the diagnostics read it. The link health is judged from the
    /// latest link report (``LinkHealth/init(phoneReport:)``), and is unknown without one.
    private var readings: Diagnostics {
        Diagnostics(
            stats: diagnostics.receiverStats, linkReport: diagnostics.linkReport,
            underruns: diagnostics.underrunTotal, receiverError: diagnostics.receiverError,
            deskError: session.deskError,
            // The outage's reason stays set on a connection the desk has not spoken on yet; while
            // connected it is no current error.
            connectionError: session.connection == .connected ? nil : session.connectionError)
    }
}

/// One labelled number, its digits aligned.
private struct ReadingRow: View {
    let reading: Diagnostics.Reading

    var body: some View {
        LabeledContent(reading.label) {
            Text(reading.value)
                .monospacedDigit()
        }
    }
}

/// The link-health light with its words, such as "Link good"; a screen reader hears the level
/// and its meaning instead.
private struct LinkHealthLight: View {
    let health: LinkHealth

    var body: some View {
        HStack(spacing: 6) {
            ConnectionToneDot(tone: health.tone)
            Text(health.summaryText)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(health.spokenDescription)
    }
}
