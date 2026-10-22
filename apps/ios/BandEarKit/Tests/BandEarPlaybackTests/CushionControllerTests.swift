import Testing

import BandEarProtocol
@testable import BandEarPlayback

/// What the cushion does beyond the Android parity suite: the bounds it keeps, and the step flags
/// composing when one packet both changes the cushion and completes a second.
@Suite("Playback cushion")
struct CushionControllerTests {

    @Test("keeps the bounds the Android client uses")
    func bounds() {
        #expect(CushionPolicy.minimumPackets == 1)
        #expect(CushionPolicy.maximumPackets == 4)
        #expect(CushionPolicy.startPackets == 2)
        #expect(CushionPolicy.adjustIntervalMilliseconds == 1_000)
    }

    @Test("judges a second at 48 kHz when the rate cannot be used", arguments: [0, -1])
    func adjustIntervalFallsBackTo48k(rate: Int) {
        #expect(CushionPolicy.adjustIntervalFrames(sampleRate: rate) == 48_000)
    }

    @Test("sizes the player for the longest buffer the desk can pin")
    func capacityFollowsTheDesksLongestBuffer() {
        let frames = CushionPolicy.capacityFrames(sampleRate: 48_000, packetFrames: 0)

        #expect(frames == Int(PlaybackBufferSetting.maximumFixedMilliseconds) * 48)
    }

    /// Android returns these as bit flags in an `Int`; the option set has to keep both when both
    /// happen on the same packet, or the caller would apply the new cushion and miss the adjustment.
    @Test("reports both steps when one packet changes the cushion and completes a second")
    func stepsCompose() {
        let cushion = CushionController()
        let step = cushion.onPacket(
            setting: .fixed(milliseconds: 40), packetFrames: 48_000, playbackRate: 48_000,
            packetRate: 48_000, capacityFrames: 96_000)

        #expect(step == [.apply, .adjust])
    }
}
