import BandEarScreens
import SwiftUI

/// A state in words beside a dot of its tone, in a capsule tinted by that tone (Android's
/// `StatusPill`): the connection pill at the top of the Mix screen. The words carry the state, so
/// it is never told by colour alone; VoiceOver hears `voiceOverLabel` instead, such as
/// ``ConnectionPill/spokenDescription``.
///
/// 36 pt tall, as the design's header controls are; it is not a control.
struct StatusPill: View {
    let tone: ConnectionTone
    let text: String
    let voiceOverLabel: String

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(BandEarColors.solid(tone))
                .frame(width: 8, height: 8)
            Text(text)
                .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
                .foregroundStyle(BandEarColors.text)
                .lineLimit(1)
        }
        .padding(.horizontal, 12)
        .frame(minHeight: 36)
        .background(BandEarColors.tint(tone), in: Capsule())
        .overlay(Capsule().strokeBorder(BandEarColors.tintBorder(tone), lineWidth: 1))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(voiceOverLabel)
    }
}
