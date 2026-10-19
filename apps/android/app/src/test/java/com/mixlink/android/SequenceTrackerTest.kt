package com.mixlink.android

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Pure JVM coverage for the decision the receive loop makes on every packet: play it or drop it,
 * and how many packets the stream skipped on the way.
 */
class SequenceTrackerTest {
    @Test
    fun playsPacketsInOrderWithNoLoss() {
        val tracker = SequenceTracker()

        assertEquals(0L, tracker.accept(0))
        assertEquals(0L, tracker.accept(1))
        assertEquals(0L, tracker.accept(2))
        assertEquals(0L, tracker.lost)
        assertEquals(0L, tracker.dropped)
    }

    @Test
    fun theFirstPacketStartsTheStreamWhereverItJoins() {
        val tracker = SequenceTracker()

        // A phone that joins a stream already running has lost nothing before its first packet.
        assertEquals(0L, tracker.accept(500))
        assertEquals(0L, tracker.accept(501))
        assertEquals(0L, tracker.lost)
    }

    @Test
    fun countsAGapOnceAndPlaysThePacketAfterIt() {
        val tracker = SequenceTracker()
        tracker.accept(0)

        assertEquals(2L, tracker.accept(3))
        assertEquals(0L, tracker.accept(4))
        assertEquals(2L, tracker.lost)
    }

    @Test
    fun dropsADuplicate() {
        val tracker = SequenceTracker()
        tracker.accept(0)
        tracker.accept(1)

        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(0L, tracker.accept(2))
        assertEquals(0L, tracker.lost)
        assertEquals(1L, tracker.dropped)
    }

    @Test
    fun dropsALatePacketWithoutRewindingTheStream() {
        val tracker = SequenceTracker()
        tracker.accept(0)
        assertEquals(2L, tracker.accept(3))

        // 1 and 2 arrive after 3 was played: too late to play, and their gap is already counted.
        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(SequenceTracker.DROPPED, tracker.accept(2))

        // The stream carries on from 3, so the next packet is in order and nothing is counted twice.
        assertEquals(0L, tracker.accept(4))
        assertEquals(2L, tracker.lost)
        assertEquals(2L, tracker.dropped)
    }

    @Test
    fun aServerRestartStartsANewStreamWithoutCountingLoss() {
        val tracker = SequenceTracker()
        tracker.accept(5_000)

        // The server restarted and its sequence began again at 0.
        assertEquals(0L, tracker.accept(0))
        assertEquals(0L, tracker.accept(1))
        assertEquals(0L, tracker.lost)
        assertEquals(0L, tracker.dropped)
    }

    @Test
    fun onlyAPacketAtLeastTheRestartDistanceBehindStartsANewStream() {
        val tracker = SequenceTracker()
        tracker.accept(1_000)

        assertEquals(SequenceTracker.DROPPED, tracker.accept(1_000 - STREAM_RESTART_PACKETS + 1))
        assertEquals(0L, tracker.accept(1_000 - STREAM_RESTART_PACKETS))
        assertEquals(0L, tracker.lost)
        assertEquals(1L, tracker.dropped)
    }
    @Test
    fun aLatePacketThatFillsAGapIsCountedAsRecoveredButNotPlayed() {
        val tracker = SequenceTracker()
        tracker.accept(0)
        assertEquals(2L, tracker.accept(3))

        // 1 and 2 arrive after 3, each for the first time: a delayed copy (or a reordered packet)
        // filled the gap. Newer audio was already played, so they are still dropped.
        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(SequenceTracker.DROPPED, tracker.accept(2))
        // A second copy of 1 is a duplicate, not another recovery.
        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))

        assertEquals(2L, tracker.recovered)
        assertEquals("the stream still skipped them", 2L, tracker.lost)
        assertEquals(3L, tracker.dropped)
    }

    @Test
    fun aCopyOfAPacketAlreadyPlayedIsNotRecovered() {
        val tracker = SequenceTracker()
        tracker.accept(0)
        tracker.accept(1)
        tracker.accept(2)

        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(SequenceTracker.DROPPED, tracker.accept(0))
        assertEquals(0L, tracker.recovered)
        assertEquals(2L, tracker.dropped)
    }

    @Test
    fun withCopiesTwoBlocksLateOnlyTheLostOriginalIsRecovered() {
        val tracker = SequenceTracker()
        // The server sends each block, then the copy of the one two blocks before it. Original 5 is
        // lost; its copy arrives after original 7.
        val arrivals = mutableListOf<Long>()
        for (block in 0L..10L) {
            if (block != 5L) arrivals += block
            if (block >= 2L) arrivals += block - 2
        }
        arrivals.forEach { tracker.accept(it) }

        assertEquals(1L, tracker.recovered)
        assertEquals(1L, tracker.lost)
    }

    @Test
    fun aGapFillerOlderThanTheWindowIsNotCounted() {
        val tracker = SequenceTracker()
        tracker.accept(0)
        tracker.accept(RECOVERY_WINDOW_PACKETS + 1L)

        // Just outside the window: nothing remembers whether it arrived, so it is not counted.
        assertEquals(SequenceTracker.DROPPED, tracker.accept(0))
        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(0L, tracker.recovered)
        // The oldest sequence still inside the window is.
        assertEquals(SequenceTracker.DROPPED, tracker.accept(2))
        assertEquals(1L, tracker.recovered)
    }

    @Test
    fun aNewStreamStartsWithNothingRemembered() {
        val tracker = SequenceTracker()
        tracker.accept(5_000)
        tracker.accept(5_002)

        // The server restarted: the window starts again with the new stream.
        // (5 001 was skipped in the old one; nothing of it carries over.)
        assertEquals(0L, tracker.accept(0))
        assertEquals(1L, tracker.accept(2))
        assertEquals(SequenceTracker.DROPPED, tracker.accept(1))
        assertEquals(1L, tracker.recovered)
    }
}
