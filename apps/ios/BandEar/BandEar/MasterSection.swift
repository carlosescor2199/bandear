import BandEarMix
import BandEarScreens
import SwiftUI

/// The Master card (Android's `MasterCard`): the master level and its Mute, the level cap that
/// protects the musician's ears, and the engineer's limit when one is set. The master, the cap and
/// the mute stay the musician's whatever the engineer set, so none of them is ever held.
struct MasterSection: View {
    let mix: MixState
    let apply: MixApply

    var body: some View {
        let muted = mix.masterMuted
        Section {
            HStack(spacing: 10) {
                Slider(
                    value: Binding(
                        get: { Double(mix.masterVolume) },
                        set: { apply(.masterVolumeChanged(level: Int($0.rounded()))) }),
                    in: 0...100, step: 1
                )
                .tint(muted ? .gray : .accentColor)
                .accessibilityLabel(MasterWords.faderDescription(muted: muted))
                .accessibilityValue(MasterWords.valueText(volume: mix.masterVolume, muted: muted))
                Text(MasterWords.valueText(volume: mix.masterVolume, muted: muted))
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(muted ? .secondary : .primary)
                    .frame(minWidth: 52, alignment: .trailing)
                    .accessibilityHidden(true)
                Toggle(
                    isOn: Binding(
                        get: { muted },
                        set: { apply(.masterMuteChanged(muted: $0)) })
                ) {
                    Text(MasterWords.muteText(muted: muted))
                        .font(.callout.weight(.semibold))
                        .frame(minWidth: 52)
                }
                .toggleStyle(.button)
                .buttonStyle(.bordered)
                .tint(muted ? .red : .gray)
                .accessibilityLabel(MasterWords.muteDescription)
            }
            .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 0) {
                    Text(MasterWords.levelCapLead)
                        .foregroundStyle(.secondary)
                    Text(MasterWords.levelCapValue(mix.levelCap))
                        .monospacedDigit()
                    Text(MasterWords.levelCapTrail)
                        .foregroundStyle(.secondary)
                }
                .font(.footnote)
                .accessibilityHidden(true)
                Slider(
                    value: Binding(
                        get: { Double(mix.levelCap) },
                        set: { apply(.levelCapChanged(level: Int($0.rounded()))) }),
                    in: 0...100, step: 1
                )
                .tint(.gray)
                .accessibilityLabel(MasterWords.levelCapDescription(for: mix))
                .accessibilityValue(MasterWords.levelCapValue(mix.levelCap))
            }
            .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
            // The engineer's limit sits apart from the phone's cap, which still works: the musician
            // hears the lower of the two, and the line says how much that is.
            if let limit = MasterWords.engineerLimitText(for: mix) {
                Label(limit, systemImage: StripSymbols.locked)
                    .font(.footnote.weight(.medium))
            }
        } header: {
            Text(MasterWords.title)
        }
    }
}
