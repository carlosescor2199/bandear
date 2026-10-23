import BandEarScreens
import SwiftUI

/// The top of the Mix screen: the musician's name, the session's desk under it, and the connection
/// pill (Android's `MixHeader`).
struct MixHeaderRow: View {
    let name: String
    let server: String
    let pill: ConnectionPill

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.headline)
                    .lineLimit(1)
                Text(server)
                    .font(.subheadline.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 8)
            MixConnectionPill(pill: pill)
        }
    }
}

/// The connection's state in words with a dot of its tone, so it is never told by colour alone.
struct MixConnectionPill: View {
    let pill: ConnectionPill

    var body: some View {
        let color = MixTone.color(pill.tone)
        HStack(spacing: 6) {
            Circle()
                .fill(color)
                .frame(width: 8, height: 8)
            Text(pill.words)
                .font(.caption.weight(.semibold))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(color.opacity(0.15), in: Capsule())
        .overlay(Capsule().strokeBorder(color.opacity(0.4)))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(pill.spokenDescription)
    }
}

/// A banner under the header: a title and what it means, with a spinner while the connection
/// retries, tinted by its tone.
struct MixBannerRow: View {
    let banner: ConnectionBanner

    var body: some View {
        let color = MixTone.color(banner.tone)
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            if banner.busy {
                ProgressView()
            } else {
                Image(systemName: MixTone.symbol(banner.tone))
                    .foregroundStyle(color)
                    .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(banner.title)
                    .font(.subheadline.weight(.semibold))
                Text(banner.detail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .listRowBackground(color.opacity(0.15))
    }
}

/// The system colours the Mix screen draws the connection's tones with.
enum MixTone {
    static func color(_ tone: ConnectionTone) -> Color {
        switch tone {
        case .green: .green
        case .amber: .orange
        case .red: .red
        case .blue: .blue
        case .neutral: .gray
        }
    }

    /// The icon beside a banner that is not working on anything.
    static func symbol(_ tone: ConnectionTone) -> String {
        switch tone {
        case .red, .amber: "exclamationmark.triangle.fill"
        case .blue: "person.2.fill"
        case .green, .neutral: "info.circle.fill"
        }
    }
}
