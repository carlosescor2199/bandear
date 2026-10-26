import BandEarMix
import BandEarScreens
import SwiftUI

/// The master panel, opened from the master strip's level cap button, in the same container as the
/// channel panel (``SwiftUI/View/modalPanel(isPresented:doneTitle:header:content:)``): the phone's
/// level cap, the line that says what it is ("Level cap 90% · protects your ears"), and the
/// engineer's limit when one is set.
///
/// The engineer's limit sits apart from the phone's cap, which still works: the musician hears the
/// lower of the two, and the limit's line says how much that is.
struct MasterPanel: View {
    let mix: MixState
    let apply: MixApply

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text(MasterWords.levelCapLead)
                    .foregroundStyle(BandEarColors.textSecondary)
                Text(MasterWords.levelCapValue(mix.levelCap))
                    .bandEarFont(BandEarType.number)
                    .foregroundStyle(BandEarColors.text)
                Text(MasterWords.levelCapTrail)
                    .foregroundStyle(BandEarColors.textSecondary)
            }
            .bandEarFont(BandEarType.bodySmall)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            // The slider says the cap and its value.
            .accessibilityHidden(true)
            PanelSlider(
                value: mix.levelCap, fill: BandEarColors.faderIdle,
                voiceOverLabel: MasterWords.levelCapDescription(for: mix),
                voiceOverValue: MasterWords.levelCapValue(mix.levelCap)
            ) {
                apply(.levelCapChanged(level: $0))
            }
            if let limit = MasterWords.engineerLimitText(for: mix) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: StripSymbols.locked)
                        .imageScale(.small)
                        .accessibilityHidden(true)
                    Text(limit)
                        .bandEarFont(BandEarType.bodySmall.with(weight: .medium))
                }
                .foregroundStyle(BandEarColors.accent)
                .padding(.top, 6)
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// The master panel's title, "MASTER" as the strip names it. VoiceOver hears it as the panel's
/// heading.
struct MasterPanelTitle: View {
    var body: some View {
        Text(MasterWords.title)
            .bandEarFont(BandEarType.titleMedium)
            .foregroundStyle(BandEarColors.text)
            .accessibilityAddTraits(.isHeader)
    }
}
