import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// Android's `openChannel` (`ControlChannelTest.kt:38-45`) and `connected(channels)`
/// (`MonitorViewModelTest.kt:71-76`): the session starts for Ana, the socket opens, and the desk
/// announces `channels` channels, as it does right after the connection opens.
@MainActor
private func connected(channels: Int = 2) async throws -> (ControlSession, FakeDesk) {
    let transport = FakeTransport()
    let session = ControlSession(transport: transport, scheduler: ManualScheduler())
    let desk = FakeDesk(transport)
    try session.start(toStage, musicianName: "Ana")
    await desk.open()
    await desk.receive(configMessage(channels: channels))
    return (session, desk)
}

/// True for a JSON number written without a fraction or exponent: what the desk parses as an
/// integer. `JSONSerialization` keeps that distinction in the `NSNumber` it returns.
private func isInteger(_ value: Any?) -> Bool {
    guard let number = value as? NSNumber else { return false }
    return !CFNumberIsFloatType(number)
}

/// The Android client's tests of its control channel, from `ControlChannelTest.kt` and
/// `ControlChannelReconnectTest.kt` (`apps/android/app/src/test/java/com/bandear/android/`), with
/// Android's own JSON, driven through a ``FakeTransport`` as Android drives its `FakeSocket`.
///
/// Where Android hands its listener an event, the session shows the state that event leads to:
/// a pushed or answered mix is the session's mix, `onAcknowledged` is ``ControlSession/mixHeldByDesk``,
/// `onControlError` is ``ControlSession/lastError``, the buffer and the hold are
/// ``ControlSession/playbackBuffer`` and ``ControlSession/redundancyHoldBlocks``. How each message
/// is decoded is ported with the decoder (`MixReportParityTests`, `ConfigParityTests`) and what it
/// means for the mix with the mix module (`DeskReportsParityTests`); the halves those suites left to
/// the connection (what is acknowledged, shown, or sent back) are completed here, under the same
/// names.
@Suite("Control session: Android parity")
@MainActor
struct ControlSessionParityTests {

    // MARK: What the desk pushes

    /// Android also checks the 32-channel tables it hands its listener; the session keeps the two
    /// channels the desk announced, and the decoder suite checks the tables.
    @Test func aPushedMixIsReadWholeAndHandedOnWithNothingSentBack() async throws {
        let (session, desk) = try await connected()
        let sent = desk.transport.sent.count

        try await desk.receive(mixReport("mix_state"))

        let mix = session.mixSession.mix
        #expect(mix.levels == [10, 11])
        #expect(mix.pans == [0, 25])
        #expect(mix.mutes == [false, false])
        #expect(mix.solos == [false, false])
        #expect(mix.masterVolume == 80)
        #expect(mix.levelCap == 90)
        #expect(mix.masterMuted)
        #expect(desk.transport.sent.count == sent)
        #expect(session.lastError == nil)
    }

    @Test func unknownTypesAreIgnored() async throws {
        let (session, desk) = try await connected()
        try #require(session.mixSession.mix.levels == [100, 100])
        let before = session.mixSession
        let sent = desk.transport.sent.count

        await desk.receive(#"{"type":"mix_preview","channels":[1,2]}"#)

        #expect(session.mixSession == before)
        #expect(desk.transport.sent.count == sent)
        #expect(session.lastError == nil)
    }

    /// Android also reports it as "Invalid control response"; this session keeps only the desk's own
    /// `error` as its error and ignores a message it cannot read, as it ignores an unknown one.
    @Test func aPushMissingPartOfTheMixIsReportedAndNotHandedOn() async throws {
        let (session, desk) = try await connected()
        try #require(session.mixSession.mix.levels == [100, 100])
        let before = session.mixSession

        await desk.receive(#"{"type":"mix_state","volume_percent":80,"channels":[100,100]}"#)

        #expect(session.mixSession == before)
        #expect(session.lastError == nil)
    }

    // MARK: The playback buffer the desk sets

    @Test func aPlaybackMessageHandsOnTheDesksBufferWithNothingSentBack() async throws {
        let (session, desk) = try await connected()
        let sent = desk.transport.sent.count
        var buffers: [PlaybackBufferSetting] = []

        for message in [
            #"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#,
            #"{"type":"playback","buffer":{"mode":"auto"}}"#,
            #"{"type":"playback","buffer":{"mode":"fixed","ms":5,"floor":2},"later":true}"#,
        ] {
            await desk.receive(message)
            buffers.append(session.playbackBuffer)
        }

        #expect(buffers == [.fixed(milliseconds: 40), .auto, .fixed(milliseconds: 5)])
        #expect(desk.transport.sent.count == sent)
        #expect(session.lastError == nil)
    }

    /// Android hands nothing on for any of these. The session starts on Auto, so a fixed buffer is
    /// set first, or a message read as Auto would pass unnoticed.
    ///
    /// Four of them diverge, in the decoder (`ServerMessage.swift`, T2b), which reads a mode it does
    /// not know and a fixed buffer with no length as Auto, and holds a length outside 5-500 ms to
    /// that range, where Android ignores the message and keeps the buffer it had. Marked as known
    /// issues so the suite fails, asking for them to be unmarked, the day the decoder agrees.
    @Test func aPlaybackMessageThePhoneCannotUseIsIgnored() async throws {
        let (session, desk) = try await connected()
        await desk.receive(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
        try #require(session.playbackBuffer == .fixed(milliseconds: 40))

        for unusable in [
            #"{"type":"playback"}"#,
            #"{"type":"playback","buffer":"auto"}"#,
            #"{"type":"playback","buffer":{"mode":"fixed","ms":40.5}}"#,
            #"{"type":"playback","buffer":{"mode":"fixed","ms":"40"}}"#,
        ] {
            await desk.receive(unusable)
            #expect(session.playbackBuffer == .fixed(milliseconds: 40), "\(unusable)")
        }
        await withKnownIssue("the decoder reads these instead of ignoring them (T2b)") {
            for unusable in [
                #"{"type":"playback","buffer":{"mode":"elastic","ms":40}}"#,
                #"{"type":"playback","buffer":{"mode":"fixed"}}"#,
                #"{"type":"playback","buffer":{"mode":"fixed","ms":4}}"#,
                #"{"type":"playback","buffer":{"mode":"fixed","ms":501}}"#,
            ] {
                await desk.receive(#"{"type":"playback","buffer":{"mode":"fixed","ms":40}}"#)
                await desk.receive(unusable)
                #expect(session.playbackBuffer == .fixed(milliseconds: 40), "\(unusable)")
            }
        }
        #expect(session.lastError == nil)
    }

    // MARK: The hold the desk's delayed redundant copies need

    @Test func aRedundancyMessageHandsOnTheHoldWithNothingSentBack() async throws {
        let (session, desk) = try await connected()
        let sent = desk.transport.sent.count
        var holds: [UInt8] = []

        for message in [
            #"{"type":"redundancy","delay_blocks":2}"#,
            #"{"type":"redundancy","delay_blocks":0}"#,
            #"{"type":"redundancy","delay_blocks":4,"later":true}"#,
        ] {
            await desk.receive(message)
            holds.append(session.redundancyHoldBlocks)
        }

        #expect(holds == [2, 0, 4])
        #expect(desk.transport.sent.count == sent)
        #expect(session.lastError == nil)
    }

    /// Android hands nothing on for any of these. The session starts with no hold, so a hold is set
    /// first, or a message read as no hold would pass unnoticed.
    ///
    /// Two of them diverge, in the decoder (`ServerMessage.swift`, T2b), which holds a delay outside
    /// 0-4 blocks to that range, where Android ignores the message and keeps the hold it had.
    @Test func aRedundancyMessageThePhoneCannotUseIsIgnored() async throws {
        let (session, desk) = try await connected()
        await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
        try #require(session.redundancyHoldBlocks == 2)

        for unusable in [
            #"{"type":"redundancy"}"#,
            #"{"type":"redundancy","delay_blocks":1.5}"#,
            #"{"type":"redundancy","delay_blocks":"2"}"#,
        ] {
            await desk.receive(unusable)
            #expect(session.redundancyHoldBlocks == 2, "\(unusable)")
        }
        await withKnownIssue("the decoder holds these to 0-4 instead of ignoring them (T2b)") {
            for unusable in [
                #"{"type":"redundancy","delay_blocks":-1}"#,
                #"{"type":"redundancy","delay_blocks":5}"#,
            ] {
                await desk.receive(#"{"type":"redundancy","delay_blocks":2}"#)
                await desk.receive(unusable)
                #expect(session.redundancyHoldBlocks == 2, "\(unusable)")
            }
        }
        #expect(session.lastError == nil)
    }

    // MARK: The desk's answers to the phone's mixes

    /// Android's fake hands the channel 12 as the newest number; the session numbers its own mixes,
    /// so the ack names the number the opening mix went out under.
    @Test func aNumberedAckAnswersItsMixWithTheMixTheServerPlays() async throws {
        let (session, desk) = try await connected()
        let opening = try #require(try desk.transport.sent(ofType: "mix").first)
        let seq = try #require(opening["seq"] as? Int)

        try await desk.receive(mixReport("mix_ack", firstLevel: 40, seq: seq))

        #expect(session.mixSession.sync.answered == UInt64(seq))
        #expect(session.mixSession.mix.levels == [40, 41])
        #expect(session.mixHeldByDesk)
    }

    @Test func anAckWithoutANumberOnlyAcknowledges() async throws {
        let (session, desk) = try await connected()
        try #require(session.mixSession.mix.levels == [100, 100])
        let before = session.mixSession

        try await desk.receive(mixReport("mix_ack", firstLevel: 40))

        #expect(session.mixSession == before)
        #expect(session.mixHeldByDesk)
    }

    /// Android's number, 12, is kept here and in the next unreadable-ack test: the session's own
    /// number is still 0, which an answer could not be seen to move.
    @Test func aNumberedErrorAnswersItsMixWithNothingToShow() async throws {
        let (session, desk) = try await connected()

        await desk.receive(#"{"type":"error","message":"too many channels","seq":12}"#)

        #expect(session.mixSession.sync.answered == 12)
        #expect(session.lastError == DeskError(message: "too many channels", sequence: 12))
    }

    @Test func anErrorWithoutANumberNeverTouchesTheMixes() async throws {
        let (session, desk) = try await connected()
        try #require(session.mixSession.mix.levels == [100, 100])
        let before = session.mixSession

        await desk.receive(#"{"type":"error","message":"no UDP target configured for client IP"}"#)

        #expect(session.mixSession == before)
        #expect(
            session.lastError
                == DeskError(message: "no UDP target configured for client IP", sequence: nil))
    }

    @Test func aNumberedAckWhoseMixCannotBeReadStillAnswersItsNumber() async throws {
        let (session, desk) = try await connected()

        await desk.receive(#"{"type":"mix_ack","seq":12,"channels":[100]}"#)

        #expect(session.mixSession.sync.answered == 12)
        #expect(session.mixHeldByDesk)
        #expect(session.lastError == nil)
    }

    // MARK: The link-stats report

    /// The keys every report carries. `recovered` is among them on Android, whose report always has
    /// a count; ``LinkStats`` leaves it out when it is nil, so these reports pass 0 as Android's do.
    private static let wireKeys: Set<String> = [
        "type", "interval_ms", "packets_received", "packets_lost", "underruns", "receive_timeouts",
        "recovered",
    ]

    /// What reached the desk for `stats`, sent on an open connection. Android builds the message
    /// with `buildStatsMessage`; here it goes out through ``ControlSession/send(stats:)``.
    private func sentStats(_ stats: LinkStats) async throws -> [String: Any] {
        let (session, desk) = try await connected()
        session.send(stats: stats)
        return try #require(try desk.transport.sent(ofType: "stats").last)
    }

    @Test func statsMessageIsExactlyTheWireShape() async throws {
        let message = try await sentStats(
            LinkStats(
                intervalMilliseconds: 1_000, packetsReceived: 200, packetsLost: 0, underruns: 0,
                receiveTimeouts: 0, recovered: 0))
        let expected = try #require(
            try JSONSerialization.jsonObject(
                with: Data(
                    #"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0,"recovered":0}"#
                        .utf8)) as? [String: Any])

        #expect(Set(message.keys) == Self.wireKeys)
        #expect(NSDictionary(dictionary: message) == NSDictionary(dictionary: expected))
    }

    @Test func statsMessageCarriesEachCountInItsOwnFieldAsAnInteger() async throws {
        let message = try await sentStats(
            LinkStats(
                intervalMilliseconds: 1_250, packetsReceived: 98, packetsLost: 2, underruns: 1,
                receiveTimeouts: 3, recovered: 4))

        #expect(message["type"] as? String == "stats")
        let expected: [String: Int] = [
            "interval_ms": 1_250, "packets_received": 98, "packets_lost": 2, "underruns": 1,
            "receive_timeouts": 3, "recovered": 4,
        ]
        for (key, value) in expected {
            #expect(isInteger(message[key]), "\(key) must be an integer, got \(String(describing: message[key]))")
            #expect(message[key] as? Int == value, "\(key)")
        }
    }

    /// Android rounds each extra to a hundredth before sending it; ``LinkStats`` (T2a) sends the
    /// value as measured, which the desk reads just as well. Marked as a known issue so the suite
    /// says so the day it rounds.
    @Test func statsMessageCarriesTheMeasuredExtrasInMilliseconds() async throws {
        let message = try await sentStats(
            LinkStats(
                intervalMilliseconds: 1_000, packetsReceived: 200, packetsLost: 0, underruns: 0,
                receiveTimeouts: 0, jitterMilliseconds: 1.2345, cushionMilliseconds: 10.0,
                backlogMilliseconds: 37.5, recovered: 0))

        #expect(Set(message.keys) == Self.wireKeys.union(["jitter_ms", "cushion_ms", "backlog_ms"]))
        withKnownIssue("LinkStats does not round to a hundredth (T2a)") {
            #expect(message["jitter_ms"] as? Double == 1.23, "rounded to a hundredth")
        }
        #expect(message["cushion_ms"] as? Double == 10.0)
        #expect(message["backlog_ms"] as? Double == 37.5)
        #expect(message["recovered"] as? Int == 0)
    }

    @Test func statsMessageLeavesOutAnExtraNotMeasuredOrOutsideWhatTheServerReads() async throws {
        func keysFor(_ jitter: Double, _ cushion: Double, _ backlog: Double) async throws -> Set<String> {
            let message = try await sentStats(
                LinkStats(
                    intervalMilliseconds: 1_000, packetsReceived: 200, packetsLost: 0, underruns: 0,
                    receiveTimeouts: 0, jitterMilliseconds: jitter, cushionMilliseconds: cushion,
                    backlogMilliseconds: backlog, recovered: 0))
            return Set(message.keys).subtracting(Self.wireKeys)
        }

        #expect(try await keysFor(2.0, .nan, .nan) == ["jitter_ms"])
        #expect(try await keysFor(.infinity, 0.0, -1.0) == ["cushion_ms"])
        #expect(try await keysFor(60_000.5, -0.5, 60_000.0) == ["backlog_ms"])
    }

    // MARK: Opening and stopping (`ControlChannelReconnectTest.kt`, the tests with no reconnect)

    @Test func aSessionOpensOneConnectionToTheGivenServer() throws {
        let transport = FakeTransport()
        let session = ControlSession(transport: transport, scheduler: ManualScheduler())

        try session.start(toStage, musicianName: "Ana")

        #expect(transport.opened == [stage])
    }

    @Test func stopClosesASocketThatIsStillOpening() async throws {
        let transport = FakeTransport()
        let scheduler = ManualScheduler()
        let session = ControlSession(transport: transport, scheduler: scheduler)
        let desk = FakeDesk(transport)
        try session.start(toStage, musicianName: "Ana")
        try #require(transport.opened.count == 1)

        session.stop()
        await desk.open()
        desk.post(.failed("late failure"))
        await settle()

        #expect(transport.closes == 1)
        #expect(transport.sent.isEmpty)
        #expect(scheduler.scheduled.isEmpty)
        #expect(session.connection == .idle)
        #expect(transport.opened.count == 1)
    }
}

/// The Android client's tests of what its ViewModel sends and shows that need a socket, from
/// `MonitorViewModelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/ui/state/`). T5e
/// ported the rest through a socket-less harness; these three need the connection's ordering.
///
/// Android queues every send on one control thread, which its fake scheduler can hold; this session
/// sends at once on the main actor, and what can arrive late is the desk's message, still on its
/// way to the main actor. So where Android holds a send back, these let a desk message wait in the
/// main queue instead, which is the same race seen from the other side.
@Suite("Control session: Android ViewModel parity")
@MainActor
struct ControlSessionViewModelParityTests {

    private static let bandConfig =
        #"{"type":"config","source_channels":3,"groups":[{"name":"Band","channels":[0,1]}]}"#

    /// The desk's answer to the phone's `sent` mix when it kept its own channels and groups: every
    /// channel from `levels` on and the first group at `groupLevel`.
    private func answerKeepingTheServersMix(
        _ sent: [String: Any], levels: [Int], groupLevel: Int
    ) throws -> [String: Any] {
        var answer = mixState(levels: levels, type: "mix_ack", seq: try #require(sent["seq"] as? Int))
        answer["group_levels"] = (0..<16).map { $0 == 0 ? groupLevel : 100 }
        return answer
    }

    /// The desk's `config` follows the open at once, so it can be waiting before the session has
    /// handled the open. The opening mix is read when the open is handled, before the `config`: it
    /// carries no channels and no groups, the desk keeps its own, and its answer gives them to the
    /// phone.
    @Test func aConfigArrivingBeforeTheOpeningMixIsSentLeavesTheServersMixAloneAndThePhoneTakesIt()
        async throws
    {
        let transport = FakeTransport()
        let session = ControlSession(transport: transport, scheduler: ManualScheduler())
        let desk = FakeDesk(transport)
        try session.start(toStage, musicianName: "Ana")

        desk.post(.opened)
        desk.post(.received(Self.bandConfig))
        await settle()

        let mixes = try transport.sent(ofType: "mix")
        try #require(mixes.count == 1)
        let opening = mixes[0]
        #expect(ints(opening, "channels") == [])
        #expect(ints(opening, "pans") == [])
        #expect((opening["mutes"] as? [Bool])?.isEmpty == true)
        #expect((opening["solos"] as? [Bool])?.isEmpty == true)
        #expect(ints(opening, "group_levels") == [])

        try await desk.receive(answerKeepingTheServersMix(opening, levels: [40, 50, 60], groupLevel: 70))

        #expect(session.mixSession.mix.levels == [40, 50, 60])
        #expect(session.mixSession.mix.groupLevels == [70])
        #expect(try transport.sent(ofType: "mix").count == 1)
    }

    @Test func anAnswerArrivingBeforeAQueuedChangeIsSentNeitherUndoesNorLosesIt() async throws {
        let (session, desk) = try await connected(channels: 2)
        session.apply(.musicianChannelChosen(1))
        session.apply(.moreOfMeToggled(on: true))
        try await desk.answerMixes()
        session.apply(.channelLevelChanged(channel: 0, level: 40))

        // The desk's answer to that mix is already on its way when the musician moves on.
        try await desk.answerMixes(settling: false)
        session.apply(.channelLevelChanged(channel: 0, level: 50))
        await settle()

        #expect(session.mixSession.mix.levels == [50, 100])
        #expect(session.mixSession.moreOfMe.isEnabled)
        #expect(ints(try desk.lastMix(), "channels") == [25, 100])

        try await desk.answerMixes()

        #expect(session.mixSession.mix.levels == [50, 100])
        #expect(session.mixSession.moreOfMe.isEnabled)
    }

    /// Android's own test is already holding the mix through the answers before it; the Swift-only
    /// suite checks an unnumbered ack alone.
    @Test func anAckWithoutANumberOnlySaysTheServerHoldsTheMix() async throws {
        let (session, desk) = try await connected(channels: 2)
        try await desk.answerMixes()

        try await desk.receive(mixState(levels: [80, 70], type: "mix_ack"))

        #expect(session.mixSession.mix.levels == [100, 100])
        #expect(session.mixHeldByDesk)
    }
}
