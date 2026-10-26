import BandEarScreens
import BandEarSession
import SwiftUI

/// The diagnostics of Settings: the phone's own link health, the six readings that tell whether
/// the audio is healthy, and an expander with every counter and the recent errors.
///
/// Follows the Android client's `DiagnosticsCard` (`SettingsScreen.kt`) in the look of the
/// "Settings" boards: the six readings as a grid of tiles, their numbers in the number face. Every
/// word and number comes from ``Diagnostics`` and ``DiagnosticsWords``.
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
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                SectionCaption(text: DiagnosticsWords.title)
                Spacer(minLength: 8)
                LinkHealthLight(health: readings.linkHealth)
            }
            .frame(minHeight: 20)
            ReadingGrid(readings: readings.summary)
            Button {
                expanded.toggle()
            } label: {
                HStack(spacing: 8) {
                    Text(DiagnosticsWords.expanderText(expanded: expanded))
                        .bandEarFont(BandEarType.bodyLarge.with(weight: .medium))
                        .foregroundStyle(BandEarColors.text)
                        .multilineTextAlignment(.leading)
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.down")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(BandEarColors.textSecondary)
                        .rotationEffect(expanded ? .degrees(180) : .zero)
                        .accessibilityHidden(true)
                }
                .padding(.horizontal, 4)
                .frame(minHeight: 48)
                .contentShape(Rectangle())
            }
            .buttonStyle(PressedDimStyle())
            .accessibilityValue(DiagnosticsWords.expanderState(expanded: expanded))
            if expanded {
                AllCounters(
                    readings: readings.allReadings(controlStatus: controlStatus),
                    errors: readings.recentErrors)
            }
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

/// The compact readings two by two, as tiles of one card with a rule between them.
private struct ReadingGrid: View {
    let readings: [Diagnostics.Reading]

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.panel)
        let rows = stride(from: 0, to: readings.count, by: 2).map {
            Array(readings[$0..<min($0 + 2, readings.count)])
        }
        VStack(spacing: 1) {
            ForEach(rows, id: \.first?.label) { row in
                HStack(spacing: 1) {
                    ForEach(row, id: \.label) { reading in
                        ReadingTile(reading: reading)
                    }
                    if row.count < 2 {
                        BandEarColors.surface
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        .background(BandEarColors.border)
        .clipShape(shape)
        .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
    }
}

/// One labelled number, its digits in the number face.
private struct ReadingTile: View {
    let reading: Diagnostics.Reading
    @Environment(\.screenOrientation) private var orientation

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(reading.label)
                .bandEarFont(BandEarType.labelMedium)
                .foregroundStyle(BandEarColors.textSecondary)
            Text(reading.value)
                .bandEarFont(BandEarType.number.with(size: orientation.isLandscape ? 17 : 18))
                .foregroundStyle(BandEarColors.text)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
        }
        .padding(.vertical, orientation.isLandscape ? 12 : 14)
        .padding(.horizontal, orientation.isLandscape ? 14 : 16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(BandEarColors.surface)
        .accessibilityElement(children: .combine)
    }
}

/// Every counter and the recent errors, on a card. Each can be selected and copied, for a bug
/// report.
private struct AllCounters: View {
    let readings: [Diagnostics.Reading]
    let errors: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(readings, id: \.label) { reading in
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(reading.label)
                        .bandEarFont(BandEarType.bodySmall)
                        .foregroundStyle(BandEarColors.textSecondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text(reading.value)
                        .bandEarFont(BandEarType.numberSmall)
                        .foregroundStyle(BandEarColors.text)
                        .multilineTextAlignment(.trailing)
                }
                .textSelection(.enabled)
                .accessibilityElement(children: .combine)
            }
            SectionCaption(text: DiagnosticsWords.recentErrorsTitle)
                .padding(.top, 8)
            if errors.isEmpty {
                Text(DiagnosticsWords.noRecentErrors)
                    .bandEarFont(BandEarType.bodySmall)
                    .foregroundStyle(BandEarColors.text)
            } else {
                ForEach(errors, id: \.self) { error in
                    Text(error)
                        .bandEarFont(BandEarType.bodySmall)
                        .foregroundStyle(BandEarColors.text)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .screenCard()
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
                .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
                .foregroundStyle(BandEarColors.text)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(health.spokenDescription)
    }
}
