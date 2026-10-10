package com.mixlink.android

import org.junit.Assert.assertEquals
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
    fun requestsThreePacketIntervalsOfPlaybackCushion() {
        assertEquals(1_440, PlaybackLatency.targetBufferFrames(480))
    }

    @Test
    fun neverRequestsAZeroFrameBuffer() {
        assertEquals(1, PlaybackLatency.targetBufferFrames(0))
    }
}
