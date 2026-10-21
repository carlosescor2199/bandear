package com.bandear.android.ui.state

import com.bandear.android.ClientMixState
import com.bandear.android.ControlScheduler
import com.bandear.android.ControlSocket
import com.bandear.android.ControlSocketEvents
import com.bandear.android.ControlTransport
import com.bandear.android.MixBank
import com.bandear.android.MixSnapshot
import com.bandear.android.PmonPlayer
import com.bandear.android.ReceiverLaunches
import com.bandear.android.ScheduledTask
import com.bandear.android.ServerDiscovery
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean

/*
 * Fakes for the ViewModel's seams. Everything runs on the test thread: the scheduler runs immediate
 * work at once and keeps delayed work until the test runs it, and the transport's sockets only open,
 * receive or fail when the test says so.
 */

/** An in-memory store with the same save, replace and delete rules as `MixBankStore`. */
internal class FakeStore(
    var name: String = "",
    var channel: Int? = null,
    banks: List<MixBank> = emptyList(),
) : MonitorStore {
    val banks = banks.toMutableList()

    override fun loadBanks(): List<MixBank> = banks.toList()

    override fun saveBank(name: String, snapshot: MixSnapshot) {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) return
        banks.removeAll { it.name == trimmed }
        banks.add(MixBank(trimmed, snapshot))
    }

    override fun deleteBank(name: String): Boolean = banks.removeAll { it.name == name.trim() }

    override fun musicianChannel(): Int? = channel

    override fun setMusicianChannel(channel: Int?) {
        this.channel = channel
    }

    override fun musicianName(): String = name

    override fun setMusicianName(name: String) {
        this.name = name.trim()
    }
}

/**
 * Records what the session asked of the receiver, and keeps what it was built with.
 *
 * It keeps the real receiver's contract by using the real [ReceiverLaunches]: each launch is a
 * receive loop that counts as alive from its launch until its stop report has been handled, and a
 * loop that ends lowers the shared session flag only while it is the latest launch, then reports
 * `onStopped` once, in launch order. Like the real receiver, [stop] only asks the loop to end; the
 * loop reports later, from its own thread. [finishStopping] and [endOnItsOwn] end it, so a test
 * decides when the report arrives.
 */
internal class FakeReceiver(
    val listener: PmonPlayer.Listener,
    val running: AtomicBoolean,
    val mixState: ClientMixState,
) : AudioReceiver {
    /** One launch's receive loop; the gate tells loops apart by identity, as the real one tells threads. */
    private class Loop

    private val gate = ReceiverLaunches<Loop>(running)

    /** Loops launched and not ended yet, oldest first. */
    private val loops = ArrayDeque<Loop>()

    val launches = mutableListOf<Pair<String, Int>>()
    var stops = 0

    /** True after a [stop] whose report [finishStopping] has not delivered yet. */
    var stopping = false
        private set

    /** As `PmonPlayer.isReceiverAlive`: from a launch until its loop's stop report was handled. */
    override fun isAlive(): Boolean = gate.current != null

    override fun launch(host: String, port: Int) {
        launches += host to port
        val loop = Loop()
        loops.addLast(loop)
        gate.launched(loop)
    }

    override fun stop() {
        stops++
        stopping = true
    }

    /** The receive loop asked to stop ends now and reports it, as the real one's `finally` does. */
    fun finishStopping(beforeReport: () -> Unit = {}) {
        check(stopping) { "stop() was not called" }
        stopping = false
        endOnItsOwn(beforeReport)
    }

    /**
     * The oldest running loop ends the way the real one does: the session flag drops if it is the
     * latest launch, then `onStopped`, and only then is the receiver free. [beforeReport] runs
     * between the flag drop and the report, where the main thread can still act before the report
     * takes the session's lock.
     */
    fun endOnItsOwn(beforeReport: () -> Unit = {}) {
        val loop = checkNotNull(loops.removeFirstOrNull()) { "no receive loop is running" }
        gate.finished(loop) {
            beforeReport()
            listener.onStopped()
        }
    }
}

internal class FakeScanner(val listener: ServerDiscovery.Listener) : ServerScanner {
    var starts = 0
    var stops = 0
    var shutdowns = 0

    /** Fails after [shutdown], as the real discovery's executor rejects new work once shut down. */
    override fun start() {
        check(shutdowns == 0) { "start() after shutdown()" }
        starts++
    }

    override fun stop() {
        stops++
    }

    override fun shutdown() {
        shutdowns++
    }
}

/**
 * Runs immediate work at once, or, while [holding], keeps it until [runHeld], as a busy control thread
 * would; keeps delayed work until [runDelayed].
 */
internal class FakeControlScheduler : ControlScheduler {
    private class Delayed(val delayMs: Long, val task: () -> Unit)

    private val delayed = mutableListOf<Delayed>()
    private val held = ArrayDeque<() -> Unit>()
    var shutDown = false
        private set

    /** While true, immediate work waits for [runHeld] instead of running at once. */
    var holding = false

    override fun execute(task: () -> Unit) {
        if (shutDown) return
        if (holding) held.addLast(task) else task()
    }

    /** Runs the immediate work held so far, in order, then whatever it queued in turn. */
    fun runHeld() {
        while (held.isNotEmpty()) held.removeFirst()()
    }

    override fun schedule(delayMs: Long, task: () -> Unit): ScheduledTask? {
        if (shutDown) return null
        val entry = Delayed(delayMs, task)
        delayed += entry
        return ScheduledTask { delayed.remove(entry) }
    }

    fun pendingDelays(): List<Long> = delayed.map { it.delayMs }

    /** Runs every delayed task that is waiting now, as if its delay passed. */
    fun runDelayed() {
        val due = delayed.toList()
        delayed.clear()
        due.forEach { it.task() }
    }

    override fun shutdown() {
        shutDown = true
        delayed.clear()
        held.clear()
    }
}

internal class FakeTransport : ControlTransport {
    val sockets = mutableListOf<FakeSocket>()
    var shutDown = false
        private set

    override fun open(url: String, events: ControlSocketEvents): ControlSocket =
        FakeSocket(url, events).also { sockets += it }

    override fun shutdown() {
        shutDown = true
    }
}

internal class FakeSocket(val url: String, private val events: ControlSocketEvents) : ControlSocket {
    val sent = mutableListOf<String>()
    var closedWith: Pair<Int, String?>? = null
        private set

    override fun send(text: String): Boolean {
        if (closedWith != null) return false
        sent += text
        return true
    }

    override fun close(code: Int, reason: String?): Boolean {
        if (closedWith != null) return false
        closedWith = code to reason
        return true
    }

    override fun cancel() = Unit

    fun open() = events.onOpen(this)

    fun receive(text: String) = events.onMessage(this, text)

    fun fail(message: String) = events.onFailure(this, IOException(message))

    fun sentOfType(type: String): List<JSONObject> =
        sent.map(::JSONObject).filter { it.optString("type") == type }
}
