import Foundation
import Testing

@testable import BandEarProtocol

private func decode(_ json: String) throws -> ServerMessage {
    try ServerMessage(decodingJSON: Data(json.utf8))
}

/// The groups a `config` carrying `groups` (as raw JSON) is read with.
private func configGroups(_ groups: String) throws -> [ChannelGroup] {
    guard case .configuration(let config) = try decode(
        #"{"type":"config","source_channels":4,"groups":"# + groups + "}")
    else {
        Issue.record("expected a configuration")
        return []
    }
    return config.groups
}

/// A whole `mix_ack` with two channels and one group, as the desk writes it
/// (`apps/server/src/protocol.rs:382-396`).
private let mixAckJSON = """
    {"type":"mix_ack","seq":7,"volume_percent":80,"max_level_percent":95,"muted":false,
     "channels":[10,20],"pans":[0,100],"mutes":[false,true],"solos":[true,false],
     "group_levels":[30],"engineer_limit_percent":90,"locked":[false,true],"following":null}
    """

/// A whole one-channel `mix_state`, every field present, written on one line so a test can replace
/// one field's text.
private let wholeMixState = """
    {"type":"mix_state","volume_percent":50,"max_level_percent":100,"muted":false,"channels":[5],\
    "pans":[50],"mutes":[false],"solos":[false],"group_levels":[],"engineer_limit_percent":100,\
    "locked":[false],"following":null}
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

        #expect(config.channels?.first?.kind == kind)
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

        #expect(config.channels?.first?.kind == .other)
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

    /// An absent `channels` is an older desk's, and it is not the same as an empty list: with no
    /// details a strip shows no kind icon, with details every strip shows one, `other` at worst.
    @Test("reads a config with no groups as none, and with no channel details as none at all")
    func toleratesAbsentLists() throws {
        guard case .configuration(let config) = try decode(
            #"{"type":"config","source_channels":0,"sample_rate":48000}"#)
        else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.groups.isEmpty)
        #expect(config.channels == nil)
    }

    /// The Android client never reads `sample_rate` from a `config`, and its own tests send configs
    /// without one. The rate that matters is in every audio datagram's header.
    @Test("reads a config with no sample rate")
    func readsConfigWithoutSampleRate() throws {
        guard case .configuration(let config) = try decode(#"{"type":"config","source_channels":2}"#)
        else {
            Issue.record("expected a configuration")
            return
        }

        #expect(config.sampleRate == nil)
        #expect(config.sourceChannels == 2)
    }

    @Test("refuses a config that does not say how many channels it describes")
    func refusesConfigWithoutSourceChannels() {
        #expect(throws: ServerMessageError.missingField("source_channels", inMessageType: "config")) {
            try decode(#"{"type":"config","groups":[]}"#)
        }
    }

    // MARK: config groups, read as leniently as the Android client's parseGroups

    /// Rules from `parseGroups` (`ControlChannel.kt:642-658`), which has no test of its own on
    /// Android: a group's `id` came after its name and channels on the desk, so an older desk sends
    /// none; an entry that is not an object, or has no name, is skipped; a channel that is not a
    /// whole number, or is negative, is dropped. One bad group never costs the musician the others.
    @Test("keeps a group from a desk that predates group ids")
    func groupWithoutID() throws {
        let groups = try configGroups(#"[{"name":"Band","channels":[0,1]}]"#)

        #expect(groups == [ChannelGroup(id: nil, name: "Band", channels: [0, 1])])
    }

    @Test(
        "skips a group that is not an object or has no name",
        arguments: [#"["not a group"]"#, #"[{"channels":[0]}]"#, #"[{"name":"","channels":[0]}]"#, #"[{"name":7}]"#])
    func skipsUnusableGroup(groups: String) throws {
        #expect(try configGroups(groups).isEmpty)
    }

    @Test("keeps the groups around one it has to skip")
    func keepsGoodGroupsAroundABadOne() throws {
        let groups = try configGroups(#"[{"name":"A","channels":[0]},"junk",{"name":"B","channels":[1]}]"#)

        #expect(groups.map(\.name) == ["A", "B"])
    }

    @Test("drops a group channel that is not a whole number or is negative")
    func groupChannelsLenient() throws {
        let groups = try configGroups(#"[{"name":"A","channels":[0,"one",-1,2.5,3,null]}]"#)

        #expect(groups.map(\.channels) == [[0, 3]])
    }

    @Test("reads a group with no channels as an empty one, and groups that are not a list as none")
    func groupListsLenient() throws {
        #expect(try configGroups(#"[{"name":"A"}]"#).map(\.channels) == [[]])
        #expect(try configGroups(#""not a list""#).isEmpty)
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

    // MARK: mix reports, read as the Android client's parseMixReport and parseEngineerControls

    /// `parseMixReport` reads each of the five lists with `getJSONArray`, which throws when one is
    /// missing (`ControlChannel.kt:601-613`): the desk always sends the whole mix.
    @Test(
        "refuses a pushed mix missing any of its lists",
        arguments: ["channels", "pans", "mutes", "solos", "group_levels"])
    func refusesReportMissingAList(list: String) throws {
        var object = try #require(
            try JSONSerialization.jsonObject(with: Data(wholeMixState.utf8)) as? [String: Any])
        object[list] = nil

        #expect(throws: ServerMessageError.self) {
            try ServerMessage(decodingJSON: JSONSerialization.data(withJSONObject: object))
        }
    }

    @Test("refuses a pushed mix with an entry of the wrong type in a list")
    func refusesReportWithWrongEntry() {
        #expect(throws: ServerMessageError.self) {
            try decode(wholeMixState.replacingOccurrences(of: #""mutes":[false]"#, with: #""mutes":[null]"#))
        }
    }

    /// Android: "An ack whose mix cannot be read still answers its number" (`ControlChannel.kt:174`).
    @Test(
        "answers the number of an ack whose mix cannot be read",
        arguments: [
            #"{"type":"mix_ack","seq":12,"channels":[100]}"#,
            #"{"type":"mix_ack","seq":12,"volume_percent":"loud"}"#,
        ])
    func numberedUnreadableAck(json: String) throws {
        #expect(try decode(json) == .unreadableMixAck(sequence: 12))
    }

    /// `answeredSequence` (`ControlChannel.kt:631`) names a mix only with a number that is not
    /// negative; anything else names none, and the ack still acknowledges.
    @Test(
        "reads an ack whose number is not a whole number that is not negative as one with none",
        arguments: [#""seq":-1,"#, #""seq":"12","#, #""seq":1.5,"#, #""seq":null,"#])
    func ackWithUnusableNumber(seq: String) throws {
        let json = wholeMixState.replacingOccurrences(of: #""type":"mix_state","#, with: #""type":"mix_ack","# + seq)

        guard case .mixAck(let report) = try decode(json) else {
            Issue.record("expected a mix ack")
            return
        }
        #expect(report.sequence == nil)
        #expect(report.channelGains == [5])
    }

    @Test("ignores whatever number a pushed mix carries")
    func pushedMixNumberIgnored() throws {
        for seq in [#""seq":7,"#, #""seq":"x","#] {
            let json = wholeMixState.replacingOccurrences(of: #""type":"mix_state","#, with: #""type":"mix_state","# + seq)
            guard case .mixState(let report) = try decode(json) else {
                Issue.record("expected a mix state")
                return
            }
            #expect(report.sequence == nil)
        }
    }

    /// Kotlin's `Number.toInt()` cuts a fraction toward zero before `coerceIn(0, 100)`; a boolean is
    /// not a `Number`, so it is no limit.
    @Test(
        "reads a limit with a fraction as its whole part, and a limit that is not a number as none",
        arguments: [("60.9", 60), ("-0.5", 0), ("100.5", 100), ("true", 100), ("null", 100)])
    func limitLenient(limit: String, read: Int) throws {
        let json = wholeMixState.replacingOccurrences(
            of: #""engineer_limit_percent":100"#, with: #""engineer_limit_percent":"# + limit)

        guard case .mixState(let report) = try decode(json) else {
            Issue.record("expected a mix state")
            return
        }
        #expect(report.engineerLimitPercent == read)
    }

    @Test("reads a locked list that is not a list as nothing locked")
    func lockedNotAList() throws {
        let json = wholeMixState.replacingOccurrences(of: #""locked":[false]"#, with: #""locked":{"0":true}"#)

        guard case .mixState(let report) = try decode(json) else {
            Issue.record("expected a mix state")
            return
        }
        #expect(report.locked == [false])
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
