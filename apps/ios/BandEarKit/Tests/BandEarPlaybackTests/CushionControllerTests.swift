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

    /// Android passes the track's packet frames to `follow` and the played packet's samples to
    /// `onPlayed` (`PmonPlayer.kt:320-323`); a desk that changes its block size mid-stream makes
    /// them differ. 40 ms in 256-frame packets is 8 packets, whatever the packet played; 24 000
    /// played frames at 48 kHz is half a second, so the second packet completes one.
    @Test("counts the cushion in the player's packets and the time in the packet played")
    func cushionAndTimeTakeTheirOwnFrames() {
        let cushion = CushionController()
        func step() -> CushionStep {
            cushion.onPacket(
                setting: .fixed(milliseconds: 40), packetFrames: 256, playedFrames: 24_000,
                playbackRate: 48_000, packetRate: 48_000, capacityFrames: 96_000)
        }

        #expect(step() == [.apply])
        #expect(cushion.cushionPackets == 8)
        #expect(step() == [.adjust])
        #expect(cushion.cushionPackets == 8)
    }

    /// Android returns these as bit flags in an `Int`; the option set has to keep both when both
    /// happen on the same packet, or the caller would apply the new cushion and miss the adjustment.
    @Test("reports both steps when one packet changes the cushion and completes a second")
    func stepsCompose() {
        let cushion = CushionController()
        let step = cushion.onPacket(
            setting: .fixed(milliseconds: 40), packetFrames: 48_000, playedFrames: 48_000,
            playbackRate: 48_000, packetRate: 48_000, capacityFrames: 96_000)

        #expect(step == [.apply, .adjust])
    }
}
