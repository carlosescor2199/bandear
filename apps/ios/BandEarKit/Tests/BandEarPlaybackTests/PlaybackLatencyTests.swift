import Testing

@testable import BandEarPlayback

/// What the playback measures do beyond the Android parity suite: where `nil` comes from, now that
/// it stands in for Android's `NaN`.
@Suite("Playback measures")
struct PlaybackLatencyTests {

    /// Android: "a read timestamp with no rate only happens in a test and reads as 0, as `backlogMs`
    /// reads it" (`PlaybackLatency.kt:129-130`). The backlog is unmeasured only when no frame was
    /// read; an unknown rate costs the cushion, not the backlog.
    @Test("reads a presented frame with no rate as no backlog, not as unmeasured")
    func backlogWithoutARate() {
        let extras = PlaybackLatency.reportExtras(
            packetFrames: 256, cushionPackets: 2, playbackRate: nil, framesWritten: 2_400,
            presentedFrame: 480)

        #expect(extras.backlogMilliseconds == 0)
        #expect(extras.cushionMilliseconds == nil)
    }
}
