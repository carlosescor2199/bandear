import BandEarMix
import BandEarProtocol
import BandEarScreens
import SwiftUI

/// One channel strip of the console (Android's `ChannelStrip`, as the design's "Mix" boards draw
/// it), top to bottom: its Mute and Solo keys, or why it is held in their place; its pan; its
/// fader; its level in words; and its scribble strip, which opens the channel's panel.
///
/// A strip the engineer locked, or any strip while the mix follows another musician's, says so in
/// its tag, and its pan and fader are dimmed and take no input; its scribble strip still opens the
/// panel, whose notes say why. The musician's own strip is raised and edged in the accent's tint.
///
/// It takes the width and the height its container gives it.
struct ConsoleChannelStrip: View {
    let strip: ChannelStrip
    let mix: MixState
    let moreOfMe: MoreOfMeState
    let apply: MixApply
    let open: @MainActor () -> Void

    var body: some View {
        let channel = strip.index
        let held = mix.isStripLocked(channel) || mix.following != nil
        let mine = StripWords.isMine(strip, moreOfMe: moreOfMe)
        let fill = StripWords.fill(for: strip, moreOfMe: moreOfMe)
        let value = StripWords.valueText(level: strip.level, muted: strip.isMuted)
        StripShell(highlighted: mine) {
            // The keys and the tag lay out 44 pt tall, the touch target, with the drawn 28 pt in the
            // middle: the 8 pt above is the strip's padding, the 8 pt below the gap to the pan.
            if held, let reason = HeldTag.Reason(StripWords.tag(for: strip)) {
                HeldTag(reason: reason)
                    .padding(.vertical, 8)
            } else {
                HStack(spacing: 4) {
                    ConsoleKey(
                        text: StripWords.muteLetter, kind: .mute, isOn: strip.isMuted,
                        voiceOverLabel: StripWords.muteDescription(for: strip)
                    ) {
                        apply(.channelMuteChanged(channel: channel, muted: $0))
                    }
                    ConsoleKey(
                        text: StripWords.soloLetter, kind: .solo, isOn: strip.isSoloed,
                        voiceOverLabel: StripWords.soloDescription(for: strip)
                    ) {
                        apply(.channelSoloChanged(channel: channel, soloed: $0))
                    }
                }
                .disabled(held)
            }
            Group {
                StripPan(
                    pan: strip.pan, mine: mine,
                    voiceOverLabel: ChannelSheetWords.panControlDescription(for: strip),
                    voiceOverValue: PanScale.spokenText(strip.pan)
                ) {
                    apply(.channelPanChanged(channel: channel, pan: $0))
                }
                VerticalFader(
                    value: strip.level, fill: BandEarColors.faderFill(fill), muted: strip.isMuted,
                    voiceOverLabel: StripWords.faderDescription(for: strip, moreOfMe: moreOfMe),
                    voiceOverValue: value
                ) {
                    apply(.channelLevelChanged(channel: channel, level: $0))
                }
                .padding(.top, 8)
                .padding(.bottom, 4)
            }
            .disabled(held)
            StripValueText(text: value, fill: fill)
                .opacity(held ? 0.4 : 1)
            ScribbleStrip(
                title: strip.label, subtitle: strip.group?.name ?? StripWords.noGroup,
                stereo: strip.isStereo, tone: Self.tone(of: strip, mine: mine),
                voiceOverLabel: ChannelSheetWords.settingsLabel(for: strip),
                voiceOverValue: ChannelSheetWords.panState(pan: strip.pan, stereo: strip.isStereo),
                voiceOverHint: StripWords.openSettingsHint, action: open
            )
            .padding(.top, 6)
        }
    }

    /// The scribble strip's tone: the accent's on the musician's own channel, otherwise its group's,
    /// or plain in no group (`groupToneFor`).
    private static func tone(of strip: ChannelStrip, mine: Bool) -> ScribbleStripTone {
        if mine { return .mine }
        return strip.group.map { .group(GroupTone.forGroup($0.index)) } ?? .plain
    }
}

/// One group strip (Android's `GroupStrip`), in the channel strip's shell with its level only:
/// groups have no mute, solo or pan. Its fader, its level in words, and its name over the channels
/// it holds on the group's tone. While the mix follows another musician's, the level is the
/// leader's: the strip says "Following" at its top and its fader takes no input.
struct ConsoleGroupStrip: View {
    let group: GroupFader
    let apply: MixApply

    var body: some View {
        let index = group.index
        let following = group.following != nil
        let value = StripWords.valueText(level: group.level, muted: false)
        StripShell(highlighted: false) {
            if following {
                HeldTag(reason: .following)
                    .padding(.vertical, 8)
            }
            VerticalFader(
                value: group.level, fill: BandEarColors.faderFill(.idle),
                voiceOverLabel: StripWords.groupFaderDescription(for: group), voiceOverValue: value
            ) {
                apply(.groupLevelChanged(group: index, level: $0))
            }
            .padding(.top, following ? 0 : 8)
            .padding(.bottom, 4)
            .disabled(following)
            StripValueText(text: value, fill: .idle)
                .opacity(following ? 0.4 : 1)
            GroupScribble(
                name: group.name, members: StripWords.groupMembersText(group.members),
                tone: .group(GroupTone.forGroup(index))
            )
            .padding(.top, 6)
        }
    }
}

/// The body of a console strip: a column on the strip's surface with the panel radius, raised and
/// edged in the accent's tint when it is the musician's own. The content starts at the top edge,
/// since the first row's touch target takes the strip's top padding; the sides and the foot keep
/// 6 and 8 pt.
struct StripShell<Content: View>: View {
    let highlighted: Bool
    @ViewBuilder let content: () -> Content

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.panel)
        VStack(spacing: 0) {
            content()
        }
        .padding(.horizontal, 6)
        .padding(.bottom, 8)
        .frame(maxHeight: .infinity)
        .background(highlighted ? BandEarColors.surfaceSelected : BandEarColors.surface, in: shape)
        .overlay(shape.strokeBorder(highlighted ? BandEarColors.amberTintBorder : .clear, lineWidth: 1))
        .accessibilityElement(children: .contain)
    }
}

/// A strip's level under its fader, in mono: the accent while More of me lifts it, red when muted
/// (its words say "muted" as well), the text colour otherwise. VoiceOver hears it from the fader.
struct StripValueText: View {
    let text: String
    let fill: StripFill

    var body: some View {
        Text(text)
            .bandEarFont(BandEarType.number.with(size: 13))
            .foregroundStyle(Self.colour(fill))
            .lineLimit(1)
            .minimumScaleFactor(0.7)
            .frame(maxWidth: .infinity, minHeight: 18)
            .accessibilityHidden(true)
    }

    private static func colour(_ fill: StripFill) -> Color {
        switch fill {
        case .accent: BandEarColors.accent
        case .muted: BandEarColors.red
        case .idle: BandEarColors.text
        }
    }
}

/// A group strip's foot: the group's name over the channels it holds, on the group's tone, drawn as
/// a ``ScribbleStrip`` is but not a button, since a group has no panel. VoiceOver reads the two
/// lines as one, in full even when a long list of channels is cut short on screen.
private struct GroupScribble: View {
    let name: String
    let members: String
    let tone: ScribbleStripTone

    var body: some View {
        VStack(spacing: 2) {
            Text(name)
                .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
            Text(members)
                .bandEarFont(BandEarType.labelSmall.with(size: 10, weight: .regular))
                .opacity(0.8)
        }
        .lineLimit(1)
        .minimumScaleFactor(0.75)
        .foregroundStyle(tone.foreground)
        .padding(4)
        .frame(maxWidth: .infinity, minHeight: 40)
        .background(tone.background, in: BandEarShapes.rounded(BandEarShapes.key))
        .accessibilityElement(children: .combine)
    }
}

/// The icon of what a channel carries. It adds a shape to find the channel by; its spoken name is
/// the kind's.
struct StripKindIcon: View {
    let kind: ChannelKind

    var body: some View {
        Image(systemName: StripSymbols.kind(kind))
            .accessibilityLabel(KindWords.label(kind))
    }
}

/// The system symbols the strips and the panels draw.
enum StripSymbols {
    static let stereo = ScribbleStrip.stereoSymbol
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
