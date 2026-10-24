import BandEarMix
import BandEarScreens
import SwiftUI

/// How wide the console's strips are in a row `width` wide holding `count` strips, and whether they
/// run past its edge.
///
/// In portrait every strip is 72 pt wide, as the design's "Mix · portrait" board draws it, and the
/// row scrolls sideways when they do not fit. In landscape they share the row (80 to 120 pt each),
/// so all strips that fit show at once, and only a long row scrolls. The master strip is 88 pt
/// wide in portrait and 112 pt in landscape, pinned at the right in both.
struct MixConsoleMetrics: Equatable {
    static let stripGap: CGFloat = 6
    static let masterGap: CGFloat = 8

    let stripWidth: CGFloat
    let masterWidth: CGFloat
    /// True when the strips do not all fit beside the master, so the row scrolls.
    let overflows: Bool

    init(width: CGFloat, count: Int, landscape: Bool) {
        masterWidth = landscape ? 112 : 88
        let room = max(width - masterWidth - Self.masterGap, 0)
        let gaps = Self.stripGap * CGFloat(max(count - 1, 0))
        if landscape, count > 0 {
            stripWidth = min(max((room - gaps) / CGFloat(count), 80), 120)
        } else {
            stripWidth = 72
        }
        overflows = CGFloat(count) * stripWidth + gaps > room + 0.5
    }
}

/// The console: the channel or group strips side by side, scrolling sideways when they do not all
/// fit, and the master strip pinned at the right. While the row has nothing to show it says why in
/// the strips' place.
///
/// It keeps its place in the view tree in both orientations, so the row's scroll position survives
/// a rotation. `title` names the strips for VoiceOver where the screen shows no title over them (in
/// landscape); otherwise the visible title says it.
struct MixConsoleRow: View {
    let row: MixRow
    let mixSession: MixSession
    let metrics: MixConsoleMetrics
    let title: String?
    let apply: MixApply
    let openChannel: @MainActor (Int) -> Void
    let openMaster: @MainActor () -> Void

    var body: some View {
        let mix = mixSession.mix
        HStack(alignment: .top, spacing: MixConsoleMetrics.masterGap) {
            strips(mix: mix)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            MasterStrip(mix: mix, apply: apply, openPanel: openMaster)
                .frame(width: metrics.masterWidth)
        }
        .frame(maxHeight: .infinity)
    }

    @ViewBuilder
    private func strips(mix: MixState) -> some View {
        switch row {
        case .channels:
            let strips = mix.strips
            if strips.isEmpty {
                MixEmptyRow(text: MixWords.waitingForChannels)
            } else {
                scrolling {
                    ForEach(strips, id: \.index) { strip in
                        ConsoleChannelStrip(
                            strip: strip, mix: mix, moreOfMe: mixSession.moreOfMe, apply: apply,
                            open: { openChannel(strip.index) }
                        )
                        .frame(width: metrics.stripWidth)
                    }
                }
            }
        case .groups:
            let groups = mix.groupFaders
            if groups.isEmpty {
                MixEmptyRow(text: MixWords.noGroups)
            } else {
                scrolling {
                    ForEach(groups, id: \.index) { group in
                        ConsoleGroupStrip(group: group, apply: apply)
                            .frame(width: metrics.stripWidth)
                    }
                }
            }
        }
    }

    /// The strips in a row that scrolls sideways; a fader's drag up or down stays the fader's.
    private func scrolling<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: MixConsoleMetrics.stripGap) {
                content()
            }
            .frame(maxHeight: .infinity)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title ?? "")
    }
}

/// Takes the strips' place while the row has nothing to show, and says why.
private struct MixEmptyRow: View {
    let text: String

    var body: some View {
        Text(text)
            .bandEarFont(BandEarType.bodyMedium)
            .foregroundStyle(BandEarColors.textSecondary)
            .multilineTextAlignment(.center)
            .padding(16)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(BandEarColors.surface, in: BandEarShapes.rounded(BandEarShapes.panel))
    }
}
