import Testing

@testable import BandEarPlayback

/// The Android client's own tests for its playback measures, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/PlaybackLatencyTest.kt`: same inputs, same
/// expected values, to the same 1e-9.
///
/// Not every test of that file is here:
/// - The five `nextCushion` tests are in `CushionParityTests`, with the policy they test.
/// - The three `keepsLowLatency` tests are not ported, because the helper is not: it decides whether
///   an Android `AudioTrack` honoured the buffer size set on it (see ``PlaybackLatency``).
/// - `trackCapacityFrames` has no test in this file; its Android test lives in
///   `CushionControllerTest.kt` and is already ported, against
///   ``CushionPolicy/capacityFrames(sampleRate:packetFrames:)``.
///
/// Two translations, both deliberate:
/// - A measure Android reports as `NaN` (not measured) is `nil` here.
/// - Android's `timestampRead` flag and its `presentedFrame` become one optional `presentedFrame`:
///   `timestampRead = false` reads `presentedFrame: nil`, whatever frame Android passed beside it.
@Suite("Playback measures: Android parity")
struct PlaybackLatencyParityTests {
    /// Android's `assertEquals(expected, actual, 1e-9)`, for a measure that must have been taken.
    private func expectMilliseconds(
        _ actual: Double?, _ expected: Double, sourceLocation: SourceLocation = #_sourceLocation
    ) throws {
        let value = try #require(actual, "not measured", sourceLocation: sourceLocation)
        #expect(
            abs(value - expected) <= 1e-9, "\(value) is not \(expected)",
            sourceLocation: sourceLocation)
    }

    // PlaybackLatencyTest.kt:9-12
    @Test func countsFramesWrittenButNotPresented() {
        #expect(PlaybackLatency.pendingFrames(framesWritten: 1_000, framePosition: 520) == 480)
    }

    // PlaybackLatencyTest.kt:14-17
    @Test func clampsWhenTheRenderPositionRunsAheadOfTheWriter() {
        #expect(PlaybackLatency.pendingFrames(framesWritten: 100, framePosition: 250) == 0)
    }

    // PlaybackLatencyTest.kt:19-23
    @Test func convertsPendingFramesToMillisecondsAt48k() throws {
        // 480 frames at 48 kHz is exactly 10 ms.
        try expectMilliseconds(
            PlaybackLatency.backlogMilliseconds(
                framesWritten: 1_000, framePosition: 520, sampleRate: 48_000),
            10.0)
    }

    // PlaybackLatencyTest.kt:25-28
    @Test func reportsZeroBacklogWhenTheTrackIsFullyDrained() throws {
        try expectMilliseconds(
            PlaybackLatency.backlogMilliseconds(
                framesWritten: 1_000, framePosition: 1_000, sampleRate: 48_000),
            0.0)
    }

    // PlaybackLatencyTest.kt:30-33
    @Test func reportsZeroForAnUnusableSampleRate() throws {
        try expectMilliseconds(
            PlaybackLatency.backlogMilliseconds(framesWritten: 1_000, framePosition: 520, sampleRate: 0),
            0.0)
    }

    // PlaybackLatencyTest.kt:35-39
    @Test func bufferIsTheCushionTimesThePacket() {
        #expect(PlaybackLatency.targetBufferFrames(packetFrames: 480, cushionPackets: 2) == 960)
        #expect(PlaybackLatency.targetBufferFrames(packetFrames: 480, cushionPackets: 1) == 480)
    }

    // PlaybackLatencyTest.kt:41-44
    @Test func bufferIsNeverZeroFrames() {
        #expect(PlaybackLatency.targetBufferFrames(packetFrames: 0, cushionPackets: 2) == 1)
    }

    // PlaybackLatencyTest.kt:46-52
    @Test func theCushionInMillisecondsIsTheBufferItAsksForAtTheTracksRate() throws {
        // 240-frame packets at 48 kHz are 5 ms each.
        try expectMilliseconds(
            PlaybackLatency.cushionMilliseconds(packetFrames: 240, cushionPackets: 2, sampleRate: 48_000),
            10.0)
        try expectMilliseconds(
            PlaybackLatency.cushionMilliseconds(packetFrames: 240, cushionPackets: 1, sampleRate: 48_000),
            5.0)
        try expectMilliseconds(
            PlaybackLatency.cushionMilliseconds(packetFrames: 480, cushionPackets: 4, sampleRate: 48_000),
            40.0)
    }

    // PlaybackLatencyTest.kt:54-58
    @Test func theCushionIsNotMeasuredBeforeTheFirstPacketOrAtAnUnusableRate() {
        #expect(
            PlaybackLatency.cushionMilliseconds(packetFrames: 0, cushionPackets: 2, sampleRate: 48_000)
                == nil)
        #expect(
            PlaybackLatency.cushionMilliseconds(packetFrames: 240, cushionPackets: 2, sampleRate: 0)
                == nil)
    }

    // PlaybackLatencyTest.kt:60-75
    @Test func aReportEndsWithTheCushionAskedOfTheTrackAndTheAudioQueued() throws {
        // Distinct packet frames and cushion packets: 240-frame packets, 3 of them, are 15 ms; using
        // one value twice would give 1200 ms or 0.19 ms.
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 240, cushionPackets: 3, playbackRate: 48_000, framesWritten: 1_000,
            presentedFrame: 520)
        try expectMilliseconds(extras.cushionMilliseconds, 15.0)
        // 480 frames queued at 48 kHz.
        try expectMilliseconds(extras.backlogMilliseconds, 10.0)
    }

    // PlaybackLatencyTest.kt:77-107
    @Test func theCushionReportedCountsThePacketsHeldBackForDelayedCopies() throws {
        // 240-frame packets at 48 kHz are 5 ms each: a 3-packet cushion and a 2-packet hold are 25 ms;
        // swapped they would read the same, so the track's applied size is used too: 600 frames and 2
        // held packets are 12.5 + 10 ms.
        let asked = PlaybackLatency.reportExtras(
            packetFrames: 240, cushionPackets: 3, playbackRate: 48_000, framesWritten: 0,
            presentedFrame: nil, holdPackets: 2)
        try expectMilliseconds(asked.cushionMilliseconds, 25.0)
        let applied = PlaybackLatency.reportExtras(
            packetFrames: 240, cushionPackets: 3, playbackRate: 48_000, framesWritten: 0,
            presentedFrame: nil, appliedBufferFrames: 600, holdPackets: 2)
        try expectMilliseconds(applied.cushionMilliseconds, 22.5)
        #expect(
            PlaybackLatency.reportExtras(
                packetFrames: 0, cushionPackets: 3, playbackRate: 48_000, framesWritten: 0,
                presentedFrame: nil, holdPackets: 2
            ).cushionMilliseconds == nil,
            "no packet, no cushion, held or not")
    }

    // PlaybackLatencyTest.kt:109-123
    @Test func aReportBeforeTheFirstPacketHasNoCushionWhateverTheCushionPackets() {
        // No packet sized the buffer yet: the start cushion of 2 packets is not a cushion. Swapped,
        // 2-frame packets would read as a tiny cushion instead of none.
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 0, cushionPackets: CushionPolicy.startPackets, playbackRate: 48_000,
            framesWritten: 0, presentedFrame: nil)
        #expect(extras.cushionMilliseconds == nil)
        #expect(extras.backlogMilliseconds == nil)
    }

    // PlaybackLatencyTest.kt:125-137
    @Test func theBacklogCountsFramesWrittenPastThePresentedFrameNotTheOtherWayRound() throws {
        // Swapped, the written count would sit behind the render position and read as 0 ms.
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 256, cushionPackets: 2, playbackRate: 48_000, framesWritten: 2_400,
            presentedFrame: 480)
        try expectMilliseconds(extras.backlogMilliseconds, 40.0)
    }

    // PlaybackLatencyTest.kt:139-151
    @Test func withoutATimestampTheBacklogIsNotMeasuredAndTheCushionStillIs() throws {
        // Android passes `framesWritten = 2_400L, presentedFrame = 480L` with `timestampRead = false`:
        // a frame that was not read is not passed at all here.
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 256, cushionPackets: 2, playbackRate: 48_000, framesWritten: 2_400,
            presentedFrame: nil)
        #expect(extras.backlogMilliseconds == nil)
        try expectMilliseconds(extras.cushionMilliseconds, 512 * 1_000.0 / 48_000)
    }

    // PlaybackLatencyTest.kt:153-165
    @Test func withoutATrackRateTheCushionIsNotMeasured() {
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 256, cushionPackets: 2, playbackRate: nil, framesWritten: 0,
            presentedFrame: nil)
        #expect(extras.cushionMilliseconds == nil)
        #expect(extras.backlogMilliseconds == nil)
    }

    // PlaybackLatencyTest.kt:167-188
    @Test func aReportCarriesTheBufferTheTrackAppliedWhenItIsKnown() throws {
        // Asked for 2 packets of 256 frames (512), the platform kept 768: the phone plays 16 ms, not
        // the 10.67 ms it asked for.
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 256, cushionPackets: 2, playbackRate: 48_000, framesWritten: 0,
            presentedFrame: nil, appliedBufferFrames: 768)
        try expectMilliseconds(extras.cushionMilliseconds, 16.0)
        // Unknown (0) falls back to what was asked; before the first packet it is still not measured.
        try expectMilliseconds(
            PlaybackLatency.reportExtras(
                packetFrames: 256, cushionPackets: 2, playbackRate: 48_000, framesWritten: 0,
                presentedFrame: nil, appliedBufferFrames: 0
            ).cushionMilliseconds,
            512 * 1_000.0 / 48_000)
        #expect(
            PlaybackLatency.reportExtras(
                packetFrames: 0, cushionPackets: 2, playbackRate: 48_000, framesWritten: 0,
                presentedFrame: nil, appliedBufferFrames: 768
            ).cushionMilliseconds == nil)
    }
}
