package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackLatencyTest {
    @Test
    fun countsFramesWrittenButNotPresented() {
        assertEquals(480L, PlaybackLatency.pendingFrames(1_000L, 520L))
    }

    @Test
    fun clampsWhenTheRenderPositionRunsAheadOfTheWriter() {
        assertEquals(0L, PlaybackLatency.pendingFrames(100L, 250L))
    }

    @Test
    fun convertsPendingFramesToMillisecondsAt48k() {
        // 480 frames at 48 kHz is exactly 10 ms.
        assertEquals(10.0, PlaybackLatency.backlogMs(1_000L, 520L, 48_000), 1e-9)
    }

    @Test
    fun reportsZeroBacklogWhenTheTrackIsFullyDrained() {
        assertEquals(0.0, PlaybackLatency.backlogMs(1_000L, 1_000L, 48_000), 1e-9)
    }

    @Test
    fun reportsZeroForAnUnusableSampleRate() {
        assertEquals(0.0, PlaybackLatency.backlogMs(1_000L, 520L, 0), 1e-9)
    }

    @Test
    fun bufferIsTheCushionTimesThePacket() {
        assertEquals(960, PlaybackLatency.targetBufferFrames(480, 2))
        assertEquals(480, PlaybackLatency.targetBufferFrames(480, 1))
    }

    @Test
    fun bufferIsNeverZeroFrames() {
        assertEquals(1, PlaybackLatency.targetBufferFrames(0, 2))
    }

    @Test
    fun theCushionInMillisecondsIsTheBufferItAsksForAtTheTracksRate() {
        // 240-frame packets at 48 kHz are 5 ms each.
        assertEquals(10.0, PlaybackLatency.cushionMs(240, 2, 48_000), 1e-9)
        assertEquals(5.0, PlaybackLatency.cushionMs(240, 1, 48_000), 1e-9)
        assertEquals(40.0, PlaybackLatency.cushionMs(480, 4, 48_000), 1e-9)
    }

    @Test
    fun theCushionIsNotMeasuredBeforeTheFirstPacketOrAtAnUnusableRate() {
        assertTrue(PlaybackLatency.cushionMs(0, 2, 48_000).isNaN())
        assertTrue(PlaybackLatency.cushionMs(240, 2, 0).isNaN())
    }

    @Test
    fun aReportEndsWithTheCushionAskedOfTheTrackAndTheAudioQueued() {
        // Distinct packet frames and cushion packets: 240-frame packets, 3 of them, are 15 ms; using
        // one value twice would give 1200 ms or 0.19 ms.
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 240,
            cushionPackets = 3,
            playbackRate = 48_000,
            timestampRead = true,
            framesWritten = 1_000L,
            presentedFrame = 520L,
        )
        assertEquals(15.0, extras.cushionMs, 1e-9)
        // 480 frames queued at 48 kHz.
        assertEquals(10.0, extras.backlogMs, 1e-9)
    }

    @Test
    fun theCushionReportedCountsThePacketsHeldBackForDelayedCopies() {
        // 240-frame packets at 48 kHz are 5 ms each: a 3-packet cushion and a 2-packet hold are 25 ms;
        // swapped they would read the same, so the track's applied size is used too: 600 frames and 2
        // held packets are 12.5 + 10 ms.
        val asked = PlaybackLatency.reportExtras(
            packetFrames = 240,
            cushionPackets = 3,
            playbackRate = 48_000,
            timestampRead = false,
            framesWritten = 0L,
            presentedFrame = 0L,
            holdPackets = 2,
        )
        assertEquals(25.0, asked.cushionMs, 1e-9)
        val applied = PlaybackLatency.reportExtras(
            packetFrames = 240,
            cushionPackets = 3,
            playbackRate = 48_000,
            timestampRead = false,
            framesWritten = 0L,
            presentedFrame = 0L,
            appliedBufferFrames = 600,
            holdPackets = 2,
        )
        assertEquals(22.5, applied.cushionMs, 1e-9)
        assertTrue(
            "no packet, no cushion, held or not",
            PlaybackLatency.reportExtras(0, 3, 48_000, false, 0L, 0L, holdPackets = 2).cushionMs.isNaN(),
        )
    }

    @Test
    fun aReportBeforeTheFirstPacketHasNoCushionWhateverTheCushionPackets() {
        // No packet sized the buffer yet: the start cushion of 2 packets is not a cushion. Swapped,
        // 2-frame packets would read as a tiny cushion instead of none.
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 0,
            cushionPackets = PlaybackLatency.START_CUSHION_PACKETS,
            playbackRate = 48_000,
            timestampRead = false,
            framesWritten = 0L,
            presentedFrame = 0L,
        )
        assertTrue(extras.cushionMs.isNaN())
        assertTrue(extras.backlogMs.isNaN())
    }

    @Test
    fun theBacklogCountsFramesWrittenPastThePresentedFrameNotTheOtherWayRound() {
        // Swapped, the written count would sit behind the render position and read as 0 ms.
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 256,
            cushionPackets = 2,
            playbackRate = 48_000,
            timestampRead = true,
            framesWritten = 2_400L,
            presentedFrame = 480L,
        )
        assertEquals(40.0, extras.backlogMs, 1e-9)
    }

    @Test
    fun withoutATimestampTheBacklogIsNotMeasuredAndTheCushionStillIs() {
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 256,
            cushionPackets = 2,
            playbackRate = 48_000,
            timestampRead = false,
            framesWritten = 2_400L,
            presentedFrame = 480L,
        )
        assertTrue(extras.backlogMs.isNaN())
        assertEquals(512 * 1000.0 / 48_000, extras.cushionMs, 1e-9)
    }

    @Test
    fun withoutATrackRateTheCushionIsNotMeasured() {
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 256,
            cushionPackets = 2,
            playbackRate = null,
            timestampRead = false,
            framesWritten = 0L,
            presentedFrame = 0L,
        )
        assertTrue(extras.cushionMs.isNaN())
        assertTrue(extras.backlogMs.isNaN())
    }

    @Test
    fun aReportCarriesTheBufferTheTrackAppliedWhenItIsKnown() {
        // Asked for 2 packets of 256 frames (512), the platform kept 768: the phone plays 16 ms, not
        // the 10.67 ms it asked for.
        val extras = PlaybackLatency.reportExtras(
            packetFrames = 256,
            cushionPackets = 2,
            playbackRate = 48_000,
            timestampRead = false,
            framesWritten = 0L,
            presentedFrame = 0L,
            appliedBufferFrames = 768,
        )
        assertEquals(16.0, extras.cushionMs, 1e-9)
        // Unknown (0) falls back to what was asked; before the first packet it is still not measured.
        assertEquals(
            512 * 1000.0 / 48_000,
            PlaybackLatency.reportExtras(256, 2, 48_000, false, 0L, 0L, appliedBufferFrames = 0).cushionMs,
            1e-9,
        )
        assertTrue(PlaybackLatency.reportExtras(0, 2, 48_000, false, 0L, 0L, appliedBufferFrames = 768).cushionMs.isNaN())
    }

    @Test
    fun aLargeTrackThatShrinksToTheWantedBufferIsKept() {
        // One packet of 256 frames wanted at 48 kHz; the minimum track would hold 1 920 frames.
        assertTrue(PlaybackLatency.keepsLowLatency(256, 256, 1_920, 48_000))
        // Rounded up by the platform within the tolerance (5 ms is 240 frames): still kept.
        assertTrue(PlaybackLatency.keepsLowLatency(256, 256 + 240, 100, 48_000))
        assertFalse(PlaybackLatency.keepsLowLatency(256, 256 + 241, 100, 48_000))
    }

    @Test
    fun aLargeTrackThatQueuesNoMoreThanTheMinimumTrackIsKept() {
        // Clamped up to the hardware minimum, but no more than a track opened at the platform
        // minimum (as before the desk could set a buffer) would queue: no worse than before.
        assertTrue(PlaybackLatency.keepsLowLatency(256, 1_920, 1_920, 48_000))
        assertFalse(PlaybackLatency.keepsLowLatency(256, 1_921, 1_920, 48_000))
    }

    @Test
    fun aLargeTrackThatWillNotShrinkIsReopened() {
        // The platform ignored the size: the whole 500 ms capacity stays queued.
        assertFalse(PlaybackLatency.keepsLowLatency(256, 94 * 256, 1_920, 48_000))
        // A refused call (an error code) or a size it cannot read proves nothing: reopen.
        assertFalse(PlaybackLatency.keepsLowLatency(256, -3, 1_920, 48_000))
        assertFalse(PlaybackLatency.keepsLowLatency(256, 0, 1_920, 48_000))
    }

    @Test
    fun growsTheCushionWhenAnUnderrunHappened() {
        assertEquals(
            PlaybackLatency.CushionDecision(3, 0),
            PlaybackLatency.nextCushion(2, 0, 1),
        )
    }

    @Test
    fun capsTheCushionAtTheMaximum() {
        assertEquals(
            PlaybackLatency.CushionDecision(PlaybackLatency.MAX_CUSHION_PACKETS, 0),
            PlaybackLatency.nextCushion(PlaybackLatency.MAX_CUSHION_PACKETS, 0, 7),
        )
    }

    @Test
    fun waitsForCleanIntervalsBeforeShrinking() {
        assertEquals(
            PlaybackLatency.CushionDecision(2, 1),
            PlaybackLatency.nextCushion(2, 0, 0),
        )
        assertEquals(
            PlaybackLatency.CushionDecision(2, 2),
            PlaybackLatency.nextCushion(2, 1, 0),
        )
    }

    @Test
    fun shrinksAfterEnoughCleanIntervals() {
        assertEquals(
            PlaybackLatency.CushionDecision(1, 0),
            PlaybackLatency.nextCushion(2, 2, 0),
        )
    }

    @Test
    fun neverShrinksBelowTheFloor() {
        assertEquals(
            PlaybackLatency.CushionDecision(PlaybackLatency.MIN_CUSHION_PACKETS, 3),
            PlaybackLatency.nextCushion(PlaybackLatency.MIN_CUSHION_PACKETS, 2, 0),
        )
    }
}
