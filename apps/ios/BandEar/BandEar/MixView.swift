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

/// The Mix screen as Android's `MixScreen` lays it out, rendered natively: who and where with the
/// connection, the banners, More of me with the musician's channel, the channels or the groups, and
/// the Master card. A strip's label opens that channel's sheet.
///
/// The presentational half: it shows the values it is given and reports edits through `apply`,
/// with every word from `BandEarScreens` and every rule from the mix. Which row shows and which
/// channel's sheet is open are its own state, as on Android.
struct MixContent: View {
    let musicianName: String
    let desk: DiscoveredDesk?
    let status: ConnectionStatus
    let mixSession: MixSession
    let apply: MixApply

    @State private var row: MixRow = .channels
    /// The channel whose sheet is open, by the channel its strip is known by.
    @State private var openChannel: Int?

    var body: some View {
        let mix = mixSession.mix
        let strips = mix.strips
        let sheetStrip = ChannelSheetWords.sheetStrip(in: strips, open: openChannel)
        List {
            Section {
                MixHeaderRow(
                    name: MixWords.headerName(musicianName),
                    server: MixWords.headerServerText(desk), pill: status.pill)
            }
            banners(following: mix.following)
            MoreOfMeSection(mixSession: mixSession, apply: apply)
            stripSection(mix: mix, strips: strips)
            MasterSection(mix: mix, apply: apply)
        }
        .sheet(
            isPresented: Binding(
                get: { sheetStrip != nil },
                set: { if !$0 { openChannel = nil } })
        ) {
            if let sheetStrip {
                ChannelSheet(
                    strip: sheetStrip, mix: mix, moreOfMe: mixSession.moreOfMe, apply: apply)
            }
        }
        // A channel the desk no longer announces has no sheet; it does not reopen if it comes back.
        .onChange(of: sheetStrip == nil) { gone in
            if gone { openChannel = nil }
        }
    }

    /// The reconnecting or lost banner while the connection is away, and the following banner while
    /// the engineer has this mix follow another musician's.
    @ViewBuilder
    private func banners(following: String?) -> some View {
        let connection = status.banner
        let followed = ConnectionBanner.following(following)
        if connection != nil || followed != nil {
            Section {
                if let connection { ConnectionBannerRow(banner: connection) }
                if let followed { ConnectionBannerRow(banner: followed) }
            }
        }
    }

    /// The switch between channels and groups, and the strips of the one shown.
    private func stripSection(mix: MixState, strips: [ChannelStrip]) -> some View {
        let groups = mix.groupFaders
        let count = row == .channels ? strips.count : groups.count
        return Section {
            Picker(selection: $row) {
                ForEach(MixRow.allCases, id: \.self) { row in
                    Text(row.label).tag(row)
                }
            } label: {
                EmptyView()
            }
            .pickerStyle(.segmented)
            switch row {
            case .channels:
                if strips.isEmpty {
                    MixEmptyRow(text: MixWords.waitingForChannels)
                } else {
                    ForEach(strips, id: \.index) { strip in
                        ChannelStripRow(
                            strip: strip, mix: mix, moreOfMe: mixSession.moreOfMe, apply: apply,
                            open: { openChannel = strip.index })
                    }
                }
            case .groups:
                if groups.isEmpty {
                    MixEmptyRow(text: MixWords.noGroups)
                } else {
                    ForEach(groups, id: \.index) { group in
                        GroupStripRow(group: group, apply: apply)
                    }
                }
            }
        } header: {
            Text(MixWords.sectionTitle(row, count: count))
        }
    }
}

/// Takes the row's place while it has nothing to show, and says why.
private struct MixEmptyRow: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, minHeight: 88)
            .multilineTextAlignment(.center)
    }
}
