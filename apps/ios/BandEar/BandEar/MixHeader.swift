import BandEarScreens
import SwiftUI

/// The top of the Mix screen: the musician's name, the session's desk under it, and the connection
/// pill (Android's `MixHeader`), drawn as every screen draws a tone (``ConnectionToneStyle``).
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
            ConnectionPillBadge(pill: pill)
        }
    }
}
