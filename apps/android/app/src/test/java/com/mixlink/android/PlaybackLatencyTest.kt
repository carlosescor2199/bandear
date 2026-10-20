package com.mixlink.android

import org.junit.Assert.assertEquals
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
