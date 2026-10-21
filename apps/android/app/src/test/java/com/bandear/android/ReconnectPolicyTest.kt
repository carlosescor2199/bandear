package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ReconnectPolicyTest {
    @Test
    fun firstAttemptsBackOffFromHalfASecond() {
        val policy = ReconnectPolicy()

        assertEquals(listOf(500L, 1_000L, 2_000L, 4_000L), List(4) { policy.nextDelayMs() })
    }

    @Test
    fun laterAttemptsWaitTheCapForever() {
        val policy = ReconnectPolicy()
        repeat(4) { policy.nextDelayMs() }

        assertEquals(5_000L, ReconnectPolicy.MAX_DELAY_MS)
        repeat(1_000) { assertEquals(ReconnectPolicy.MAX_DELAY_MS, policy.nextDelayMs()) }
    }

    @Test
    fun aSuccessfulOpenStartsTheScheduleAgain() {
        val policy = ReconnectPolicy()
        repeat(6) { policy.nextDelayMs() }

        policy.reset()

        assertEquals(500L, policy.nextDelayMs())
        assertEquals(1_000L, policy.nextDelayMs())
    }

    @Test
    fun delayForAttemptFollowsTheScheduleAndIsSafeAtTheEdges() {
        assertEquals(500L, ReconnectPolicy.delayForAttempt(0))
        assertEquals(2_000L, ReconnectPolicy.delayForAttempt(2))
        assertEquals(4_000L, ReconnectPolicy.delayForAttempt(3))
        assertEquals(5_000L, ReconnectPolicy.delayForAttempt(4))
        assertEquals(5_000L, ReconnectPolicy.delayForAttempt(Int.MAX_VALUE))
        assertEquals(500L, ReconnectPolicy.delayForAttempt(-1))
    }

    @Test
    fun reconnectsOnlyWhileTheSessionRunsAndWasNotStopped() {
        assertTrue(ReconnectPolicy.shouldReconnect(sessionRunning = true, stopped = false))
        assertFalse(ReconnectPolicy.shouldReconnect(sessionRunning = true, stopped = true))
        assertFalse(ReconnectPolicy.shouldReconnect(sessionRunning = false, stopped = false))
        assertFalse(ReconnectPolicy.shouldReconnect(sessionRunning = false, stopped = true))
    }
}

/**
 * The socket-identity guard: a callback acts only for the connection attempt that is current, so a
 * late event from an older socket can never touch a newer connection.
 */
class ConnectionGenerationsTest {
    @Test
    fun aNewAttemptIsCurrent() {
        val generations = ConnectionGenerations()

        val first = generations.begin()

        assertTrue(generations.isCurrent(first))
    }

    @Test
    fun aNewerAttemptMakesTheOlderOneStale() {
        val generations = ConnectionGenerations()
        val old = generations.begin()

        val replacement = generations.begin()

        assertNotEquals(old, replacement)
        assertFalse(generations.isCurrent(old))
        assertTrue(generations.isCurrent(replacement))
    }

    @Test
    fun theEndOfTheCurrentConnectionIsHandledExactlyOnce() {
        val generations = ConnectionGenerations()
        val current = generations.begin()

        assertTrue(generations.end(current))
        assertFalse(generations.end(current))
        assertFalse(generations.isCurrent(current))
    }

    @Test
    fun aLateEndFromAnOlderSocketLeavesTheCurrentOneAlone() {
        val generations = ConnectionGenerations()
        val old = generations.begin()
        val replacement = generations.begin()

        assertFalse(generations.end(old))
        assertTrue(generations.isCurrent(replacement))
    }

    @Test
    fun anAttemptStartedAfterAnEndIsCurrentAgain() {
        val generations = ConnectionGenerations()
        val lost = generations.begin()
        generations.end(lost)

        val retry = generations.begin()

        assertTrue(generations.isCurrent(retry))
        assertFalse(generations.isCurrent(lost))
    }

    @Test
    fun invalidatingMakesEveryAttemptStale() {
        val generations = ConnectionGenerations()
        val current = generations.begin()

        generations.invalidate()

        assertFalse(generations.isCurrent(current))
        assertFalse(generations.end(current))
    }
}
