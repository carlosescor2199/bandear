import Foundation
import Testing

@testable import BandEarProtocol

private func details(_ json: String) throws -> [SourceChannel]? {
    guard case .configuration(let config) = try ServerMessage(decodingJSON: Data(json.utf8)) else {
        Issue.record("expected a configuration")
        return nil
    }
    return config.channels
}

private func info(
    _ name: String = "", _ kind: ChannelKind = .other, hidden: Bool = false, link: Bool = false
) -> SourceChannel {
    SourceChannel(name: name, kind: kind, isHidden: hidden, isLinkedToNext: link)
}

/// The Android client's tests of how a `config` is read, from `ControlChannelTest.kt`
/// (`apps/android/app/src/test/java/com/bandear/android/`), with Android's own JSON. Android
/// calls `parseChannelDetails(array, count)` directly in one of them; here the same array and count
/// go through a whole `config`, whose `source_channels` is that count.
@Suite("Desk config: Android parity")
struct ConfigParityTests {

    @Test func aConfigHandsOnTheChannelDetailsInChannelOrder() throws {
        let json = """
            {"type":"config","source_channels":4,"groups":[],"channels":[
                {"name":"Vox","kind":"voice","hidden":false,"link":false},
                {"name":"Keys","kind":"keys","hidden":false,"link":true},
                {"name":"","kind":"keys","hidden":false,"link":false},
                {"name":"Spare","kind":"other","hidden":true,"link":false}]}
            """

        #expect(
            try details(json) == [
                info("Vox", .voice), info("Keys", .keys, link: true), info("", .keys),
                info("Spare", .other, hidden: true),
            ])
    }

    @Test func aConfigFromAnOlderServerHandsOnNoDetails() throws {
        // The opening config of Android's `openChannel`, which has no `channels`, as an older desk's.
        #expect(try details(#"{"type":"config","source_channels":2,"groups":[]}"#) == nil)
    }

    @Test func channelDetailsAreReadLeniently() throws {
        let json = """
            {"type":"config","source_channels":3,"channels":[
                {"name":"Gtr","kind":"theremin","hidden":true,"link":true},
                "not an entry",
                {"name":7,"kind":null},
                {"name":"Extra","kind":"bass"}]}
            """

        // An unknown kind reads as other; an entry that is not an object, or a field of the wrong
        // type, reads as the default; an entry past the announced count is dropped.
        #expect(try details(json) == [info("Gtr", .other, hidden: true, link: true), info(), info()])
        // Fewer entries than channels: the rest are channels the engineer never touched.
        #expect(
            try details(#"{"type":"config","source_channels":2,"channels":[{"name":"Bass","kind":"bass"}]}"#)
                == [info("Bass", .bass), info()])
        // Anything but an array is no details at all.
        #expect(try details(#"{"type":"config","source_channels":2}"#) == nil)
        #expect(try details(#"{"type":"config","source_channels":2,"channels":"voice"}"#) == nil)
    }

    @Test func everyKindIsReadByItsWireName() {
        for kind in ChannelKind.allCases {
            #expect(ChannelKind(rawValue: kind.rawValue) == kind)
        }
        #expect(
            ChannelKind.allCases.map(\.rawValue) == [
                "voice", "guitar", "bass", "keys", "drums", "percussion", "horns", "strings",
                "playback", "click", "other",
            ])
    }
}
