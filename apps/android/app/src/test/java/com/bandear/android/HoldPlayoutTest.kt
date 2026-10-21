package com.bandear.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The receive loop's wiring of the hold and the link report ([HoldPlayout]), wake-up by wake-up as
 * [PmonPlayer] drives it, and the `stats` it ends up sending: a packet a delayed copy brought is
 * played, counted in `packets_received` and in `recovered`, never in `packets_lost`.
 */
class HoldPlayoutTest {
    private fun playout() = HoldPlayout(ReorderHold<Long>(), LinkStatsAccumulator(startedAtMs = 0L))

    /** One wake-up with a packet, as the loop runs it: the hold, the offer, then every packet due. */
    private fun HoldPlayout<Long>.arrive(sequence: Long, nowMs: Long, holdPackets: Int): List<Long> {
        wake(holdPackets)
        if (!offer(sequence, sequence)) return emptyList()
        return drain(nowMs)
    }

    /** One wake-up with no packet: the timeout, the hold, then every packet now due. */
    private fun HoldPlayout<Long>.timeOut(nowMs: Long, holdPackets: Int): List<Long> {
        onTimeout()
        wake(holdPackets)
        return drain(nowMs)
    }

    private fun HoldPlayout<Long>.drain(nowMs: Long): List<Long> {
        val played = mutableListOf<Long>()
        while (true) played += nextToPlay(nowMs) ?: break
        return played
    }

    @Test
    fun aDelayedCopyIsPlayedInItsPlaceAndReportedAsReceivedAndRecoveredNotLost() {
        val playout = playout()
        assertEquals(emptyList<Long>(), playout.arrive(10, nowMs = 100, holdPackets = 1))
        assertEquals(listOf(10L), playout.arrive(11, nowMs = 110, holdPackets = 1))
        // 12 is lost on the way; 13 comes, and 12's copy after it, still in time.
        assertEquals(listOf(11L), playout.arrive(13, nowMs = 120, holdPackets = 1))
        assertEquals(listOf(12L), playout.arrive(12, nowMs = 130, holdPackets = 1))
        // Nothing else comes: the timeout plays what is held.
        assertEquals(listOf(13L), playout.timeOut(nowMs = 1_000, holdPackets = 1))

        val report = playout.poll(nowMs = 1_110, cumulativeUnderruns = 0)
        assertNotNull(report)
        report!!
        assertEquals(4L, report.packetsReceived)
        assertEquals(0L, report.packetsLost)
        assertEquals(1L, report.recovered)
        assertEquals(1L, report.receiveTimeouts)
        assertEquals(0L, playout.lost)

        // The same `stats` the server's protocol test reads
        // (`protocol::tests::the_stats_a_phone_sends_after_a_copy_filled_a_gap`): keep them equal.
        val sent = JSONObject(buildStatsMessage(report))
        assertTrue("$sent", sent.similar(JSONObject(STATS_AFTER_A_COPY_FILLED_A_GAP)))
    }

    @Test
    fun aCopyTooLateForTheHoldIsDroppedAndItsOriginalCountedLostOnce() {
        val playout = playout()
        playout.arrive(0, nowMs = 100, holdPackets = 1)
        assertEquals(listOf(0L), playout.arrive(2, nowMs = 110, holdPackets = 1))
        // 3 makes 1 past its deadline: given up on, and 2 plays with the gap before it.
        assertEquals(listOf(2L), playout.arrive(3, nowMs = 120, holdPackets = 1))
        assertEquals(emptyList<Long>(), playout.arrive(1, nowMs = 130, holdPackets = 1))

        val report = playout.poll(nowMs = 1_110, cumulativeUnderruns = 0)!!
        assertEquals(2L, report.packetsReceived)
        assertEquals(1L, report.packetsLost)
        assertEquals("a copy past the hold is not played, so not recovered", 0L, report.recovered)
        assertEquals(1L, playout.lost)
    }

    @Test
    fun theHoldTheDeskAsksForAppliesFromTheNextWakeUp() {
        val playout = playout()
        assertEquals(emptyList<Long>(), playout.arrive(0, nowMs = 100, holdPackets = 2))
        assertEquals(emptyList<Long>(), playout.arrive(1, nowMs = 110, holdPackets = 2))
        assertEquals(2, playout.holdPackets)
        // The desk turns the delay off: what is held is now due, and the next packet plays at once.
        assertEquals(listOf(0L, 1L, 2L), playout.arrive(2, nowMs = 120, holdPackets = 0))
        assertEquals(0, playout.holdPackets)
        assertEquals(listOf(3L), playout.arrive(3, nowMs = 130, holdPackets = 0))
    }

    @Test
    fun recoveredIsReportedPerIntervalAndNeverMoreThanThePacketsPlayed() {
        val playout = playout()
        playout.arrive(0, nowMs = 100, holdPackets = 1)
        playout.arrive(2, nowMs = 110, holdPackets = 1)
        playout.arrive(1, nowMs = 120, holdPackets = 1)
        playout.arrive(3, nowMs = 130, holdPackets = 1)
        val first = playout.poll(nowMs = 1_110, cumulativeUnderruns = 0)!!
        assertEquals(1L, first.recovered)
        assertTrue(first.recovered <= first.packetsReceived)

        playout.arrive(4, nowMs = 1_200, holdPackets = 1)
        val second = playout.poll(nowMs = 2_110, cumulativeUnderruns = 0)!!
        assertEquals("the difference since the last report", 0L, second.recovered)
        assertNull(playout.poll(nowMs = 2_120, cumulativeUnderruns = 0))
        assertFalse(second.packetsLost > 0)
    }

    companion object {
        /** A one-second report: four packets played, one of them brought by a delayed copy. */
        const val STATS_AFTER_A_COPY_FILLED_A_GAP =
            """{"type":"stats","interval_ms":1000,"packets_received":4,"packets_lost":0,"underruns":0,"receive_timeouts":1,"recovered":1}"""
    }
}
