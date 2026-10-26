import BandEarScreens
import SwiftUI

/// The top of the Mix screen (Android's `MixHeader`): the musician's name with the session's desk
/// under it, then `controls`, then the connection pill.
///
/// In portrait (`compact` false) the name is large and `controls` is usually empty; in landscape
/// the header is one line, and `controls` holds the Channels and Groups choice and More of me. The
/// name gives way first: it is cut short before any control is.
struct MixHeaderRow<Controls: View>: View {
    let name: String
    let server: String
    let pill: ConnectionPill
    var compact = false
    @ViewBuilder let controls: () -> Controls

    var body: some View {
        HStack(spacing: compact ? 10 : 12) {
            VStack(alignment: .leading, spacing: 0) {
                Text(name)
                    .bandEarFont(compact ? BandEarType.titleSmall : BandEarType.titleLarge)
                    .foregroundStyle(BandEarColors.text)
                Text(server)
                    .bandEarFont(compact ? BandEarType.labelMedium : BandEarType.bodySmall)
                    .foregroundStyle(BandEarColors.textSecondary)
            }
            .lineLimit(1)
            // Measured narrow, so a one-line header is chosen while the controls fit beside a name
            // cut short; laid out, it takes what the controls leave.
            .frame(minWidth: 0, idealWidth: compact ? 60 : nil, maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .combine)
            controls()
            ConnectionPillBadge(pill: pill)
                .fixedSize()
        }
    }
}

/// A ``ConnectionBanner`` as a card on its tone's tint, under the Mix screen's header.
struct MixBanner: View {
    let banner: ConnectionBanner

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.card)
        ConnectionBannerRow(banner: banner)
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(BandEarColors.tint(banner.tone), in: shape)
            .overlay(shape.strokeBorder(BandEarColors.tintBorder(banner.tone), lineWidth: 1))
    }
}
