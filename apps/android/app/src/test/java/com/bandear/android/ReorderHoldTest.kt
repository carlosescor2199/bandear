package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure JVM coverage for the decision the receive loop makes on every packet: hold it, play it (and
 * any held packet now due) in sequence order, or drop it; how many packets the stream skipped; and
 * which late packets a delayed copy brought in time to be played.
 */
class ReorderHoldTest {
    /** One arrival: offers [sequence] and answers the sequences played because of it, in order. */
    private fun ReorderHold<Long>.arrive(sequence: Long): List<Long> {
        offer(sequence, sequence)
        return drain()
    }

    private fun ReorderHold<Long>.drain(): List<Long> {
        val played = mutableListOf<Long>()
        while (true) played += next() ?: break
        return played
    }

    private fun hold(packets: Int) = ReorderHold<Long>().apply { holdPackets = packets }

    // With no hold the phone plays as it always did.

    @Test
    fun withNoHoldPacketsInOrderPlayAsTheyArrive() {
        val hold = hold(0)

        assertEquals(listOf(0L), hold.arrive(0))
        assertEquals(listOf(1L), hold.arrive(1))
        assertEquals(listOf(2L), hold.arrive(2))
        assertEquals(0L, hold.lost)
        assertEquals(0L, hold.dropped)
    }

    @Test
    fun theFirstPacketStartsTheStreamWhereverItJoins() {
        val hold = hold(0)

        // A phone that joins a stream already running has lost nothing before its first packet.
        assertEquals(listOf(500L), hold.arrive(500))
        assertEquals(0L, hold.lostBefore)
        assertEquals(listOf(501L), hold.arrive(501))
        assertEquals(0L, hold.lost)
    }

    @Test
    fun withNoHoldAGapIsCountedOnceAndThePacketAfterItPlays() {
        val hold = hold(0)
        hold.arrive(0)

        assertEquals(listOf(3L), hold.arrive(3))
        assertEquals(2L, hold.lostBefore)
        assertEquals(listOf(4L), hold.arrive(4))
        assertEquals(0L, hold.lostBefore)
        assertEquals(2L, hold.lost)
    }

    @Test
    fun withNoHoldADuplicateIsDropped() {
        val hold = hold(0)
        hold.arrive(0)
        hold.arrive(1)

        assertFalse(hold.offer(1, 1))
        assertEquals(listOf(2L), hold.arrive(2))
        assertEquals(0L, hold.lost)
        assertEquals(1L, hold.dropped)
    }

    @Test
    fun withNoHoldALatePacketIsDroppedWithoutRewindingTheStream() {
        val hold = hold(0)
        hold.arrive(0)
        hold.arrive(3)

        // 1 and 2 arrive after 3 was played: too late to play, and their gap is already counted.
        assertEquals(emptyList<Long>(), hold.arrive(1))
        assertEquals(emptyList<Long>(), hold.arrive(2))

        assertEquals(listOf(4L), hold.arrive(4))
        assertEquals(2L, hold.lost)
        assertEquals(2L, hold.dropped)
        assertEquals("a packet never played is never recovered", 0L, hold.recovered)
    }

    @Test
    fun aServerRestartStartsANewStreamWithoutCountingLoss() {
        val hold = hold(0)
        hold.arrive(5_000)

        // The server restarted and its sequence began again at 0.
        assertEquals(listOf(0L), hold.arrive(0))
        assertEquals(listOf(1L), hold.arrive(1))
        assertEquals(0L, hold.lost)
        assertEquals(0L, hold.dropped)
    }

    @Test
    fun onlyAPacketAtLeastTheRestartDistanceBehindStartsANewStream() {
        val hold = hold(0)
        hold.arrive(1_000)

        assertEquals(emptyList<Long>(), hold.arrive(1_000 - STREAM_RESTART_PACKETS + 1))
        assertEquals(listOf(1_000 - STREAM_RESTART_PACKETS), hold.arrive(1_000 - STREAM_RESTART_PACKETS))
        assertEquals(0L, hold.lost)
        assertEquals(1L, hold.dropped)
    }

    @Test
    fun withNoHoldAnImmediateCopyFillsItsGapInOrder() {
        val hold = hold(0)
        // The server sends each block, then its copy right away. Original 2 is lost: its copy arrives
        // before 3, so it is simply played in its place, and the second copies are dropped.
        val arrivals = mutableListOf<Long>()
        for (block in 0L..4L) {
            if (block != 2L) arrivals += block
            arrivals += block
        }
        val played = arrivals.flatMap { hold.arrive(it) }

        assertEquals(listOf(0L, 1L, 2L, 3L, 4L), played)
        assertEquals(0L, hold.lost)
        assertEquals(4L, hold.dropped)
        assertEquals("it cannot be told from its original", 0L, hold.recovered)
    }

    // With a hold the phone plays a few packets late, so a late copy can fill its gap.

    @Test
    fun withAHoldPacketsInOrderPlayThatManyPacketsLate() {
        val hold = hold(2)

        assertEquals(emptyList<Long>(), hold.arrive(0))
        assertEquals(emptyList<Long>(), hold.arrive(1))
        assertEquals(listOf(0L), hold.arrive(2))
        assertEquals(listOf(1L), hold.arrive(3))
        assertEquals(listOf(2L), hold.arrive(4))
        assertEquals(0L, hold.lost)
    }

    @Test
    fun aDelayedCopyFillsTheGapOfItsLostOriginalAndIsCountedAsRecovered() {
        val hold = hold(2)
        // The server sends each block, then the copy of the one two blocks before it. Original 5 is
        // lost; its copy arrives right after original 7.
        val arrivals = mutableListOf<Long>()
        for (block in 0L..10L) {
            if (block != 5L) arrivals += block
            if (block >= 2L) arrivals += block - 2
        }
        val played = arrivals.flatMap { hold.arrive(it) } + run {
            hold.release()
            hold.drain()
        }

        assertEquals((0L..10L).toList(), played)
        assertEquals(0L, hold.lost)
        assertEquals(1L, hold.recovered)
        assertEquals("every other copy is a duplicate or too late", 8L, hold.dropped)
    }

    @Test
    fun theRecoveredPacketIsMarkedWhenItIsPlayed() {
        val hold = hold(1)
        assertEquals(emptyList<Long>(), hold.arrive(0))
        assertEquals("0 is due, 1 may still come", listOf(0L), hold.arrive(2))

        assertTrue(hold.offer(1, 1))
        assertEquals(1L, hold.next())
        assertTrue(hold.lastRecovered)
        assertEquals(0L, hold.lostBefore)
        assertEquals(null, hold.next())
        assertEquals(listOf(2L), hold.arrive(3))
        assertFalse(hold.lastRecovered)
    }

    @Test
    fun aGapNotFilledBeforeItsDeadlineIsSkippedAndCountedLost() {
        val hold = hold(2)
        listOf(0L, 1L, 2L).forEach { hold.arrive(it) }

        // 3 is lost, and its copy too: once 6 arrives it can no longer come in time.
        assertEquals(listOf(1L, 2L), hold.arrive(4))
        assertEquals("3 may still come as a copy", emptyList<Long>(), hold.arrive(5))
        assertEquals(listOf(4L), hold.arrive(6))
        assertEquals(1L, hold.lostBefore)
        // Its copy, arriving now, is too late to play.
        assertEquals(emptyList<Long>(), hold.arrive(3))
        assertEquals(1L, hold.lost)
        assertEquals(0L, hold.recovered)
    }

    @Test
    fun aDuplicateOfAHeldPacketIsDropped() {
        val hold = hold(2)
        hold.arrive(0)
        hold.arrive(1)

        assertFalse(hold.offer(1, 1))
        assertFalse(hold.offer(0, 0))
        assertEquals(2L, hold.dropped)
        assertEquals(listOf(0L), hold.arrive(2))
    }

    @Test
    fun aServerRestartDropsWhatTheOldStreamLeftHeld() {
        val hold = hold(2)
        hold.arrive(5_000)
        hold.arrive(5_001)

        // The server restarted: its new stream starts over, held as any other.
        assertEquals(emptyList<Long>(), hold.arrive(0))
        assertEquals(emptyList<Long>(), hold.arrive(1))
        assertEquals(listOf(0L), hold.arrive(2))
        assertEquals(0L, hold.lostBefore)
        assertEquals(0L, hold.lost)
    }

    @Test
    fun aLongJumpForwardPlaysWhatWasHeldThenCountsTheGapOnce() {
        val hold = hold(2)
        hold.arrive(0)
        hold.arrive(1)

        assertEquals(listOf(0L, 1L), hold.arrive(1_000))
        assertEquals(emptyList<Long>(), hold.arrive(1_001))
        assertEquals(listOf(1_000L), hold.arrive(1_002))
        assertEquals(998L, hold.lostBefore)
        assertEquals(998L, hold.lost)
    }

    @Test
    fun aPacketThatArrivedFarAheadDuringABurstIsPlayedInItsPlace() {
        val hold = hold(4)
        // The server sends each block, then the copy of the one four blocks before it. Originals 6
        // and 7 are lost, so 8 arrives too far ahead of what is held to be held with it at once.
        val arrivals = mutableListOf<Long>()
        for (block in 0L..15L) {
            if (block != 6L && block != 7L) arrivals += block
            if (block >= 4L) arrivals += block - 4
        }
        val played = mutableListOf<Long>()
        fun drainChecked() {
            while (true) {
                played += hold.next() ?: break
                assertTrue("lostBefore ${hold.lostBefore}", hold.lostBefore >= 0L)
            }
        }
        arrivals.forEach {
            hold.offer(it, it)
            drainChecked()
        }
        hold.release()
        drainChecked()

        assertEquals("every packet once, in order, 8 included", (0L..15L).toList(), played)
        assertEquals(0L, hold.lost)
        assertEquals("the copies of 6 and 7", 2L, hold.recovered)

        // Nothing is left behind to rewind the stream later.
        hold.release()
        hold.holdPackets = 0
        drainChecked()
        assertEquals(listOf(16L), hold.arrive(16))
        assertEquals((0L..15L).toList(), played)
        assertEquals(0L, hold.lostBefore)
        assertEquals(0L, hold.lost)
    }

    @Test
    fun releasingPlaysEverythingHeldInOrder() {
        val hold = hold(2)
        listOf(0L, 1L, 2L).forEach { hold.arrive(it) }

        // No packet for a while: what is held is played rather than kept stale.
        hold.release()
        assertEquals(listOf(1L, 2L), hold.drain())
        assertEquals(emptyList<Long>(), hold.arrive(3))
        assertEquals(emptyList<Long>(), hold.arrive(4))
        assertEquals(listOf(3L), hold.arrive(5))
    }

    @Test
    fun aShorterHoldPlaysWhatIsNowDue() {
        val hold = hold(3)
        listOf(0L, 1L, 2L).forEach { hold.arrive(it) }

        hold.holdPackets = 0
        assertEquals(listOf(0L, 1L, 2L), hold.drain())
        assertEquals(listOf(3L), hold.arrive(3))
    }

    @Test
    fun theHoldStaysWithinWhatThePhoneKeepsRoomFor() {
        val hold = ReorderHold<Long>()
        assertEquals(0, hold.holdPackets)
        hold.holdPackets = 9
        assertEquals(MAX_HOLD_PACKETS, hold.holdPackets)
        hold.holdPackets = -1
        assertEquals(0, hold.holdPackets)
    }
}
