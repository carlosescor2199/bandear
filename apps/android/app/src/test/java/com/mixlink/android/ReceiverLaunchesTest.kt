package com.mixlink.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicBoolean

class ReceiverLaunchesTest {
    private val running = AtomicBoolean(false)
    private val launches = ReceiverLaunches<String>(running)
    private var reports = 0

    /** What a caller does to start a session: raise the flag, then launch. */
    private fun startSession(receiver: String) {
        running.set(true)
        launches.launched(receiver)
    }

    @Test
    fun aReceiverThatEndsOnItsOwnEndsTheSession() {
        startSession("first")

        launches.finished("first") { reports++ }

        assertFalse(running.get())
        assertEquals(1, reports)
        assertNull(launches.current)
    }

    @Test
    fun aFinishingReceiverCannotEndANewerSession() {
        startSession("first")
        running.set(false) // Stop.
        startSession("second") // A new session before the first loop has finished.

        launches.finished("first") { reports++ }

        assertTrue("the newer session keeps running", running.get())
        assertEquals("the older loop still reports its own stop", 1, reports)
        assertSame("second", launches.current)
    }

    @Test
    fun theReceiverIsReleasedOnlyAfterTheFlagIsDownAndItsStopIsReported() {
        startSession("first")

        launches.finished("first") {
            // While the stop is being reported the flag is already down and the receiver is still
            // current, so no caller can launch into a session the old loop is about to end.
            assertFalse(running.get())
            assertSame("first", launches.current)
            reports++
        }

        assertEquals(1, reports)
        assertNull(launches.current)
    }

    @Test
    fun theNextSessionStartedAfterTheReleaseKeepsRunning() {
        startSession("first")
        launches.finished("first") { reports++ }

        startSession("second")

        assertTrue(running.get())
        assertSame("second", launches.current)
    }

    @Test
    fun aReportThatFailsStillReleasesTheReceiver() {
        startSession("first")

        val failure = runCatching { launches.finished("first") { error("listener failed") } }

        assertTrue(failure.isFailure)
        assertFalse(running.get())
        assertNull(launches.current)
    }
}
