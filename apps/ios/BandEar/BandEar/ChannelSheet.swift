import BandEarMix
import BandEarScreens
import SwiftUI

/// One channel's panel, opened from its strip's scribble strip (Android's `ChannelSheet`, as the
/// design's "Channel panel" boards draw it): what the channel is, why it is held, its pan, its
/// level, and wide Mute and Solo keys, so everything about the channel is in one place.
///
/// The content of a ``SwiftUI/View/modalPanel(isPresented:doneTitle:header:content:)``: a bottom
/// sheet in portrait and a panel from the trailing edge in landscape, under ``ChannelSheetTitle``
/// and the Done button. It always shows `strip` as the mix holds it now, so a change made on the
/// strip shows here at once. A held channel's controls take no input, and its notes say why in
/// words.
struct ChannelSheet: View {
    let strip: ChannelStrip
    let mix: MixState
    let moreOfMe: MoreOfMeState
    let apply: MixApply

    var body: some View {
        let channel = strip.index
        let following = mix.following
        let locked = mix.isStripLocked(channel)
        let held = locked || following != nil
        VStack(alignment: .leading, spacing: 16) {
            ChannelSheetTags(strip: strip, mine: StripWords.isMine(strip, moreOfMe: moreOfMe))
            if following != nil || locked {
                VStack(alignment: .leading, spacing: 6) {
                    if let following {
                        ChannelSheetNote(symbol: StripSymbols.following, text: MixWords.followingTitle(following))
                    }
                    if locked {
                        ChannelSheetNote(symbol: StripSymbols.locked, text: ChannelSheetWords.lockedNote)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 0) {
                // A stereo pair's pan is its balance: the same control, written as both pans.
                ChannelSheetValueRow(
                    title: ChannelSheetWords.panTitle(stereo: strip.isStereo),
                    value: PanScale.text(strip.pan), dim: false)
                // On the pan scale, so a touch in the middle tenth reads as the centre.
                PanelSlider(
                    value: strip.pan, scale: .pan, fill: nil,
                    voiceOverLabel: ChannelSheetWords.panControlDescription(for: strip),
                    voiceOverValue: PanScale.spokenText(strip.pan)
                ) {
                    apply(.channelPanChanged(channel: channel, pan: $0))
                }
                .disabled(held)
            }
            VStack(alignment: .leading, spacing: 0) {
                ChannelSheetValueRow(
                    title: ChannelSheetWords.levelTitle,
                    value: StripWords.valueText(level: strip.level, muted: strip.isMuted),
                    dim: strip.isMuted)
                PanelSlider(
                    value: strip.level,
                    fill: BandEarColors.faderFill(StripWords.fill(for: strip, moreOfMe: moreOfMe)),
                    voiceOverLabel: StripWords.faderDescription(for: strip, moreOfMe: moreOfMe),
                    voiceOverValue: StripWords.valueText(level: strip.level, muted: strip.isMuted)
                ) {
                    apply(.channelLevelChanged(channel: channel, level: $0))
                }
                .disabled(held)
            }
            HStack(spacing: 10) {
                ChannelSheetKey(
                    text: ChannelSheetWords.muteKeyText(muted: strip.isMuted), kind: .mute,
                    isOn: strip.isMuted, voiceOverLabel: StripWords.muteDescription(for: strip)
                ) {
                    apply(.channelMuteChanged(channel: channel, muted: $0))
                }
                ChannelSheetKey(
                    text: ChannelSheetWords.soloKeyText(soloed: strip.isSoloed), kind: .solo,
                    isOn: strip.isSoloed, voiceOverLabel: StripWords.soloDescription(for: strip)
                ) {
                    apply(.channelSoloChanged(channel: channel, soloed: $0))
                }
            }
            .disabled(held)
        }
    }
}

/// The channel panel's title: the channel's name, with the stereo mark for a pair (the tags under it
/// say "Stereo pair" in words, so the mark is not read). VoiceOver hears it as the panel's heading.
struct ChannelSheetTitle: View {
    let strip: ChannelStrip

    var body: some View {
        HStack(spacing: 8) {
            Text(strip.label)
                .bandEarFont(BandEarType.titleMedium)
                .lineLimit(1)
                .minimumScaleFactor(0.75)
            if strip.isStereo {
                Image(systemName: StripSymbols.stereo)
                    .foregroundStyle(BandEarColors.textSecondary)
                    .accessibilityHidden(true)
            }
        }
        .foregroundStyle(BandEarColors.text)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// What the channel carries, its group, whether it is a stereo pair and whether it is the
/// musician's own, as small tags.
private struct ChannelSheetTags: View {
    let strip: ChannelStrip
    let mine: Bool

    var body: some View {
        HStack(spacing: 6) {
            if let kind = strip.kind {
                StripKindIcon(kind: kind)
                    .font(.body)
                    .foregroundStyle(BandEarColors.textSecondary)
                    .padding(.trailing, 2)
            }
            if let group = strip.group {
                let tone = GroupTone.forGroup(group.index)
                ChannelSheetTag(
                    text: group.name, background: BandEarColors.groupBackground(tone),
                    foreground: BandEarColors.groupText(tone))
            } else {
                ChannelSheetTag(
                    text: ChannelSheetWords.noGroup, background: BandEarColors.surfaceRaised,
                    foreground: BandEarColors.textSecondary)
            }
            if strip.isStereo {
                ChannelSheetTag(
                    text: ChannelSheetWords.stereoTag, background: BandEarColors.surfaceRaised,
                    foreground: BandEarColors.textSecondary)
            }
            if mine {
                ChannelSheetTag(
                    text: ChannelSheetWords.yourChannel, background: BandEarColors.amberTint,
                    foreground: BandEarColors.accent)
            }
        }
    }
}

/// One small tag of the channel panel, with the tag radius.
private struct ChannelSheetTag: View {
    let text: String
    let background: Color
    let foreground: Color

    var body: some View {
        Text(text)
            .bandEarFont(BandEarType.labelMedium)
            .foregroundStyle(foreground)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(background, in: BandEarShapes.rounded(BandEarShapes.tag))
    }
}

/// Why a held channel's controls take no input, in words beside the symbol of the reason.
private struct ChannelSheetNote: View {
    let symbol: String
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: symbol)
                .imageScale(.small)
                .foregroundStyle(BandEarColors.textSecondary)
                .accessibilityHidden(true)
            Text(text)
                .bandEarFont(BandEarType.bodySmall.with(weight: .medium))
                .foregroundStyle(BandEarColors.text)
        }
        .accessibilityElement(children: .combine)
    }
}

/// A control's caption and its value. The control under it tells a screen reader both, so the row
/// is hidden from it rather than read twice.
private struct ChannelSheetValueRow: View {
    let title: String
    let value: String
    let dim: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .bandEarFont(BandEarType.caps)
                .foregroundStyle(BandEarColors.textSecondary)
            Spacer()
            Text(value)
                .bandEarFont(BandEarType.number)
                .foregroundStyle(dim ? BandEarColors.textSecondary : BandEarColors.text)
        }
        .accessibilityHidden(true)
    }
}

/// A wide Mute or Solo key, 48 pt tall: raised while off; filled while on, red for mute and the
/// accent for solo, and reading "Muted" or "Soloed", so its state is not told by colour alone.
/// VoiceOver hears it as the strip's key, a button named `voiceOverLabel` that says it is selected
/// while on. Inside a disabled view it fades to 40%.
private struct ChannelSheetKey: View {
    let text: String
    let kind: ConsoleKey.Kind
    let isOn: Bool
    let voiceOverLabel: String
    let onToggle: @MainActor (Bool) -> Void

    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.control)
        let onColour = kind == .mute ? BandEarColors.red : BandEarColors.accent
        Button {
            onToggle(!isOn)
        } label: {
            Text(text)
                .bandEarFont(BandEarType.labelLarge)
                .foregroundStyle(isOn ? BandEarColors.onAccent : BandEarColors.text)
                .lineLimit(1)
                .frame(maxWidth: .infinity, minHeight: 48)
                .background(isOn ? onColour : BandEarColors.surfaceRaised, in: shape)
                .overlay(shape.strokeBorder(isOn ? onColour : BandEarColors.border, lineWidth: 1))
                .contentShape(shape)
        }
        .buttonStyle(PressedDimStyle())
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityRepresentation {
            Toggle(isOn: Binding(get: { isOn }, set: { onToggle($0) })) {
                Text(voiceOverLabel)
            }
            .toggleStyle(.button)
        }
    }
}
