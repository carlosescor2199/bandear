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
/// What VoiceOver hears is the caller's, so the strip keeps the words its label button had
/// (``ChannelSheetWords/settingsLabel(for:)``, ``ChannelSheetWords/panState(pan:stereo:)`` and
/// ``StripWords/openSettingsHint``); the drawn words and the stereo mark are not read on their own.
struct ScribbleStrip: View {
    let title: String
    let subtitle: String
    var stereo = false
    let tone: ScribbleStripTone
    let voiceOverLabel: String
    var voiceOverValue: String?
    var voiceOverHint: String?
    let action: @MainActor () -> Void

    /// The mark beside a stereo pair's name, as the strips drew it before.
    static let stereoSymbol = "hifispeaker.2"

    var body: some View {
        Button {
            action()
        } label: {
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
                Text(subtitle)
                    .bandEarFont(BandEarType.labelSmall.with(size: 10, weight: .regular))
                    .lineLimit(1)
                    .minimumScaleFactor(0.75)
                    .opacity(0.8)
            }
            .foregroundStyle(tone.foreground)
            .padding(.horizontal, 4)
            .padding(.vertical, 4)
            .frame(maxWidth: .infinity, minHeight: 40)
            .background(tone.background, in: BandEarShapes.rounded(BandEarShapes.key))
            .contentShape(BandEarShapes.rounded(BandEarShapes.key))
        }
        .buttonStyle(PressedDimStyle())
        .accessibilityLabel(voiceOverLabel)
        .accessibilityValue(voiceOverValue ?? "")
        .accessibilityHint(voiceOverHint ?? "")
    }
}
