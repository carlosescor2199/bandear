import BandEarScreens
import SwiftUI

/// How a ``ConnectionTone`` is drawn: a system colour and an SF Symbol. Every sign also says its
/// state in words, so the colour never carries it alone.
enum ConnectionToneStyle {
    static func color(_ tone: ConnectionTone) -> Color {
        switch tone {
        case .green: .green
        case .amber: .orange
        case .red: .red
        case .blue: .blue
        case .neutral: .gray
        }
    }

    /// The symbol beside a banner of this tone.
    static func symbol(_ tone: ConnectionTone) -> String {
        switch tone {
        case .green: "checkmark.circle.fill"
        case .amber: "exclamationmark.triangle.fill"
        case .red: "exclamationmark.octagon.fill"
        case .blue: "info.circle.fill"
        case .neutral: "circle.fill"
        }
    }
}

/// The coloured dot of a tone. Hidden from VoiceOver: the words beside it say the same.
struct ConnectionToneDot: View {
    let tone: ConnectionTone

    var body: some View {
        Circle()
            .fill(ConnectionToneStyle.color(tone))
            .frame(width: 10, height: 10)
            .accessibilityHidden(true)
    }
}

/// A ``ConnectionBanner`` as a list row: its tone's symbol, its title and detail, and a spinner
/// while it says the connection is being retried.
struct ConnectionBannerRow: View {
    let banner: ConnectionBanner

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: ConnectionToneStyle.symbol(banner.tone))
                .foregroundStyle(ConnectionToneStyle.color(banner.tone))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(banner.title)
                    .font(.subheadline.weight(.semibold))
                Text(banner.detail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if banner.busy {
                Spacer(minLength: 8)
                ProgressView()
                    .accessibilityHidden(true)
            }
        }
        .accessibilityElement(children: .combine)
    }
}
