package com.mixlink.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure JVM coverage for the once-a-second link report: when it is due, what interval it claims and
 * which counts it carries. Time is injected, so nothing here waits.
 */
class LinkStatsTest {
    @Test
    fun noReportIsDueBeforeTheIntervalHasPassed() {
        val stats = LinkStatsAccumulator(startedAtMs = 10_000)
        stats.onPacket(lostBefore = 0)

        assertFalse(stats.isDue(10_999))
        assertNull(stats.poll(10_999, cumulativeUnderruns = 0))
    }

    @Test
    fun reportsTheIntervalCountsOnceTheIntervalHasPassed() {
        val stats = LinkStatsAccumulator(startedAtMs = 10_000)
        repeat(3) { stats.onPacket(lostBefore = 0) }
        stats.onPacket(lostBefore = 2)
        stats.onReceiveTimeout()

        assertTrue(stats.isDue(11_000))
        assertEquals(
            LinkStatsReport(
                intervalMs = 1_000,
                packetsReceived = 4,
                packetsLost = 2,
                underruns = 0,
                receiveTimeouts = 1,
            ),
            stats.poll(11_000, cumulativeUnderruns = 0),
        )
    }

    @Test
    fun aTimeoutBeforeTheFirstPacketIsNotCounted() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)

        // The receiver starts before the server knows where to send audio.
        stats.onReceiveTimeout()
        stats.onReceiveTimeout()

        assertEquals(0L, stats.poll(1_000, cumulativeUnderruns = 0)?.receiveTimeouts)
    }

    @Test
    fun aTimeoutAfterTheFirstPacketIsCounted() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)
        stats.onReceiveTimeout()
        stats.onPacket(lostBefore = 0)

        stats.onReceiveTimeout()

        val report = stats.poll(1_000, cumulativeUnderruns = 0)
        assertEquals(1L, report?.receiveTimeouts)
        assertEquals(1L, report?.packetsReceived)
    }

    @Test
    fun theStreamStaysStartedAcrossReports() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)
        stats.onPacket(lostBefore = 0)
        stats.poll(1_000, cumulativeUnderruns = 0)

        // Audio stopped after the stream started: every wait now says something about the link.
        stats.onReceiveTimeout()

        assertEquals(1L, stats.poll(2_000, cumulativeUnderruns = 0)?.receiveTimeouts)
    }

    @Test
    fun aLatePollReportsTheRealElapsedInterval() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)

        // The loop can only poll between receives, up to one receive timeout late.
        assertEquals(1_250L, stats.poll(1_250, cumulativeUnderruns = 0)?.intervalMs)
    }

    @Test
    fun theCountsStartAgainAfterAReport() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)
        repeat(100) { stats.onPacket(lostBefore = 0) }
        stats.onPacket(lostBefore = 5)
        stats.onReceiveTimeout()
        stats.poll(1_000, cumulativeUnderruns = 0)

        stats.onPacket(lostBefore = 0)

        // The next interval runs from the last report, not from the start.
        assertNull(stats.poll(1_999, cumulativeUnderruns = 0))
        assertEquals(
            LinkStatsReport(
                intervalMs = 1_000,
                packetsReceived = 1,
                packetsLost = 0,
                underruns = 0,
                receiveTimeouts = 0,
            ),
            stats.poll(2_000, cumulativeUnderruns = 0),
        )
    }

    @Test
    fun underrunsAreTheDifferenceOfTheTracksCumulativeCount() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)

        assertEquals(3L, stats.poll(1_000, cumulativeUnderruns = 3)?.underruns)
        assertEquals(0L, stats.poll(2_000, cumulativeUnderruns = 3)?.underruns)
        assertEquals(4L, stats.poll(3_000, cumulativeUnderruns = 7)?.underruns)
    }

    @Test
    fun theIntervalIsClampedToWhatTheServerAccepts() {
        val stats = LinkStatsAccumulator(startedAtMs = 0)

        // A phone that slept for two minutes still sends an interval the server accepts.
        assertEquals(
            MAX_LINK_STATS_INTERVAL_MS,
            stats.poll(120_000, cumulativeUnderruns = 0)?.intervalMs,
        )
    }
}
