package com.mixlink.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The playback cushion the receive loop keeps: adapted once per second of audio on Auto, held at the
 * desk's fixed length otherwise ([CushionController]), and the conversion of a fixed length in
 * milliseconds to whole packets ([PlaybackLatency.fixedCushionPackets]).
 */
class CushionControllerTest {
    /** The track holds the longest fixed buffer at 48 kHz in 256-frame packets. */
    private val capacity = PlaybackLatency.trackCapacityFrames(48_000, 256)

    @Test
    fun theTrackHoldsTheLongestFixedBufferInWholePackets() {
        // 500 ms at 48 kHz is 24 000 frames: 93.75 packets of 256, so 94 of them.
        assertEquals(94 * 256, PlaybackLatency.trackCapacityFrames(48_000, 256))
        assertEquals(24_000, PlaybackLatency.trackCapacityFrames(48_000, 240))
        assertEquals(22_050, PlaybackLatency.trackCapacityFrames(44_100, 0))
    }

    @Test
    fun aFixedLengthIsRoundedUpToWholePackets() {
        // 40 ms at 48 kHz is 1 920 frames: 7.5 packets of 256 frames, so 8; exactly 8 of 240.
        assertEquals(8, PlaybackLatency.fixedCushionPackets(40, 256, 48_000, capacity))
        assertEquals(8, PlaybackLatency.fixedCushionPackets(40, 240, 48_000, capacity))
        // One frame past a whole packet is one packet more.
        assertEquals(2, PlaybackLatency.fixedCushionPackets(6, 256, 48_000, capacity))
        // Never below one packet.
        assertEquals(1, PlaybackLatency.fixedCushionPackets(5, 512, 48_000, capacity))
        // The longest fits whole. Distinct arguments: swapping the packet frames and the rate would
        // give one packet.
        assertEquals(94, PlaybackLatency.fixedCushionPackets(500, 256, 48_000, capacity))
    }

    @Test
    fun aFixedLengthIsClampedToWhatTheTrackHolds() {
        // A track that holds 4 800 frames holds 18 whole packets of 256.
        assertEquals(18, PlaybackLatency.fixedCushionPackets(500, 256, 48_000, 4_800))
        // Never below one packet, even for a track smaller than a packet.
        assertEquals(1, PlaybackLatency.fixedCushionPackets(500, 256, 48_000, 100))
        // Before the first packet sized anything there is nothing to convert: one packet.
        assertEquals(1, PlaybackLatency.fixedCushionPackets(40, 0, 48_000, capacity))
    }

    @Test
    fun anAdjustmentIsDueOnceASecondOfAudioWasPlayedWhateverThePacketSize() {
        val small = CushionController()
        // 256-frame packets at 48 kHz are 187.5 a second: the 188th completes the second.
        repeat(187) { assertFalse("packet ${it + 1}", small.onPlayed(256, 48_000)) }
        assertTrue(small.onPlayed(256, 48_000))
        // What passed the second counts towards the next: 375 packets are exactly two seconds.
        repeat(186) { assertFalse("packet ${it + 189}", small.onPlayed(256, 48_000)) }
        assertTrue(small.onPlayed(256, 48_000))

        val large = CushionController()
        repeat(99) { assertFalse(large.onPlayed(480, 48_000)) }
        assertTrue("100 packets of 10 ms", large.onPlayed(480, 48_000))
    }

    @Test
    fun onAutoTheCushionAdaptsAsBefore() {
        val cushion = CushionController()
        assertEquals(PlaybackLatency.START_CUSHION_PACKETS, cushion.cushionPackets)
        assertEquals(PlaybackLatency.DESK_BUFFER_AUTO, cushion.fixedMs)

        assertTrue("an underrun grows it", cushion.adjust(1))
        assertEquals(3, cushion.cushionPackets)
        assertFalse(cushion.adjust(1))
        assertFalse(cushion.adjust(1))
        assertTrue("three clean seconds shrink it", cushion.adjust(1))
        assertEquals(2, cushion.cushionPackets)
    }

    @Test
    fun aFixedBufferHoldsTheCushionWhateverTheUnderruns() {
        val cushion = CushionController()

        assertTrue(cushion.follow(40, 256, 48_000, capacity))
        assertEquals(8, cushion.cushionPackets)
        assertEquals(40, cushion.fixedMs)
        for (underruns in listOf(3, 9, 9, 9, 9, 20)) {
            assertFalse(cushion.adjust(underruns))
            assertEquals(8, cushion.cushionPackets)
        }
        assertFalse("the same length again changes nothing", cushion.follow(40, 256, 48_000, capacity))
        assertTrue(cushion.follow(100, 256, 48_000, capacity))
        assertEquals(19, cushion.cushionPackets)
    }

    @Test
    fun autoResumesAdaptingFromTheCushionPlayedWithinItsBounds() {
        val cushion = CushionController()
        cushion.follow(40, 256, 48_000, capacity)
        // Underruns while fixed are the fixed buffer's: Auto does not grow for them.
        cushion.adjust(5)

        assertTrue(cushion.follow(PlaybackLatency.DESK_BUFFER_AUTO, 256, 48_000, capacity))
        assertEquals(PlaybackLatency.MAX_CUSHION_PACKETS, cushion.cushionPackets)
        assertEquals(PlaybackLatency.DESK_BUFFER_AUTO, cushion.fixedMs)
        assertFalse(cushion.adjust(5))
        assertFalse(cushion.adjust(5))
        assertTrue("shrinks after three clean seconds", cushion.adjust(5))
        assertEquals(PlaybackLatency.MAX_CUSHION_PACKETS - 1, cushion.cushionPackets)

        val small = CushionController()
        small.follow(5, 512, 48_000, capacity)
        assertEquals(1, small.cushionPackets)
        assertFalse("one packet is within the bounds already", small.follow(PlaybackLatency.DESK_BUFFER_AUTO, 512, 48_000, capacity))
        assertEquals(1, small.cushionPackets)
    }
}
