import BandEarMix
import BandEarScreens
import SwiftUI

/// One channel's sheet, opened from its strip's label (Android's `ChannelSheet`): the pan, which
/// the strips have no room for, then the channel's level, Mute and Solo, so everything about the
/// channel is in one place.
///
/// It always shows `strip` as the mix holds it now, so a change made on the strip shows here at
/// once. A held channel's controls take no input, and its notes say why in words.
struct ChannelSheet: View {
    let strip: ChannelStrip
    let mix: MixState
    let moreOfMe: MoreOfMeState
    let apply: MixApply

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let channel = strip.index
        let following = mix.following
        let locked = mix.isStripLocked(channel)
        let held = locked || following != nil
        NavigationStack {
            List {
                Section {
                    ChannelSheetHeader(strip: strip, mine: StripWords.isMine(strip, moreOfMe: moreOfMe))
                    if let following {
                        Label(MixWords.followingTitle(following), systemImage: StripSymbols.following)
                            .font(.subheadline.weight(.medium))
                    }
                    if locked {
                        Label(ChannelSheetWords.lockedNote, systemImage: StripSymbols.locked)
                            .font(.subheadline.weight(.medium))
                    }
                }
                Section {
                    // A stereo pair's pan is its balance: the same control, written as both pans.
                    ChannelSheetValueRow(
                        title: ChannelSheetWords.panTitle(stereo: strip.isStereo),
                        value: PanScale.text(strip.pan), dim: false)
                    // Bound through the pan scale over 0...1, so a touch in the middle tenth reads
                    // as the centre and the thumb rests there.
                    Slider(
                        value: Binding(
                            get: { PanScale.position(of: strip.pan, travel: 1) },
                            set: {
                                apply(.channelPanChanged(channel: channel, pan: PanScale.value(at: $0, travel: 1)))
                            }),
                        in: 0...1
                    )
                    .accessibilityLabel(ChannelSheetWords.panControlDescription(for: strip))
                    .accessibilityValue(PanScale.spokenText(strip.pan))
                    .disabled(held)
                }
                Section {
                    ChannelSheetValueRow(
                        title: ChannelSheetWords.levelTitle,
                        value: StripWords.valueText(level: strip.level, muted: strip.isMuted),
                        dim: strip.isMuted)
                    Slider(
                        value: Binding(
                            get: { Double(strip.level) },
                            set: { apply(.channelLevelChanged(channel: channel, level: Int($0.rounded()))) }),
                        in: 0...100, step: 1
                    )
                    .tint(StripColors.fill(StripWords.fill(for: strip, moreOfMe: moreOfMe)))
                    .accessibilityLabel(StripWords.faderDescription(for: strip, moreOfMe: moreOfMe))
                    .accessibilityValue(StripWords.valueText(level: strip.level, muted: strip.isMuted))
                    .disabled(held)
                }
                Section {
                    HStack(spacing: 12) {
                        ChannelSheetKey(
                            text: ChannelSheetWords.muteKeyText(muted: strip.isMuted), on: strip.isMuted,
                            onTint: .red, description: StripWords.muteDescription(for: strip)
                        ) {
                            apply(.channelMuteChanged(channel: channel, muted: $0))
                        }
                        ChannelSheetKey(
                            text: ChannelSheetWords.soloKeyText(soloed: strip.isSoloed), on: strip.isSoloed,
                            onTint: .accentColor, description: StripWords.soloDescription(for: strip)
                        ) {
                            apply(.channelSoloChanged(channel: channel, soloed: $0))
                        }
                    }
                    .disabled(held)
                }
            }
            .navigationTitle(strip.label)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(ChannelSheetWords.done) { dismiss() }
                }
            }
        }
        // Fully open, as Android's sheet is (`skipPartiallyExpanded`): at half height the keys
        // would sit below the fold.
    }
}

/// What the channel carries, its group, whether it is a stereo pair (in words, with its mark) and
/// whether it is the musician's own. The channel's name is the sheet's title.
private struct ChannelSheetHeader: View {
    let strip: ChannelStrip
    let mine: Bool

    var body: some View {
        HStack(spacing: 10) {
            if let kind = strip.kind {
                StripKindIcon(kind: kind)
                    .font(.title3)
                    .foregroundStyle(.secondary)
            }
            if let group = strip.group {
                StripGroupChip(name: group.name, tone: GroupTone.forGroup(group.index))
            } else {
                Text(ChannelSheetWords.noGroup)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if strip.isStereo {
                Label(ChannelSheetWords.stereoTag, systemImage: StripSymbols.stereo)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            if mine {
                Text(ChannelSheetWords.yourChannel)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
            }
        }
    }
}

/// A control's caption and its value. The control under it tells a screen reader both, so the row
/// is hidden from it rather than read twice.
private struct ChannelSheetValueRow: View {
    let title: String
    let value: String
    let dim: Bool

    var body: some View {
        HStack {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.body.monospacedDigit())
                .foregroundStyle(dim ? .secondary : .primary)
        }
        .accessibilityHidden(true)
    }
}

/// A wide Mute or Solo key: filled and reading "Muted" or "Soloed" while on, not only coloured.
private struct ChannelSheetKey: View {
    let text: String
    let on: Bool
    let onTint: Color
    let description: String
    let toggle: @MainActor (Bool) -> Void

    var body: some View {
        Toggle(isOn: Binding(get: { on }, set: { toggle($0) })) {
            Text(text)
                .font(.body.weight(.semibold))
                .frame(maxWidth: .infinity)
        }
        .toggleStyle(.button)
        .buttonStyle(.bordered)
        .controlSize(.large)
        .tint(on ? onTint : .gray)
        .accessibilityLabel(description)
    }
}
