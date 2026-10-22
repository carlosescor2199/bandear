import Testing

import BandEarProtocol
@testable import BandEarPlayback

/// The Android client's own tests for the playback cushion, ported case for case: every test in
/// `CushionControllerTest.kt` and the `nextCushion` tests in `PlaybackLatencyTest.kt`
/// (`apps/android/app/src/test/java/com/bandear/android/`). Same inputs, same expected outputs.
///
/// Two translations, both deliberate:
/// - Android's `fixedMs` is an `Int` where 0 means Auto. Here it is ``PlaybackBufferSetting``, the
///   value the `playback` message already decodes to, so `fixedMs == 40` reads
///   `setting == .fixed(milliseconds: 40)` and `DESK_BUFFER_AUTO` reads `.auto`.
/// - Android's `onPacket` takes `packetFrames` (to size the cushion) and `packetSamples` (to count
///   time, divided by a fixed `CHANNELS = 2`). Here it takes frames only, read off the datagram
///   header with its real channel count. Counting samples as frames, which two Android tests exist
///   to catch, can no longer be written. Those two tests are ported by what they assert about time
///   (how many frames make a second) rather than by their now-impossible argument lists.
@Suite("Playback cushion: Android parity")
struct CushionParityTests {
    /// The player holds the longest fixed buffer at 48 kHz in 256-frame packets.
    private let capacity = CushionPolicy.capacityFrames(sampleRate: 48_000, packetFrames: 256)

    // MARK: CushionControllerTest.kt

    @Test func theTrackHoldsTheLongestFixedBufferInWholePackets() {
        // 500 ms at 48 kHz is 24 000 frames: 93.75 packets of 256, so 94 of them.
        #expect(CushionPolicy.capacityFrames(sampleRate: 48_000, packetFrames: 256) == 94 * 256)
        #expect(CushionPolicy.capacityFrames(sampleRate: 48_000, packetFrames: 240) == 24_000)
        #expect(CushionPolicy.capacityFrames(sampleRate: 44_100, packetFrames: 0) == 22_050)
    }

    @Test func aFixedLengthIsRoundedUpToWholePackets() {
        func packets(_ ms: Int, _ frames: Int) -> Int {
            CushionPolicy.fixedCushionPackets(
                milliseconds: ms, packetFrames: frames, sampleRate: 48_000, capacityFrames: capacity)
        }
        // 40 ms at 48 kHz is 1 920 frames: 7.5 packets of 256 frames, so 8; exactly 8 of 240.
        #expect(packets(40, 256) == 8)
        #expect(packets(40, 240) == 8)
        // One frame past a whole packet is one packet more.
        #expect(packets(6, 256) == 2)
        // Never below one packet.
        #expect(packets(5, 512) == 1)
        // The longest fits whole.
        #expect(packets(500, 256) == 94)
    }

    @Test func aFixedLengthIsClampedToWhatTheTrackHolds() {
        // A player that holds 4 800 frames holds 18 whole packets of 256.
        #expect(
            CushionPolicy.fixedCushionPackets(
                milliseconds: 500, packetFrames: 256, sampleRate: 48_000, capacityFrames: 4_800) == 18)
        // Never below one packet, even for a player smaller than a packet.
        #expect(
            CushionPolicy.fixedCushionPackets(
                milliseconds: 500, packetFrames: 256, sampleRate: 48_000, capacityFrames: 100) == 1)
        // Before the first packet sized anything there is nothing to convert: one packet.
        #expect(
            CushionPolicy.fixedCushionPackets(
                milliseconds: 40, packetFrames: 0, sampleRate: 48_000, capacityFrames: capacity) == 1)
    }

    @Test func anAdjustmentIsDueOnceASecondOfAudioWasPlayedWhateverThePacketSize() {
        let small = CushionController()
        // 256-frame packets at 48 kHz are 187.5 a second: the 188th completes the second.
        for packet in 1...187 {
            #expect(small.onPlayed(frames: 256, sampleRate: 48_000) == false, "packet \(packet)")
        }
        #expect(small.onPlayed(frames: 256, sampleRate: 48_000) == true)
        // What passed the second counts towards the next: 375 packets are exactly two seconds.
        for packet in 189...374 {
            #expect(small.onPlayed(frames: 256, sampleRate: 48_000) == false, "packet \(packet)")
        }
        #expect(small.onPlayed(frames: 256, sampleRate: 48_000) == true)

        let large = CushionController()
        for _ in 1...99 { #expect(large.onPlayed(frames: 480, sampleRate: 48_000) == false) }
        #expect(large.onPlayed(frames: 480, sampleRate: 48_000) == true, "100 packets of 10 ms")
    }

    @Test func onAutoTheCushionAdaptsAsBefore() {
        let cushion = CushionController()
        #expect(cushion.cushionPackets == CushionPolicy.startPackets)
        #expect(cushion.setting == .auto)

        #expect(cushion.adjust(underrunsTotal: 1) == true, "an underrun grows it")
        #expect(cushion.cushionPackets == 3)
        #expect(cushion.adjust(underrunsTotal: 1) == false)
        #expect(cushion.adjust(underrunsTotal: 1) == false)
        #expect(cushion.adjust(underrunsTotal: 1) == true, "three clean seconds shrink it")
        #expect(cushion.cushionPackets == 2)
    }

    @Test func aFixedBufferHoldsTheCushionWhateverTheUnderruns() {
        let cushion = CushionController()

        #expect(
            cushion.follow(
                .fixed(milliseconds: 40), packetFrames: 256, sampleRate: 48_000,
                capacityFrames: capacity) == true)
        #expect(cushion.cushionPackets == 8)
        #expect(cushion.setting == .fixed(milliseconds: 40))
        for underruns in [3, 9, 9, 9, 9, 20] {
            #expect(cushion.adjust(underrunsTotal: underruns) == false)
            #expect(cushion.cushionPackets == 8)
        }
        #expect(
            cushion.follow(
                .fixed(milliseconds: 40), packetFrames: 256, sampleRate: 48_000,
                capacityFrames: capacity) == false,
            "the same length again changes nothing")
        #expect(
            cushion.follow(
                .fixed(milliseconds: 100), packetFrames: 256, sampleRate: 48_000,
                capacityFrames: capacity) == true)
        #expect(cushion.cushionPackets == 19)
    }

    @Test func aPacketFollowsTheDesksBufferWithItsOwnFramesRateAndTrack() {
        let cushion = CushionController()
        // 40 ms at 48 kHz in 256-frame packets is 8 packets; swapping the packet frames with the
        // player's capacity, or the rate with either, would give 1 packet or the whole player.
        let step = cushion.onPacket(
            setting: .fixed(milliseconds: 40), packetFrames: 256, playbackRate: 48_000,
            packetRate: 44_100, capacityFrames: 4_800)
        #expect(step.contains(.apply))
        #expect(cushion.cushionPackets == 8)
        #expect(cushion.setting == .fixed(milliseconds: 40))
        // A player that holds 4 800 frames clamps a long buffer to its 18 whole packets.
        #expect(
            cushion.onPacket(
                setting: .fixed(milliseconds: 500), packetFrames: 256, playbackRate: 48_000,
                packetRate: 44_100, capacityFrames: 4_800
            ).contains(.apply))
        #expect(cushion.cushionPackets == 18)
    }

    @Test func theSameDeskBufferAgainAsksNothingOfTheTrack() {
        let cushion = CushionController()
        func step(_ controller: CushionController, _ setting: PlaybackBufferSetting) -> CushionStep {
            controller.onPacket(
                setting: setting, packetFrames: 256, playbackRate: 48_000, packetRate: 48_000,
                capacityFrames: capacity)
        }
        #expect(step(cushion, .fixed(milliseconds: 40)).contains(.apply))
        #expect(!step(cushion, .fixed(milliseconds: 40)).contains(.apply))
        // Auto from the start changes nothing either: the controller already adapts.
        #expect(!step(CushionController(), .auto).contains(.apply))
    }

    /// Android: `onPacket(AUTO, packetFrames = 24_000, packetSamples = 48_000, …)`. Here the frames
    /// are all there is, so this pins the half of the test that is about time: 24 000 frames at
    /// 48 kHz is half a second, so the second packet completes it.
    @Test func aSecondIsCountedInFramesOfThePacketNotItsSamples() {
        let cushion = CushionController()
        func step() -> CushionStep {
            cushion.onPacket(
                setting: .auto, packetFrames: 24_000, playbackRate: 48_000, packetRate: 48_000,
                capacityFrames: capacity)
        }
        #expect(!step().contains(.adjust))
        #expect(step().contains(.adjust))
    }

    /// Android passes `packetSamples = 44_100`, which is 22 050 stereo frames; that is the frame
    /// count used here. Half a second at the packet's 44.1 kHz, so with no player rate the second
    /// packet completes the second; at a 22.05 kHz player rate one does.
    @Test func withoutATrackRateTheSecondIsThePacketsRate() {
        let noTrack = CushionController()
        func noTrackStep() -> CushionStep {
            noTrack.onPacket(
                setting: .auto, packetFrames: 22_050, playbackRate: nil, packetRate: 44_100,
                capacityFrames: capacity)
        }
        #expect(!noTrackStep().contains(.adjust))
        #expect(noTrackStep().contains(.adjust))

        let slowTrack = CushionController()
        #expect(
            slowTrack.onPacket(
                setting: .auto, packetFrames: 22_050, playbackRate: 22_050, packetRate: 44_100,
                capacityFrames: capacity
            ).contains(.adjust))
    }

    @Test func autoResumesAdaptingFromTheCushionPlayedWithinItsBounds() {
        let cushion = CushionController()
        _ = cushion.follow(
            .fixed(milliseconds: 40), packetFrames: 256, sampleRate: 48_000, capacityFrames: capacity)
        // Underruns while fixed are the fixed buffer's: Auto does not grow for them.
        _ = cushion.adjust(underrunsTotal: 5)

        #expect(
            cushion.follow(.auto, packetFrames: 256, sampleRate: 48_000, capacityFrames: capacity)
                == true)
        #expect(cushion.cushionPackets == CushionPolicy.maximumPackets)
        #expect(cushion.setting == .auto)
        #expect(cushion.adjust(underrunsTotal: 5) == false)
        #expect(cushion.adjust(underrunsTotal: 5) == false)
        #expect(cushion.adjust(underrunsTotal: 5) == true, "shrinks after three clean seconds")
        #expect(cushion.cushionPackets == CushionPolicy.maximumPackets - 1)

        let small = CushionController()
        _ = small.follow(
            .fixed(milliseconds: 5), packetFrames: 512, sampleRate: 48_000, capacityFrames: capacity)
        #expect(small.cushionPackets == 1)
        #expect(
            small.follow(.auto, packetFrames: 512, sampleRate: 48_000, capacityFrames: capacity)
                == false,
            "one packet is within the bounds already")
        #expect(small.cushionPackets == 1)
    }

    // MARK: PlaybackLatencyTest.kt, nextCushion

    @Test func growsTheCushionWhenAnUnderrunHappened() {
        #expect(
            CushionPolicy.next(cushionPackets: 2, cleanIntervals: 0, underrunsInInterval: 1)
                == CushionPolicy.Decision(cushionPackets: 3, cleanIntervals: 0))
    }

    @Test func capsTheCushionAtTheMaximum() {
        #expect(
            CushionPolicy.next(
                cushionPackets: CushionPolicy.maximumPackets, cleanIntervals: 0,
                underrunsInInterval: 7)
                == CushionPolicy.Decision(cushionPackets: CushionPolicy.maximumPackets, cleanIntervals: 0))
    }

    @Test func waitsForCleanIntervalsBeforeShrinking() {
        #expect(
            CushionPolicy.next(cushionPackets: 2, cleanIntervals: 0, underrunsInInterval: 0)
                == CushionPolicy.Decision(cushionPackets: 2, cleanIntervals: 1))
        #expect(
            CushionPolicy.next(cushionPackets: 2, cleanIntervals: 1, underrunsInInterval: 0)
                == CushionPolicy.Decision(cushionPackets: 2, cleanIntervals: 2))
    }

    @Test func shrinksAfterEnoughCleanIntervals() {
        #expect(
            CushionPolicy.next(cushionPackets: 2, cleanIntervals: 2, underrunsInInterval: 0)
                == CushionPolicy.Decision(cushionPackets: 1, cleanIntervals: 0))
    }

    @Test func neverShrinksBelowTheFloor() {
        #expect(
            CushionPolicy.next(
                cushionPackets: CushionPolicy.minimumPackets, cleanIntervals: 2,
                underrunsInInterval: 0)
                == CushionPolicy.Decision(cushionPackets: CushionPolicy.minimumPackets, cleanIntervals: 3))
    }
}
