import BandEarProtocol
import Foundation
import Testing

@testable import BandEarMix

/// A mix report as the desk writes it: its full 32-channel and 16-group tables, and `seq` if any.
/// Android's `mixReport` helper in `ControlChannelTest.kt:47-59`.
private func mixReport(_ type: String, firstLevel: Int = 10, seq: Int? = nil) -> [String: Any] {
    var object: [String: Any] = [
        "type": type,
        "volume_percent": 80,
        "max_level_percent": 90,
        "muted": true,
        "channels": (0..<32).map { firstLevel + $0 },
        "pans": (0..<32).map { $0 == 1 ? 25 : ($0 % 2 == 0 ? 0 : 100) },
        "mutes": (0..<32).map { $0 == 2 },
        "solos": (0..<32).map { $0 == 3 },
        "group_levels": (0..<16).map { 50 + $0 },
    ]
    if let seq { object["seq"] = seq }
    return object
}

/// `object` with `key` set to `value`, or without `key` when `value` is nil, which is what org.json's
/// `put` does with a Kotlin null. `NSNull()` stands for Android's `JSONObject.NULL`.
private func putting(_ key: String, _ value: Any?, in object: [String: Any]) -> [String: Any] {
    var object = object
    object[key] = value
    return object
}

/// Android's `withEngineer` helper: `report` carrying the engineer's `limit` and `locked`.
private func withEngineer(_ report: [String: Any], _ limit: Any?, _ locked: Any?) -> [String: Any] {
    putting("locked", locked, in: putting("engineer_limit_percent", limit, in: report))
}

/// What the desk's `messages` mean for the mix, in order: what Android's control channel hands its
/// listener as `onMixPushed` and `onMixAnswered`.
private func mixEvents(_ messages: [String: Any]...) throws -> [MixEvent] {
    try messages.compactMap { object in
        MixEvent(reportedBy: try message(object))
    }
}

private func message(_ object: [String: Any]) throws -> ServerMessage {
    try ServerMessage(decodingJSON: JSONSerialization.data(withJSONObject: object))
}

private func message(_ json: String) throws -> ServerMessage {
    try ServerMessage(decodingJSON: Data(json.utf8))
}

/// The engineer's controls `events` carry, in order: Android's `RecordingListener.engineer`, which
/// records a push's and a readable answer's, never a refusal's.
private func engineer(of events: [MixEvent]) -> [EngineerControls] {
    events.compactMap { event in
        switch event {
        case .mixPushed(let report): report.engineerControls
        case .mixAnswered(_, let report): report?.engineerControls
        default: nil
        }
    }
}

/// A one-channel mix as the desk reports it, numbered `sequence`.
private func oneChannel(_ sequence: UInt64? = nil) -> MixReport {
    MixReport(
        sequence: sequence, volumePercent: 80, maxLevelPercent: 90, muted: false, channelGains: [70],
        pans: [50], channelMutes: [false], channelSolos: [false], groupLevels: [],
        engineerLimitPercent: 60, locked: [false, true, false, true], following: "Ana")
}

/// The Android client's tests of what its control channel hands on from the desk's mix reports,
/// from `ControlChannelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/`), with
/// Android's own JSON. Each message is decoded and mapped to the event it means for the mix; what
/// the channel also does with an ack (counting it, marking the mix held by the desk) is the
/// connection's, ported with it in T6.
@Suite("Desk reports: Android parity")
struct DeskReportsParityTests {

    @Test func aPushAndAnAckCarryTheEngineersLimitAndLocks() throws {
        let events = try mixEvents(
            withEngineer(mixReport("mix_state"), 60, (0..<32).map { $0 == 0 || $0 == 3 }),
            withEngineer(mixReport("mix_ack", seq: 12), 75, (0..<32).map { $0 == 1 }))

        #expect(
            engineer(of: events) == [
                EngineerControls(limitPercent: 60, lockedChannels: [0, 3]),
                EngineerControls(limitPercent: 75, lockedChannels: [1]),
            ])
        try #require(events.count == 2)
        guard case .mixPushed(let pushed) = events[0] else {
            Issue.record("expected a push first")
            return
        }
        #expect(pushed.channelGains.first == 10)
    }

    @Test func aReportFromAnOlderServerHasNoLimitAndNothingLocked() throws {
        let events = try mixEvents(mixReport("mix_state"))

        #expect(engineer(of: events) == [EngineerControls()])
        #expect(!EngineerControls().isLimited)
    }

    @Test func aReportSaysWhoseMixThePhoneFollows() throws {
        let events = try mixEvents(
            putting("following", "Ana", in: mixReport("mix_state")),
            putting("following", NSNull(), in: mixReport("mix_ack", seq: 12)))

        #expect(engineer(of: events).map(\.following) == ["Ana", nil])
    }

    /// Android also counts the ack as acknowledged and marks the mix held by the desk; that is the
    /// connection's (T6).
    @Test func aNumberedAckAnswersItsMixWithTheMixTheServerPlays() throws {
        let events = try mixEvents(mixReport("mix_ack", firstLevel: 40, seq: 12))

        try #require(events.count == 1)
        guard case .mixAnswered(let sequence, let mix) = events[0] else {
            Issue.record("expected an answer")
            return
        }
        #expect(sequence == 12)
        #expect(mix?.channelGains.first == 40)
    }

    /// Android also counts it as acknowledged; that is the connection's (T6). Here the ack still
    /// decodes, so there is something to count.
    @Test func anAckWithoutANumberOnlyAcknowledges() throws {
        let ack = try message(mixReport("mix_ack", firstLevel: 40))

        #expect(MixEvent(reportedBy: ack) == nil)
        guard case .mixAck(let report) = ack else {
            Issue.record("expected a mix ack")
            return
        }
        #expect(report.sequence == nil)
    }

    @Test func aNumberedErrorAnswersItsMixWithNothingToShow() throws {
        let error = try message(#"{"type":"error","message":"too many channels","seq":12}"#)

        #expect(MixEvent(reportedBy: error) == .mixAnswered(sequence: 12, report: nil))
        #expect(error == .error(DeskError(message: "too many channels", sequence: 12)))
    }

    @Test func anErrorWithoutANumberNeverTouchesTheMixes() throws {
        let error = try message(#"{"type":"error","message":"no UDP target configured for client IP"}"#)

        #expect(MixEvent(reportedBy: error) == nil)
        #expect(error == .error(DeskError(message: "no UDP target configured for client IP", sequence: nil)))
    }

    /// Android also counts it as acknowledged (T6); no error is the decode not throwing.
    @Test func aNumberedAckWhoseMixCannotBeReadStillAnswersItsNumber() throws {
        let ack = try message(#"{"type":"mix_ack","seq":12,"channels":[100]}"#)

        #expect(MixEvent(reportedBy: ack) == .mixAnswered(sequence: 12, report: nil))
    }
}

/// What each message from the desk means for the mix, and the engineer's controls a report carries.
@Suite("Desk reports")
struct DeskReportsTests {

    @Test("hands a pushed mix on as a push, whatever it carries")
    func pushMapsToPush() {
        #expect(MixEvent(reportedBy: .mixState(oneChannel())) == .mixPushed(oneChannel()))
    }

    @Test("hands a numbered ack on as the answer to its number, with its mix")
    func numberedAckMapsToAnswer() {
        #expect(
            MixEvent(reportedBy: .mixAck(oneChannel(3)))
                == .mixAnswered(sequence: 3, report: oneChannel(3)))
    }

    @Test("answers the number of an ack it could not read, and of an error, with nothing to show")
    func refusalsMapToAnswersWithoutAMix() {
        #expect(
            MixEvent(reportedBy: .unreadableMixAck(sequence: 4)) == .mixAnswered(sequence: 4, report: nil))
        #expect(
            MixEvent(reportedBy: .error(DeskError(message: "locked", sequence: 5)))
                == .mixAnswered(sequence: 5, report: nil))
    }

    @Test(
        "means nothing for the mix from an unnumbered ack or error, or any other message",
        arguments: [
            ServerMessage.mixAck(oneChannel()),
            .error(DeskError(message: "no UDP target", sequence: nil)),
            .configuration(
                DeskConfiguration(sourceChannels: 2, sampleRate: nil, groups: [], channels: nil)),
            .playbackBuffer(.auto),
            .redundancyHold(blocks: 2),
            .unrecognized(type: "mix_preview"),
        ])
    func otherMessagesMapToNothing(message: ServerMessage) {
        #expect(MixEvent(reportedBy: message) == nil)
    }

    @Test("reads the engineer's controls off a report: its limit, the channels it locks, whom it follows")
    func engineerControlsFromReport() {
        #expect(
            oneChannel().engineerControls
                == EngineerControls(limitPercent: 60, lockedChannels: [1, 3], following: "Ana"))
    }

    @Test("says the engineer limited the musician only below full scale")
    func isLimitedBelowFullScale() {
        #expect(EngineerControls(limitPercent: 60).isLimited)
        #expect(EngineerControls(limitPercent: 99).isLimited)
        #expect(!EngineerControls(limitPercent: 100).isLimited)
    }
}
