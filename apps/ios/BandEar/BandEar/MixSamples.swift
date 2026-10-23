#if DEBUG
    import BandEarDiscovery
    import BandEarMix
    import BandEarProtocol
    import BandEarScreens
    import BandEarSession
    import SwiftUI

    /// Sample mixes for the Mix screen's previews, built with the mix's own reducer from what a desk
    /// would announce, so they hold only states the app can reach.
    enum MixSamples {
        static let desk = DiscoveredDesk(
            name: "BandEar on studio-mac",
            endpoint: .bonjour(name: "BandEar on studio-mac", type: "_bandear._tcp", domain: "local."),
            sampleRate: 48_000)

        static let connected = ConnectionStatus(
            connection: .connected, sessionRunning: true, desk: desk.endpoint, mixHeldByDesk: true)

        static let reconnecting = ConnectionStatus(
            connection: .reconnecting, sessionRunning: true, desk: desk.endpoint, mixHeldByDesk: true)

        /// Eight channels: a named voice, a keys pair, an unnamed guitar, bass, two drums the
        /// engineer locked, and a hidden talkback; three groups; More of me lifting the voice; an
        /// engineer's limit of 85%.
        static let moreOfMeOn: MixSession = session(following: nil, moreOfMe: true)

        /// The same mix while the engineer has it follow Ana's.
        static let following: MixSession = session(following: "Ana", moreOfMe: false)

        /// A session whose desk has not announced its channels yet.
        static let waiting: MixSession = MixSession().reduced(by: .sessionStarted)

        private static func session(following: String?, moreOfMe: Bool) -> MixSession {
            let channels = [
                SourceChannel(name: "Lead vox", kind: .voice, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "Keys", kind: .keys, isHidden: false, isLinkedToNext: true),
                SourceChannel(name: "Keys R", kind: .keys, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "", kind: .guitar, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "Bass", kind: .bass, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "Kick", kind: .drums, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "Snare", kind: .drums, isHidden: false, isLinkedToNext: false),
                SourceChannel(name: "Talkback", kind: .other, isHidden: true, isLinkedToNext: false),
            ]
            let config = DeskConfiguration(
                sourceChannels: 8, sampleRate: 48_000,
                groups: [
                    ChannelGroup(id: "g1", name: "Vocals", channels: [0]),
                    ChannelGroup(id: "g2", name: "Band", channels: [1, 2, 3, 4]),
                    ChannelGroup(id: "g3", name: "Drums", channels: [5, 6]),
                ],
                channels: channels)
            let report = MixReport(
                sequence: nil, volumePercent: 80, maxLevelPercent: 90, muted: false,
                channelGains: [72, 60, 60, 45, 70, 55, 50, 100],
                pans: [50, 0, 100, 30, 50, 50, 50, 50],
                channelMutes: [false, false, false, true, false, false, false, false],
                channelSolos: [false, false, false, false, true, false, false, false],
                groupLevels: [90, 75, 60], engineerLimitPercent: 85,
                locked: [false, false, false, false, false, true, true, false],
                following: following)
            var events: [MixEvent] = [
                .sessionStarted, .configReceived(config), .mixPushed(report), .musicianChannelChosen(0),
            ]
            if moreOfMe { events.append(.moreOfMeToggled(on: true)) }
            return events.reduce(MixSession()) { $0.reduced(by: $1) }
        }
    }

    /// The Mix screen over a sample mix that takes its own edits through the mix's reducer, so the
    /// controls move in a preview as they do in the app.
    struct MixPreviewHost: View {
        @State var mixSession: MixSession
        var status = MixSamples.connected

        var body: some View {
            MixContent(
                musicianName: "Carlos", desk: MixSamples.desk, status: status,
                mixSession: mixSession, apply: { mixSession = mixSession.reduced(by: $0) })
        }
    }

    #Preview("Mix · More of me on, drums locked") {
        MixPreviewHost(mixSession: MixSamples.moreOfMeOn)
    }

    #Preview("Mix · following Ana's mix") {
        MixPreviewHost(mixSession: MixSamples.following)
    }

    #Preview("Mix · reconnecting") {
        MixPreviewHost(mixSession: MixSamples.moreOfMeOn, status: MixSamples.reconnecting)
    }

    #Preview("Mix · waiting for channels") {
        MixPreviewHost(mixSession: MixSamples.waiting)
    }
#endif
