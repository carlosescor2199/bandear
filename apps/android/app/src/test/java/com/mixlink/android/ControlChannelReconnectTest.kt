package com.mixlink.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Drives the control channel's connection state machine on the JVM through fake seams: a transport
 * whose sockets the test opens, fails and closes by hand, and a scheduler that only runs what the
 * test tells it to. Everything happens on the test thread, so each step is deterministic.
 */
class ControlChannelReconnectTest {
    private val running = AtomicBoolean(false)
    private val transport = FakeTransport()
    private val scheduler = FakeScheduler()
    private val events = RecordingListener()
    private val channel = ControlChannel(
        listener = events,
        running = running,
        currentSnapshot = { MixSnapshot() },
        moreOfMeEnabled = { false },
        musicianChannels = { emptyList() },
        transport = transport,
        scheduler = scheduler,
    )

    private fun startSession() {
        running.set(true)
        channel.connect(HOST, CONTROL_PORT, UDP_PORT, "Ana")
        scheduler.runImmediate()
    }

    /** Opens [socket] and lets the server prove the connection with its first message. */
    private fun openAndProve(socket: FakeSocket) {
        socket.open()
        socket.receive(CONFIG)
        scheduler.runImmediate()
    }

    @Test
    fun aSessionOpensOneConnectionToTheGivenServer() {
        startSession()

        assertEquals(listOf("ws://$HOST:$CONTROL_PORT"), transport.attempts.map { it.url })
    }

    @Test
    fun everyOpenRegistersFirstAndThenSendsTheMix() {
        startSession()
        val first = transport.attempts.single()
        openAndProve(first)
        first.fail("wifi gone")
        scheduler.runNextDue()

        val second = transport.attempts[1]
        second.open()
        scheduler.runImmediate()

        assertEquals(listOf("register", "mix"), first.sentTypes())
        assertEquals(listOf("register", "mix"), second.sentTypes())
        assertEquals("ws://$HOST:$CONTROL_PORT", second.url)
    }

    @Test
    fun aFailureWhileRunningSchedulesARetryAfterThePolicyDelay() {
        startSession()
        val first = transport.attempts.single()
        openAndProve(first)

        first.fail("connection reset")

        assertEquals(ControlState.RECONNECTING, channel.controlState)
        assertEquals(listOf(500L), scheduler.pendingDelays())
        assertEquals(1, transport.attempts.size)

        scheduler.runNextDue()

        assertEquals(2, transport.attempts.size)
    }

    @Test
    fun aServerClosingTheConnectionIsAnsweredAndRetried() {
        startSession()
        val first = transport.attempts.single()
        openAndProve(first)

        first.serverCloses()

        assertEquals(NORMAL_CLOSE, first.closedWith)
        assertEquals(ControlState.RECONNECTING, channel.controlState)
        // onClosing and the onClosed after it end the same connection: one retry, not two.
        assertEquals(listOf(500L), scheduler.pendingDelays())
    }

    @Test
    fun stopCancelsAPendingRetryAndNothingReconnectsAfterwards() {
        startSession()
        val first = transport.attempts.single()
        openAndProve(first)
        first.fail("connection reset")
        val pending = scheduler.pending().single()

        running.set(false)
        channel.close(NORMAL_CLOSE, "stopped")

        assertTrue(pending.cancelled)
        assertTrue(scheduler.pending().isEmpty())
        // Even an attempt that was already due when Stop came does not connect.
        pending.task()
        scheduler.runImmediate()
        assertEquals(1, transport.attempts.size)
    }

    @Test
    fun stopClosesASocketThatIsStillOpening() {
        startSession()
        val opening = transport.attempts.single()

        running.set(false)
        channel.close(NORMAL_CLOSE, "stopped")
        opening.open()
        opening.fail("late failure")

        assertEquals(NORMAL_CLOSE, opening.closedWith)
        assertTrue(opening.sent.isEmpty())
        assertTrue(scheduler.pending().isEmpty())
        assertEquals(1, transport.attempts.size)
    }

    @Test
    fun aLateCallbackFromAnOlderAttemptDoesNotTouchTheCurrentConnection() {
        startSession()
        val old = transport.attempts.single()
        openAndProve(old)
        old.fail("connection reset")
        scheduler.runNextDue()
        val current = transport.attempts[1]
        openAndProve(current)
        val errorsBefore = events.errors.size

        old.fail("late failure")
        old.serverCloses()
        old.receive("""{"type":"mix_ack"}""")
        channel.sendMixControl()
        scheduler.runImmediate()

        assertEquals(ControlState.CONNECTED, channel.controlState)
        assertTrue(scheduler.pending().isEmpty())
        assertFalse(channel.mixAcknowledged)
        assertEquals(errorsBefore, events.errors.size)
        assertEquals(listOf("register", "mix", "mix"), current.sentTypes())
    }

    @Test
    fun aServerThatClosesRightAfterOpenBacksOffToTheCap() {
        startSession()

        val delays = mutableListOf<Long>()
        repeat(6) {
            val attempt = transport.attempts.last()
            attempt.open()
            scheduler.runImmediate()
            attempt.serverCloses()
            delays += scheduler.pendingDelays().single()
            scheduler.runNextDue()
        }

        assertEquals(listOf(500L, 1_000L, 2_000L, 4_000L, 5_000L, 5_000L), delays)
    }

    @Test
    fun aConnectionThatHearsFromTheServerStartsTheBackoffAgain() {
        startSession()
        repeat(3) {
            val attempt = transport.attempts.last()
            attempt.fail("refused")
            scheduler.runNextDue()
        }
        val healthy = transport.attempts.last()
        openAndProve(healthy)

        healthy.fail("wifi gone")

        assertEquals(listOf(500L), scheduler.pendingDelays())
    }

    @Test
    fun theErrorIsReportedOncePerOutage() {
        startSession()
        repeat(4) {
            transport.attempts.last().fail("refused")
            assertEquals(ControlState.RECONNECTING, channel.controlState)
            scheduler.runNextDue()
        }

        assertEquals(1, events.errors.size)
        assertTrue(events.errors.single().startsWith("WebSocket error: refused"))

        val healthy = transport.attempts.last()
        openAndProve(healthy)
        healthy.fail("wifi gone")
        scheduler.runNextDue()
        transport.attempts.last().fail("still gone")

        assertEquals(2, events.errors.size)
        assertTrue(events.errors.last().startsWith("WebSocket error: wifi gone"))
        assertEquals(ControlState.RECONNECTING, channel.controlState)
    }

    @Test
    fun theReceiverEndingStopsTheRetriesEvenBeforeTheChannelIsClosed() {
        startSession()
        val first = transport.attempts.single()
        openAndProve(first)
        first.fail("connection reset")

        running.set(false)
        scheduler.runNextDue()

        assertEquals(1, transport.attempts.size)
    }

    @Test
    fun theClientPingsOftenEnoughForTheServerButNotAggressively() {
        assertEquals(5L, CONTROL_PING_INTERVAL_SECONDS)
    }

    private class FakeTransport : ControlTransport {
        val attempts = mutableListOf<FakeSocket>()
        var shutDown = false

        override fun open(url: String, events: ControlSocketEvents): ControlSocket =
            FakeSocket(url, events).also { attempts += it }

        override fun shutdown() {
            shutDown = true
        }
    }

    private class FakeSocket(val url: String, private val events: ControlSocketEvents) : ControlSocket {
        val sent = mutableListOf<String>()
        var closedWith: Int? = null
        var cancelled = false

        override fun send(text: String): Boolean {
            sent += text
            return true
        }

        override fun close(code: Int, reason: String?): Boolean {
            if (closedWith != null) return false
            closedWith = code
            return true
        }

        override fun cancel() {
            cancelled = true
        }

        fun sentTypes(): List<String> = sent.map { JSONObject(it).getString("type") }

        fun open() = events.onOpen(this)

        fun receive(text: String) = events.onMessage(this, text)

        fun fail(message: String) = events.onFailure(this, IOException(message))

        fun serverCloses() {
            events.onClosing(this, NORMAL_CLOSE, "")
            events.onClosed(this, NORMAL_CLOSE, "")
        }
    }

    private class FakeScheduler : ControlScheduler {
        private val immediate = ArrayDeque<() -> Unit>()
        private val scheduled = mutableListOf<Scheduled>()

        class Scheduled(val delayMs: Long, val task: () -> Unit) : ScheduledTask {
            var cancelled = false
            var ran = false

            override fun cancel() {
                cancelled = true
            }
        }

        override fun execute(task: () -> Unit) {
            immediate += task
        }

        override fun schedule(delayMs: Long, task: () -> Unit): ScheduledTask =
            Scheduled(delayMs, task).also { scheduled += it }

        override fun shutdown() = Unit

        fun pending(): List<Scheduled> = scheduled.filter { !it.cancelled && !it.ran }

        fun pendingDelays(): List<Long> = pending().map { it.delayMs }

        fun runImmediate() {
            while (immediate.isNotEmpty()) immediate.removeFirst()()
        }

        /** Runs the earliest pending scheduled task, then whatever it queued to run at once. */
        fun runNextDue() {
            val next = pending().minByOrNull { it.delayMs }
            assertNotNull("nothing is scheduled", next)
            next!!.ran = true
            next.task()
            runImmediate()
        }
    }

    private class RecordingListener : ControlChannel.Listener {
        val errors = mutableListOf<String>()
        var opened = 0
        var closed = 0

        override fun onOpened() {
            opened++
        }

        override fun onConfig(channels: Int, groups: List<GroupInfo>, details: List<ChannelInfo>?) = Unit

        override fun onAcknowledged() = Unit

        override fun onClosed() {
            closed++
        }

        override fun onControlError(message: String) {
            errors += message
        }
    }

    private companion object {
        const val HOST = "192.168.1.20"
        const val CONTROL_PORT = 9001
        const val UDP_PORT = 5004
        const val NORMAL_CLOSE = 1000
        const val CONFIG = """{"type":"config","source_channels":2,"groups":[]}"""
    }
}
