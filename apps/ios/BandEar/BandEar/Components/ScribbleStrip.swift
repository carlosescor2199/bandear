import BandEarScreens
import SwiftUI

/// The colours of a ``ScribbleStrip``: its background and the text on it.
struct ScribbleStripTone: Equatable {
    let background: Color
    let foreground: Color

    /// A group's strip, on the group's tone (``GroupTone/forGroup(_:)``).
    static func group(_ tone: GroupTone) -> Self {
        Self(background: BandEarColors.groupBackground(tone), foreground: BandEarColors.groupText(tone))
    }

    /// The musician's own channel: the accent on its tint, whatever its group.
    static let mine = Self(background: BandEarColors.amberTint, foreground: BandEarColors.accent)

    /// A channel in no group, or a button such as the master's level cap: raised and plain.
    static let plain = Self(background: BandEarColors.surfaceRaised, foreground: BandEarColors.text)
}

/// The "scribble strip" at the foot of a console strip: the channel's name (with the stereo mark for
/// a pair) over a smaller line such as its group, on a tone that tells its group at a glance. It is a
/// button the whole strip wide and at least 40 pt tall; on a channel strip it opens the channel
/// panel.
///
/// What VoiceOver hears of the button is the caller's, so the strip keeps the words its label button
/// had (``ChannelSheetWords/settingsLabel(for:)``, ``ChannelSheetWords/panState(pan:stereo:)`` and
/// ``StripWords/openSettingsHint``); the drawn name and the stereo mark are not read on their own.
/// The smaller line is read after the button as an element of its own, as the strip's group chip or
/// "No group" was before the console: it is drawn over the button, outside it, and lets touches
/// through to it.
struct ScribbleStrip: View {
    let title: String
    let subtitle: String
    var stereo = false
    let tone: ScribbleStripTone
    let voiceOverLabel: String
    var voiceOverValue: String?
    var voiceOverHint: String?
    let action: @MainActor () -> Void

    /// Whether the button is held down, so the smaller line drawn over it dims with it.
    @State private var pressed = false

    /// The mark beside a stereo pair's name, as the strips drew it before.
    static let stereoSymbol = "hifispeaker.2"

    var body: some View {
        ZStack {
            Button {
                action()
            } label: {
                lines(drawing: .title)
                    .background(tone.background, in: BandEarShapes.rounded(BandEarShapes.key))
                    .contentShape(BandEarShapes.rounded(BandEarShapes.key))
            }
            .buttonStyle(PressReportingDimStyle(pressed: $pressed))
            .accessibilityLabel(voiceOverLabel)
            .accessibilityValue(voiceOverValue ?? "")
            .accessibilityHint(voiceOverHint ?? "")
            // Read before the smaller line, which VoiceOver would otherwise take first, as the one
            // drawn on top.
            .accessibilitySortPriority(1)
            // The same lines again, laid out alike so the smaller one lands where the button left
            // room for it, and dimmed while pressed as ``PressedDimStyle`` dims the button.
            lines(drawing: .subtitle)
                .opacity(pressed ? 0.7 : 1)
                .allowsHitTesting(false)
        }
        .accessibilityElement(children: .contain)
    }

    private enum Line {
        case title
        case subtitle
    }

    /// The name over the smaller line on the strip's tone, with only `drawing` drawn: the other
    /// keeps its room, hidden from sight and from VoiceOver.
    private func lines(drawing line: Line) -> some View {
        VStack(spacing: 2) {
            HStack(spacing: 4) {
                Text(title)
                    .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.75)
                if stereo {
                    Image(systemName: Self.stereoSymbol)
                        .imageScale(.small)
                }
            }
            .hidden(line != .title)
            Text(subtitle)
                .bandEarFont(BandEarType.labelSmall.with(size: 10, weight: .regular))
                .lineLimit(1)
                .minimumScaleFactor(0.75)
                .opacity(0.8)
                .hidden(line != .subtitle)
        }
        .foregroundStyle(tone.foreground)
        .padding(.horizontal, 4)
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity, minHeight: 40)
    }
}

/// ``PressedDimStyle``, which also tells the strip while it is pressed.
private struct PressReportingDimStyle: ButtonStyle {
    @Binding var pressed: Bool

    func makeBody(configuration: Configuration) -> some View {
        PressedDimStyle().makeBody(configuration: configuration)
            .onChange(of: configuration.isPressed) { pressed = $0 }
    }
}

extension View {
    /// When `hidden`, only the view's room: it is not drawn and VoiceOver does not read it.
    @ViewBuilder
    fileprivate func hidden(_ hidden: Bool) -> some View {
        if hidden {
            self.hidden()
        } else {
            self
        }
    }
}
