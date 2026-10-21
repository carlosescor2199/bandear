import Foundation
import Testing

@testable import BandEarProtocol

private func decode(_ json: String) throws -> ServerMessage {
    try ServerMessage(decodingJSON: Data(json.utf8))
}

/// A whole `mix_ack` with two channels and one group, as the desk writes it
/// (`apps/server/src/protocol.rs:382-396`).
private let mixAckJSON = """
    {"type":"mix_ack","seq":7,"volume_percent":80,"max_level_percent":95,"muted":false,
     "channels":[10,20],"pans":[0,100],"mutes":[false,true],"solos":[true,false],
     "group_levels":[30],"engineer_limit_percent":90,"locked":[false,true],"following":null}
    """

@Suite("Desk control messages")
struct ServerMessageTests {

    // MARK: config

    @Test("reads the whole config the desk sends on connect")
    func decodesConfig() throws {
        let json = """
            {"type":"config","source_channels":2,"sample_rate":48000,
             "groups":[{"id":"g1","name":"Drums","channels":[0,1]}],
             "channels":[{"name":"Lead","kind":"voice","hidden":false,"link":true},
                         {"name":"","kind":"guitar","hidden":true,"link":false}]}
            """

        guard case .configuration(let config) = try decode(json) else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.sourceChannels == 2)
        #expect(config.sampleRate == 48_000)
        #expect(config.groups == [ChannelGroup(id: "g1", name: "Drums", channels: [0, 1])])
        #expect(
            config.channels == [
                SourceChannel(name: "Lead", kind: .voice, isHidden: false, isLinkedToNext: true),
                SourceChannel(name: "", kind: .guitar, isHidden: true, isLinkedToNext: false),
            ])
    }

    @Test("reads every kind the desk can name", arguments: ChannelKind.allCases)
    func decodesEveryKind(kind: ChannelKind) throws {
        let json = """
            {"type":"config","source_channels":1,"sample_rate":48000,"groups":[],
             "channels":[{"name":"CH 1","kind":"\(kind.rawValue)","hidden":false,"link":false}]}
            """

        guard case .configuration(let config) = try decode(json) else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.channels.first?.kind == kind)
    }

    /// The desk's own contract: "one that meets a kind it does not know shows it as `other`"
    /// (`apps/server/src/protocol.rs:420-424`).
    @Test("shows a kind it does not know as other")
    func unknownKindBecomesOther() throws {
        let json = """
            {"type":"config","source_channels":1,"sample_rate":48000,"groups":[],
             "channels":[{"name":"CH 1","kind":"theremin","hidden":false,"link":false}]}
            """

        guard case .configuration(let config) = try decode(json) else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.channels.first?.kind == .other)
    }

    @Test("ignores a field it has never heard of")
    func ignoresUnknownFields() throws {
        let json = """
            {"type":"config","source_channels":1,"sample_rate":48000,"groups":[],"channels":[],
             "something_added_later":{"nested":true}}
            """

        guard case .configuration(let config) = try decode(json) else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.sourceChannels == 1)
    }

    @Test("reads a config with no groups or channels listed as empty")
    func toleratesAbsentLists() throws {
        guard case .configuration(let config) = try decode(
            #"{"type":"config","source_channels":0,"sample_rate":48000}"#)
        else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.groups.isEmpty)
        #expect(config.channels.isEmpty)
    }

    @Test("refuses a config with no sample rate")
    func refusesConfigWithoutSampleRate() {
        #expect(throws: ServerMessageError.missingField("sample_rate", inMessageType: "config")) {
            try decode(#"{"type":"config","source_channels":2}"#)
        }
    }

    @Test("knows the mix sizes the desk works in")
    func mixSizes() {
        #expect(DeskConfiguration.maximumMixChannels == 32)
        #expect(DeskConfiguration.maximumGroups == 16)
    }

    // MARK: mix_ack and mix_state

    @Test("reads an ack as the answer to its own numbered mix")
    func decodesMixAck() throws {
        guard case .mixAck(let report) = try decode(mixAckJSON) else {
            Issue.record("expected a mix ack")
            return
        }

        #expect(report.sequence == 7)
        #expect(report.volumePercent == 80)
        #expect(report.maxLevelPercent == 95)
        #expect(report.muted == false)
        #expect(report.channelGains == [10, 20])
        #expect(report.pans == [0, 100])
        #expect(report.channelMutes == [false, true])
        #expect(report.channelSolos == [true, false])
        #expect(report.groupLevels == [30])
        #expect(report.engineerLimitPercent == 90)
        #expect(report.locked == [false, true])
        #expect(report.following == nil)
    }

    @Test("reads a pushed state, which never carries a number")
    func decodesMixState() throws {
        let json = """
            {"type":"mix_state","volume_percent":50,"max_level_percent":100,"muted":true,
             "channels":[5],"pans":[50],"mutes":[false],"solos":[false],"group_levels":[],
             "engineer_limit_percent":100,"locked":[false],"following":"Ana"}
            """

        guard case .mixState(let report) = try decode(json) else {
            Issue.record("expected a mix state")
            return
        }

        #expect(report.sequence == nil)
        #expect(report.muted == true)
        #expect(report.following == "Ana")
    }

    /// `engineer_limit_percent`, `locked` and `following` were all added after the rest, so a desk
    /// that predates one of them sends a report without it. 100 is the documented "no limit".
    @Test("reads a report from a desk that predates the engineer's controls")
    func toleratesAbsentEngineerControls() throws {
        let json = """
            {"type":"mix_ack","volume_percent":80,"max_level_percent":100,"muted":false,
             "channels":[10,20,30],"pans":[50,50,50],"mutes":[false,false,false],
             "solos":[false,false,false],"group_levels":[]}
            """

        guard case .mixAck(let report) = try decode(json) else {
            Issue.record("expected a mix ack")
            return
        }

        #expect(MixReport.noEngineerLimitPercent == 100)
        #expect(report.engineerLimitPercent == 100)
        #expect(report.locked == [false, false, false])
        #expect(report.following == nil)
    }

    @Test("refuses a report with no master volume")
    func refusesReportWithoutVolume() {
        #expect(
            throws: ServerMessageError.missingField("volume_percent", inMessageType: "mix_ack")
        ) {
            try decode(#"{"type":"mix_ack","max_level_percent":100,"muted":false}"#)
        }
    }

    // MARK: playback

    @Test("reads the desk handing the cushion back to the phone")
    func decodesAutoPlayback() throws {
        #expect(
            try decode(#"{"type":"playback","buffer":{"mode":"auto"}}"#)
                == .playbackBuffer(.auto))
    }

    @Test("reads a cushion the desk pinned")
    func decodesFixedPlayback() throws {
        #expect(
            try decode(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
                == .playbackBuffer(.fixed(milliseconds: 40)))
    }

    @Test("holds a pinned cushion to the range the desk allows", arguments: [(1, 5), (999, 500)])
    func clampsFixedPlayback(sent: Int, kept: UInt16) throws {
        #expect(
            try decode(#"{"type":"playback","buffer":{"mode":"fixed","ms":\#(sent)}}"#)
                == .playbackBuffer(.fixed(milliseconds: kept)))
        #expect(PlaybackBufferSetting.minimumFixedMilliseconds == 5)
        #expect(PlaybackBufferSetting.maximumFixedMilliseconds == 500)
    }

    /// A pinned cushion with no length, or a mode this build does not know, cannot be honoured.
    /// Adapting on our own is the safe reading; dropping the message would leave us guessing too.
    @Test(
        "falls back to adapting when it cannot honour the buffer the desk named",
        arguments: [
            #"{"type":"playback","buffer":{"mode":"fixed"}}"#,
            #"{"type":"playback","buffer":{"mode":"elastic","ms":40}}"#,
        ])
    func fallsBackToAuto(json: String) throws {
        #expect(try decode(json) == .playbackBuffer(.auto))
    }

    @Test("refuses a playback message with no buffer at all")
    func refusesPlaybackWithoutBuffer() {
        #expect(throws: ServerMessageError.missingField("buffer", inMessageType: "playback")) {
            try decode(#"{"type":"playback"}"#)
        }
    }

    // MARK: redundancy

    @Test("reads how long to hold packets back")
    func decodesRedundancy() throws {
        #expect(try decode(#"{"type":"redundancy","delay_blocks":2}"#) == .redundancyHold(blocks: 2))
        #expect(try decode(#"{"type":"redundancy","delay_blocks":0}"#) == .redundancyHold(blocks: 0))
    }

    @Test("holds the delay to the blocks the desk can ask for")
    func clampsRedundancy() throws {
        #expect(ServerMessage.maximumRedundancyHoldBlocks == 4)
        #expect(try decode(#"{"type":"redundancy","delay_blocks":9}"#) == .redundancyHold(blocks: 4))
    }

    @Test("refuses a redundancy message with no delay")
    func refusesRedundancyWithoutDelay() {
        #expect(
            throws: ServerMessageError.missingField("delay_blocks", inMessageType: "redundancy")
        ) {
            try decode(#"{"type":"redundancy"}"#)
        }
    }

    // MARK: error

    @Test("reads an error that answers nothing in particular")
    func decodesError() throws {
        #expect(
            try decode(#"{"type":"error","message":"no such channel"}"#)
                == .error(DeskError(message: "no such channel", sequence: nil)))
    }

    @Test("reads the number of the mix an error refused")
    func decodesErrorWithSequence() throws {
        #expect(
            try decode(#"{"type":"error","message":"locked","seq":7}"#)
                == .error(DeskError(message: "locked", sequence: 7)))
    }

    // MARK: routing

    /// The desk's contract, stated for message after message: "a phone ignores any unknown type".
    /// Keeping the name rather than returning nothing means a diagnostics screen can say what
    /// arrived instead of silently counting it as nothing.
    @Test("keeps the name of a type it does not know instead of failing")
    func keepsUnknownType() throws {
        #expect(
            try decode(#"{"type":"something_new","payload":{"a":1}}"#)
                == .unrecognized(type: "something_new"))
    }

    @Test("refuses something that is not JSON at all")
    func refusesNonJSON() {
        #expect(throws: ServerMessageError.notJSON) {
            try decode("not json at all")
        }
    }

    @Test("refuses a message with no type to route on", arguments: [#"{}"#, #"{"message":"hi"}"#])
    func refusesMissingType(json: String) {
        #expect(throws: ServerMessageError.missingType) {
            try decode(json)
        }
    }
}
