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
}
