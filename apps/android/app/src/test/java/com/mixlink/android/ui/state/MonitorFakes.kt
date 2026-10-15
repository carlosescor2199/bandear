package com.mixlink.android.ui.state

import com.mixlink.android.ClientMixState
import com.mixlink.android.ControlScheduler
import com.mixlink.android.ControlSocket
import com.mixlink.android.ControlSocketEvents
import com.mixlink.android.ControlTransport
import com.mixlink.android.MixBank
import com.mixlink.android.MixSnapshot
import com.mixlink.android.PmonPlayer
import com.mixlink.android.ScheduledTask
import com.mixlink.android.ServerDiscovery
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

/** Records what the session asked of the receiver, and keeps what it was built with. */
internal class FakeReceiver(
    val listener: PmonPlayer.Listener,
    val running: AtomicBoolean,
    val mixState: ClientMixState,
) : AudioReceiver {
    val launches = mutableListOf<Pair<String, Int>>()
    var stops = 0
    var alive = false

    override fun isAlive(): Boolean = alive

    override fun launch(host: String, port: Int) {
        launches += host to port
    }

    override fun stop() {
        stops++
    }

    /** Ends the receive loop the way the real one does: the session flag drops, then onStopped. */
    fun endOnItsOwn() {
        running.set(false)
        listener.onStopped()
    }
}

internal class FakeScanner(val listener: ServerDiscovery.Listener) : ServerScanner {
    var starts = 0
    var stops = 0
    var shutdowns = 0

    override fun start() {
        starts++
    }

    override fun stop() {
        stops++
    }

    override fun shutdown() {
        shutdowns++
    }
}

/** Runs immediate work at once; keeps delayed work until [runDelayed]. */
internal class FakeControlScheduler : ControlScheduler {
    private class Delayed(val delayMs: Long, val task: () -> Unit)

    private val delayed = mutableListOf<Delayed>()
    var shutDown = false
        private set

    override fun execute(task: () -> Unit) {
        if (!shutDown) task()
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
