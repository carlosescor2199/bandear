import Foundation
import Testing

@testable import BandEarProtocol

/// A mix report as the desk writes it: its full 32-channel and 16-group tables, and `seq` if any.
/// Android's `mixReport` helper in `ControlChannelTest.kt:47-59`.
private func mixReport(_ type: String, firstLevel: Int = 10, seq: Int? = nil) -> [String: Any] {
    var object: [String: Any] = [
        "type": type,
        "volume_percent": 80,
        "max_level_percent": 90,
        "muted": true,
        "channels": (0..<32).map { firstLevel + $0 },
        "pans": (0..<32).map { $0 == 1 ? 25 : defaultPan($0) },
        "mutes": (0..<32).map { $0 == 2 },
        "solos": (0..<32).map { $0 == 3 },
        "group_levels": (0..<16).map { 50 + $0 },
    ]
    if let seq { object["seq"] = seq }
    return object
}

/// Android's `defaultPan`: even channels left, odd ones right.
private func defaultPan(_ channel: Int) -> Int {
    channel % 2 == 0 ? 0 : 100
}

/// `object` with `key` set to `value`, or without `key` when `value` is nil, which is what org.json's
/// `put` does with a Kotlin null. `NSNull()` stands for Android's `JSONObject.NULL`.
private func putting(_ key: String, _ value: Any?, in object: [String: Any]) -> [String: Any] {
    var object = object
    object[key] = value
    return object
}

private func decode(_ object: [String: Any]) throws -> ServerMessage {
    try ServerMessage(decodingJSON: JSONSerialization.data(withJSONObject: object))
}

/// What Android's `parseEngineerControls` returns, read off a decoded report. Android's
/// `EngineerControls` lives in the mix module here, which the protocol tests do not see; a locked
/// channel is one whose `locked` entry reads true.
private struct Controls: Equatable {
    let limit: Int
    let locked: Set<Int>
    let following: String?

    init(_ limit: Int = 100, _ locked: Set<Int> = [], following: String? = nil) {
        self.limit = limit
        self.locked = locked
        self.following = following
    }

    init(of report: MixReport) {
        self.init(
            report.engineerLimitPercent, Set(report.locked.indices.filter { report.locked[$0] }),
            following: report.following)
    }
}

/// The Android client's tests of how a `mix_state` and a `mix_ack` are read, from
/// `ControlChannelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/`), with Android's
/// own JSON. Android calls `parseEngineerControls` directly in two of them; here the same report
/// goes through a whole `mix_state`. What the channel then hands its listener (the mix events) is
/// ported in `DeskReportsTests` of the mix module.
@Suite("Desk mix reports: Android parity")
struct MixReportParityTests {

    /// Android also asserts that nothing is sent back and no error is reported. Nothing is sent
    /// back is the connection's (T6); no error is the decode not throwing.
    @Test func aPushedMixIsReadWholeAndHandedOnWithNothingSentBack() throws {
        guard case .mixState(let mix) = try decode(mixReport("mix_state")) else {
            Issue.record("expected a mix state")
            return
        }

        #expect(mix.channelGains == (0..<32).map { 10 + $0 })
        #expect(mix.pans == (0..<32).map { $0 == 1 ? 25 : defaultPan($0) })
        #expect(mix.channelMutes == (0..<32).map { $0 == 2 })
        #expect(mix.channelSolos == (0..<32).map { $0 == 3 })
        #expect(mix.groupLevels == (0..<16).map { 50 + $0 })
        #expect(mix.volumePercent == 80)
        #expect(mix.maxLevelPercent == 90)
        #expect(mix.muted)
    }

    /// Android reports it as "Invalid control response" and hands nothing on; here the decode
    /// throws, and a message that throws is never handed on.
    @Test func aPushMissingPartOfTheMixIsReportedAndNotHandedOn() {
        #expect(throws: ServerMessageError.self) {
            try ServerMessage(
                decodingJSON: Data(#"{"type":"mix_state","volume_percent":80,"channels":[100,100]}"#.utf8))
        }
    }

    @Test func theEngineersControlsAreReadLeniently() throws {
        func read(_ limit: Any?, _ locked: Any?) throws -> Controls? {
            let object = putting(
                "locked", locked, in: putting("engineer_limit_percent", limit, in: mixReport("mix_state")))
            guard case .mixState(let report) = try decode(object) else {
                Issue.record("expected a mix state")
                return nil
            }
            return Controls(of: report)
        }

        // A short list leaves the rest open; an entry that is not a boolean is open.
        #expect(try read(nil, [true, false, true]) == Controls(100, [0, 2]))
        #expect(try read(nil, [1, NSNull(), true, "true"] as [Any]) == Controls(100, [2]))
        // A limit that is not a number is no limit; one out of range is held to 0-100.
        #expect(try read("high", "all") == Controls())
        #expect(try read(140, nil) == Controls(100))
        #expect(try read(-5, nil) == Controls(0))
    }

    @Test func followingIsReadLeniently() throws {
        func read(_ value: Any?) throws -> String? {
            guard case .mixState(let report) = try decode(
                putting("following", value, in: mixReport("mix_state")))
            else {
                Issue.record("expected a mix state")
                return nil
            }
            return report.following
        }

        // Absent (an older desk), null, or anything but a string follows nobody.
        #expect(try read(nil) == nil)
        #expect(try read(NSNull()) == nil)
        #expect(try read(7) == nil)
        #expect(try read(["Ana"]) == nil)
        #expect(try read("Ana") == "Ana")
        // A leader the desk has no name for is still followed.
        #expect(try read("") == "")
    }
}
