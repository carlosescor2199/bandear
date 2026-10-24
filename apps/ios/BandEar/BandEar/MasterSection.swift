import BandEarMix
import BandEarScreens
import SwiftUI

/// The master strip pinned at the console's right (Android's `MasterCard`, as the design's "Mix"
/// boards draw it): "MASTER", its Mute key, the master fader with a marker at the engineer's limit
/// when one is set, the level in words, and a button showing the phone's level cap that opens the
/// master panel (``MasterPanel``), where the cap is set.
///
/// The master, the cap and the mute stay the musician's whatever the engineer set, so none of them
/// is ever held. It takes the width and the height its container gives it.
struct MasterStrip: View {
    let mix: MixState
    let apply: MixApply
    let openPanel: @MainActor () -> Void

    var body: some View {
        let muted = mix.masterMuted
        let value = MasterWords.valueText(volume: mix.masterVolume, muted: muted)
        let shape = BandEarShapes.rounded(BandEarShapes.panel)
        VStack(spacing: 0) {
            Text(MasterWords.title)
                .bandEarFont(BandEarType.caps.with(size: 11))
                .foregroundStyle(BandEarColors.textSecondary)
                .lineLimit(1)
                .frame(minHeight: 16)
                .padding(.top, 8)
                .accessibilityAddTraits(.isHeader)
            // The word key lays out 44 pt tall around its drawn 32 pt: 6 pt above and below it.
            ConsoleKey(
                text: MasterWords.muteText(muted: muted), kind: .mute, size: .word, isOn: muted,
                voiceOverLabel: MasterWords.muteDescription
            ) {
                apply(.masterMuteChanged(muted: $0))
            }
            VerticalFader(
                value: mix.masterVolume, fill: BandEarColors.textSecondary, muted: muted,
                limit: mix.engineer.isLimited ? mix.engineer.limitPercent : nil,
                voiceOverLabel: MasterWords.faderDescription(muted: muted), voiceOverValue: value
            ) {
                apply(.masterVolumeChanged(level: $0))
            }
            .padding(.top, 2)
            .padding(.bottom, 4)
            StripValueText(text: value, fill: muted ? .muted : .idle)
            LevelCapButton(mix: mix, action: openPanel)
                .padding(.top, 6)
        }
        .padding(.horizontal, 6)
        .padding(.bottom, 8)
        .frame(maxHeight: .infinity)
        .background(BandEarColors.surface, in: shape)
        .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
        .accessibilityElement(children: .contain)
    }
}

/// The foot of the master strip: the phone's level cap ("Level cap" over "90%"), with the lock in
/// the accent while the engineer's limit applies too. It opens the master panel; VoiceOver hears it
/// as the cap itself, by the cap control's own name and value.
private struct LevelCapButton: View {
    let mix: MixState
    let action: @MainActor () -> Void

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.key)
        Button {
            action()
        } label: {
            VStack(spacing: 2) {
                Text(MasterWords.levelCapLead.trimmingCharacters(in: .whitespaces))
                    .bandEarFont(BandEarType.labelSmall.with(size: 10, weight: .regular))
                    .foregroundStyle(BandEarColors.textSecondary)
                HStack(spacing: 3) {
                    if mix.engineer.isLimited {
                        Image(systemName: StripSymbols.locked)
                            .imageScale(.small)
                            .foregroundStyle(BandEarColors.accent)
                    }
                    Text(MasterWords.levelCapValue(mix.levelCap))
                        .bandEarFont(BandEarType.number.with(size: 13))
                        .foregroundStyle(BandEarColors.text)
                }
            }
            .lineLimit(1)
            .minimumScaleFactor(0.75)
            .padding(4)
            .frame(maxWidth: .infinity, minHeight: 40)
            .background(BandEarColors.surfaceRaised, in: shape)
            .contentShape(shape)
        }
        .buttonStyle(PressedDimStyle())
        .accessibilityLabel(MasterWords.levelCapDescription(for: mix))
        .accessibilityValue(MasterWords.levelCapValue(mix.levelCap))
    }
}
