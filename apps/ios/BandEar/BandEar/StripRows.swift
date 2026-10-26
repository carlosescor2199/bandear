import BandEarMix
import BandEarProtocol
import BandEarScreens
import SwiftUI

/// One channel strip (Android's `ChannelStrip`): its label, which opens the channel's sheet, what
/// stands where its group goes, its level with the value in words, and its Mute and Solo keys.
///
/// A strip the engineer locked, or any strip while the mix follows another musician's, says so in
/// its tag and takes no input; its label still opens the sheet.
struct ChannelStripRow: View {
    let strip: ChannelStrip
    let mix: MixState
    let moreOfMe: MoreOfMeState
    let apply: MixApply
    let open: () -> Void

    var body: some View {
        let channel = strip.index
        let held = mix.isStripLocked(channel) || mix.following != nil
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                StripLabelButton(strip: strip, open: open)
                Spacer(minLength: 8)
                StripTagView(tag: StripWords.tag(for: strip))
            }
            HStack(spacing: 10) {
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
                Text(StripWords.valueText(level: strip.level, muted: strip.isMuted))
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(strip.isMuted ? .secondary : .primary)
                    .frame(minWidth: 52, alignment: .trailing)
                    .accessibilityHidden(true)
                StripKey(
                    text: StripWords.muteLetter, on: strip.isMuted, onTint: .red,
                    description: StripWords.muteDescription(for: strip)
                ) {
                    apply(.channelMuteChanged(channel: channel, muted: $0))
                }
                .disabled(held)
                StripKey(
                    text: StripWords.soloLetter, on: strip.isSoloed, onTint: .accentColor,
                    description: StripWords.soloDescription(for: strip)
                ) {
                    apply(.channelSoloChanged(channel: channel, soloed: $0))
                }
                .disabled(held)
            }
        }
        .padding(.vertical, 4)
        .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
        .listRowBackground(
            StripWords.isMine(strip, moreOfMe: moreOfMe)
                ? Color.accentColor.opacity(0.1) : nil)
    }
}

/// One group fader (Android's `GroupStrip`): its chip, its level with the value, and the channels it
/// holds. While the mix follows another musician's, the level is the leader's and takes no input.
struct GroupStripRow: View {
    let group: GroupFader
    let apply: MixApply

    var body: some View {
        let index = group.index
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                StripGroupChip(name: group.name, tone: GroupTone.forGroup(index))
                Spacer(minLength: 8)
                if group.following != nil {
                    StripTagView(tag: .following)
                }
            }
            HStack(spacing: 10) {
                Slider(
                    value: Binding(
                        get: { Double(group.level) },
                        set: { apply(.groupLevelChanged(group: index, level: Int($0.rounded()))) }),
                    in: 0...100, step: 1
                )
                .accessibilityLabel(StripWords.groupFaderDescription(for: group))
                .accessibilityValue(StripWords.valueText(level: group.level, muted: false))
                .disabled(group.following != nil)
                Text(StripWords.valueText(level: group.level, muted: false))
                    .font(.callout.monospacedDigit())
                    .frame(minWidth: 52, alignment: .trailing)
                    .accessibilityHidden(true)
            }
            Text(StripWords.groupMembersText(group.members))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(3)
        }
        .padding(.vertical, 4)
        .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
    }
}

/// The channel's label as a button that opens its sheet: the icon of what it carries, its name,
/// and its pan written under it, after the stereo mark for a pair (Android's `StripLabelButton`).
private struct StripLabelButton: View {
    let strip: ChannelStrip
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            VStack(alignment: .leading, spacing: 2) {
                // The name takes the button's tint, as tappable text does; the rest stays plain grey.
                // Inside a button `.secondary` would be the tint's, so the colour is named.
                HStack(spacing: 5) {
                    if let kind = strip.kind {
                        StripKindIcon(kind: kind)
                            .font(.subheadline)
                            .foregroundStyle(Color.secondary)
                    }
                    Text(strip.label)
                        .font(.body.weight(.semibold))
                        .lineLimit(1)
                }
                HStack(spacing: 4) {
                    if strip.isStereo {
                        Image(systemName: StripSymbols.stereo)
                    }
                    Text(PanScale.text(strip.pan))
                        .monospacedDigit()
                }
                .font(.caption)
                .foregroundStyle(Color.secondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(ChannelSheetWords.settingsLabel(for: strip))
        .accessibilityValue(ChannelSheetWords.panState(pan: strip.pan, stereo: strip.isStereo))
        .accessibilityHint(StripWords.openSettingsHint)
    }
}

/// A strip's Mute or Solo key: filled in its tint while on, so its state shows apart from colour by
/// the fill, and the system says it is selected.
struct StripKey: View {
    let text: String
    let on: Bool
    let onTint: Color
    let description: String
    let toggle: @MainActor (Bool) -> Void

    var body: some View {
        Toggle(isOn: Binding(get: { on }, set: { toggle($0) })) {
            Text(text)
                .font(.callout.weight(.semibold))
                .frame(minWidth: 18)
        }
        .toggleStyle(.button)
        .buttonStyle(.bordered)
        .tint(on ? onTint : .gray)
        .accessibilityLabel(description)
    }
}

/// What a strip shows where its group goes: why it is held, in words with an icon, or its group's
/// chip, or "No group".
struct StripTagView: View {
    let tag: StripTag

    var body: some View {
        switch tag {
        case .locked:
            StripHeldTag(symbol: StripSymbols.locked, text: tag.text)
        case .following:
            StripHeldTag(symbol: StripSymbols.following, text: tag.text)
        case .group(let group):
            StripGroupChip(name: group.name, tone: GroupTone.forGroup(group.index))
        case .noGroup:
            Text(tag.text)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

/// Why a strip takes no input, as an icon and its words, never told by its fade alone.
private struct StripHeldTag: View {
    let symbol: String
    let text: String

    var body: some View {
        // Not a `Label`: inside a list row it takes the row's wide icon column.
        HStack(spacing: 3) {
            Image(systemName: symbol)
                .accessibilityHidden(true)
            Text(text)
        }
        .font(.caption.weight(.medium))
        .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .overlay(Capsule().strokeBorder(.secondary.opacity(0.5)))
    }
}

/// A group's name on a chip of its tone.
struct StripGroupChip: View {
    let name: String
    let tone: GroupTone

    var body: some View {
        let color = StripColors.group(tone)
        Text(name)
            .font(.caption.weight(.semibold))
            .lineLimit(1)
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
    }
}

/// The icon of what a channel carries. It adds a shape to find the channel by; its spoken name is
/// the kind's, and a strip's label already says the channel.
struct StripKindIcon: View {
    let kind: ChannelKind

    var body: some View {
        Image(systemName: StripSymbols.kind(kind))
            .accessibilityLabel(KindWords.label(kind))
    }
}

/// The system colours the strips draw their roles with.
enum StripColors {
    /// A strip's level fill: amber only while More of me lifts the musician's own strip.
    static func fill(_ fill: StripFill) -> Color {
        switch fill {
        case .accent: .orange
        case .idle: .accentColor
        case .muted: .gray
        }
    }

    static func group(_ tone: GroupTone) -> Color {
        switch tone {
        case .teal: .teal
        case .violet: .purple
        case .orange: .orange
        case .blue: .blue
        }
    }
}

/// The system symbols the strips and the channel sheet draw.
enum StripSymbols {
    static let stereo = "hifispeaker.2"
    static let locked = "lock.fill"
    static let following = "person.2.fill"

    static func kind(_ kind: ChannelKind) -> String {
        switch kind {
        case .voice: "music.mic"
        case .guitar: "guitars"
        case .bass: "guitars.fill"
        case .keys: "pianokeys"
        case .drums: "circle.circle"
        case .percussion: "hands.clap"
        case .horns: "megaphone"
        case .strings: "music.note"
        case .playback: "play.circle"
        case .click: "metronome"
        case .other: "waveform"
        }
    }
}
