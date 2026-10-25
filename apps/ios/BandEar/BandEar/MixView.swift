import BandEarDiscovery
import BandEarMix
import BandEarScreens
import SwiftUI

/// The Mix tab: the musician's mix while a session runs.
///
/// The container: it reads the session and hands its values to ``MixContent``, which lays them out,
/// and sends every edit back through ``SessionModel/apply(_:)``.
struct MixView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        MixContent(
            musicianName: session.musicianName,
            desk: session.desk,
            status: ConnectionStatus(
                connection: session.connection, sessionRunning: session.isRunning,
                desk: session.desk?.endpoint, mixHeldByDesk: session.mixHeldByDesk),
            mixSession: session.mixSession,
            apply: { session.apply($0) })
    }
}

/// An edit to the mix, as the Mix screen reports it.
typealias MixApply = @MainActor (MixEvent) -> Void

/// The Mix screen as a mixing console (Android's `MixScreen`, as the design's "Mix" boards draw
/// it): who and where with the connection, the banners, More of me with the musician's channel,
/// the choice of channels or groups, and the console of strips with the master pinned at the right.
/// A strip's scribble strip opens that channel's panel, and the master's level cap the master
/// panel.
///
/// Portrait stacks the header, More of me, the choice of row and its title over the console, whose
/// strips scroll sideways ("Swipe for more" says so while they overflow). Landscape puts the header,
/// the choice and More of me on one line over a console that shows every strip that fits. The
/// console fills the height the rest leaves; the screen itself does not scroll, unless that would
/// leave the console less than ``consoleLeastHeight``, as when banners stack in landscape.
///
/// The presentational half: it shows the values it is given and reports edits through `apply`,
/// with every word from `BandEarScreens` and every rule from the mix. Which row shows and which
/// panel is open are its own state, as on Android, held above the orientation switch so they
/// survive a rotation.
struct MixContent: View {
    let musicianName: String
    let desk: DiscoveredDesk?
    let status: ConnectionStatus
    let mixSession: MixSession
    let apply: MixApply

    /// The least height the console keeps for its strips: their keys, pan, value and scribble strip,
    /// and a fader of about 70 pt travel, long enough to set by hand. That is what one banner leaves
    /// it in landscape on a phone 402 pt tall, so a second banner scrolls the screen instead of
    /// taking the faders' travel (Android keeps 260 dp, `MinConsoleHeight` in `MixScreen.kt`).
    static let consoleLeastHeight: CGFloat = 240

    @State private var row: MixRow = .channels
    /// The channel whose panel is open, by the channel its strip is known by.
    @State private var openChannel: Int?
    @State private var masterPanelOpen = false

    var body: some View {
        let mix = mixSession.mix
        let sheetStrip = ChannelSheetWords.sheetStrip(in: mix.strips, open: openChannel)
        GeometryReader { proxy in
            let landscape = LayoutOrientation(proxy.size).isLandscape
            let padding: CGFloat = landscape ? 12 : 20
            let count = row == .channels ? mix.strips.count : mix.groupFaders.count
            let metrics = MixConsoleMetrics(
                width: proxy.size.width - 2 * padding, count: count, landscape: landscape)
            let title = MixWords.sectionTitle(row, count: count)
            let topPadding: CGFloat = landscape ? 10 : 8
            let bottomPadding: CGFloat = 12
            ScrollView(.vertical) {
                MixColumnLayout(
                    spaceHeight: proxy.size.height - topPadding - bottomPadding,
                    spacing: landscape ? 8 : 12, consoleLeastHeight: Self.consoleLeastHeight
                ) {
                    if landscape {
                        landscapeHeader
                    } else {
                        MixHeaderRow(name: headerName, server: headerServer, pill: status.pill) {
                            EmptyView()
                        }
                    }
                    banners(following: mix.following)
                    if !landscape {
                        HStack(spacing: 8) {
                            MoreOfMeSwitch(mixSession: mixSession, apply: apply)
                            MoreOfMeChannelPicker(mixSession: mixSession, apply: apply)
                                .fixedSize()
                        }
                    }
                    if let notice = mixSession.moreOfMe.notice {
                        MoreOfMeNoticeLine(notice: notice)
                    }
                    if !landscape {
                        rowChoice
                        rowTitle(title, overflows: metrics.overflows)
                    }
                    MixConsoleRow(
                        row: row, mixSession: mixSession, metrics: metrics,
                        title: landscape ? title : nil, apply: apply,
                        openChannel: { openChannel = $0 }, openMaster: { masterPanelOpen = true })
                }
                .padding(.horizontal, padding)
                .padding(.top, topPadding)
                .padding(.bottom, bottomPadding)
            }
            .bouncesOnlyWhenScrollable()
        }
        .background(BandEarColors.ground.ignoresSafeArea())
        .modalPanel(
            isPresented: Binding(
                get: { sheetStrip != nil },
                set: { if !$0 { openChannel = nil } }),
            doneTitle: ChannelSheetWords.done
        ) {
            if let sheetStrip {
                ChannelSheetTitle(strip: sheetStrip)
            }
        } content: {
            if let sheetStrip {
                ChannelSheet(strip: sheetStrip, mix: mix, moreOfMe: mixSession.moreOfMe, apply: apply)
            }
        }
        .modalPanel(isPresented: $masterPanelOpen, doneTitle: ChannelSheetWords.done) {
            MasterPanelTitle()
        } content: {
            MasterPanel(mix: mix, apply: apply)
        }
        // A channel the desk no longer announces has no panel; it does not reopen if it comes back.
        .onChange(of: sheetStrip == nil) { gone in
            if gone { openChannel = nil }
        }
        .moreOfMeAnnouncements(mixSession.moreOfMe.notice)
    }

    private var headerName: String {
        MixWords.headerName(musicianName)
    }

    private var headerServer: String {
        MixWords.headerServerText(desk)
    }

    /// The landscape header on one line, as the "Mix · landscape" board draws it: the name, the
    /// choice of row, More of me with the musician's channel, and the pill. Where they do not fit
    /// beside the name (a narrow phone, a long connection state), the controls drop to a second line.
    private var landscapeHeader: some View {
        ViewThatFits(in: .horizontal) {
            MixHeaderRow(name: headerName, server: headerServer, pill: status.pill, compact: true) {
                rowChoice
                MoreOfMeSwitch(mixSession: mixSession, apply: apply)
                    .fixedSize()
                MoreOfMeChannelPicker(mixSession: mixSession, apply: apply)
                    .fixedSize()
            }
            VStack(alignment: .leading, spacing: 4) {
                MixHeaderRow(name: headerName, server: headerServer, pill: status.pill, compact: true) {
                    EmptyView()
                }
                HStack(spacing: 10) {
                    MoreOfMeSwitch(mixSession: mixSession, apply: apply)
                        .fixedSize()
                    MoreOfMeChannelPicker(mixSession: mixSession, apply: apply)
                        .fixedSize()
                    Spacer(minLength: 0)
                    rowChoice
                }
            }
        }
    }

    /// The switch between channels and groups.
    private var rowChoice: some View {
        SegmentedChoice(options: MixRow.allCases, selection: $row) { $0.label }
            .fixedSize()
    }

    /// The small capitals over the portrait console, "CHANNELS · 8", and "Swipe for more" beside
    /// them while the strips run past the screen's edge.
    private func rowTitle(_ title: String, overflows: Bool) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .bandEarFont(BandEarType.caps)
                .foregroundStyle(BandEarColors.textSecondary)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 8)
            if overflows {
                Text(MixWords.swipeForMore)
                    .bandEarFont(BandEarType.labelMedium.with(weight: .regular))
                    .foregroundStyle(BandEarColors.textSecondary)
            }
        }
        .lineLimit(1)
    }

    /// The reconnecting or lost banner while the connection is away, and the following banner while
    /// the engineer has this mix follow another musician's.
    @ViewBuilder
    private func banners(following: String?) -> some View {
        if let connection = status.banner {
            MixBanner(banner: connection)
        }
        if let followed = ConnectionBanner.following(following) {
            MixBanner(banner: followed)
        }
    }
}

/// The Mix screen's column: the parts above the console one under the other, `spacing` apart and
/// as tall as they need, at the leading edge; then the console, the last part, the whole width and
/// as tall as they leave of `spaceHeight`, but never less than `consoleLeastHeight`
/// (``LayoutRules/consoleHeight(spaceHeight:topHeight:least:)``). The column is then taller than the
/// space, and the scroll view around it scrolls.
private struct MixColumnLayout: Layout {
    let spaceHeight: CGFloat
    let spacing: CGFloat
    let consoleLeastHeight: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.replacingUnspecifiedDimensions().width
        let heights = heights(width: width, subviews: subviews)
        return CGSize(width: width, height: heights.reduce(0, +) + gaps(subviews))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for (subview, height) in zip(subviews, heights(width: bounds.width, subviews: subviews)) {
            subview.place(
                at: CGPoint(x: bounds.minX, y: y), anchor: .topLeading,
                proposal: ProposedViewSize(width: bounds.width, height: height))
            y += height + spacing
        }
    }

    /// Each part's height: the parts above the console as tall as they would like at `width`, and
    /// the console what they leave.
    private func heights(width: CGFloat, subviews: Subviews) -> [CGFloat] {
        guard !subviews.isEmpty else { return [] }
        let top = subviews.dropLast().map {
            $0.sizeThatFits(ProposedViewSize(width: width, height: nil)).height
        }
        let console = LayoutRules.consoleHeight(
            spaceHeight: Double(spaceHeight), topHeight: Double(top.reduce(0, +) + gaps(subviews)),
            least: Double(consoleLeastHeight))
        return top + [CGFloat(console)]
    }

    private func gaps(_ subviews: Subviews) -> CGFloat {
        spacing * CGFloat(max(subviews.count - 1, 0))
    }
}

extension View {
    /// A scroll view that bounces only when what it holds is taller than it is, so a screen that
    /// fits does not move under a drag. iOS 16.0 to 16.3 have no such setting and bounce always.
    @ViewBuilder
    fileprivate func bouncesOnlyWhenScrollable() -> some View {
        if #available(iOS 16.4, *) {
            scrollBounceBehavior(.basedOnSize)
        } else {
            self
        }
    }
}
