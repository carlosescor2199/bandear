package com.bandear.android

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

/**
 * How often the client pings the server on an open control connection, in seconds. A pong that has
 * not come back by the next ping fails the socket, so a half-open link (Wi-Fi gone without any
 * close) is noticed within about two intervals and reconnection starts, instead of waiting for a
 * write to fail. Five seconds keeps that inside the server's own ten-second silence limit without
 * pinging a busy stage network more than it needs.
 */
internal const val CONTROL_PING_INTERVAL_SECONDS = 5L

/*
 * The seams of the control channel: how it reaches the network and how it runs work later. The
 * production implementations below wrap OkHttp and a single-thread executor; the JVM tests drive the
 * channel's connection state machine through fakes of the same interfaces.
 */

/** Opens control connections. Each socket reports to the [ControlSocketEvents] it was opened with. */
internal interface ControlTransport {
    /** Starts opening [url]; may throw for a URL that cannot be used at all. */
    fun open(url: String, events: ControlSocketEvents): ControlSocket

    /** Releases the transport's threads. Called once, when the channel shuts down. */
    fun shutdown()
}

/** One control connection, open or still opening. */
internal interface ControlSocket {
    /** Queues [text]; false once the socket is closing or failed. */
    fun send(text: String): Boolean

    /** Starts a graceful close; false if one was already started or the socket failed. */
    fun close(code: Int, reason: String?): Boolean

    /** Drops the connection at once, without a close handshake. */
    fun cancel()
}

/** What happens to one control socket, in the order the transport reports it. */
internal interface ControlSocketEvents {
    fun onOpen(socket: ControlSocket)
    fun onMessage(socket: ControlSocket, text: String)
    fun onClosing(socket: ControlSocket, code: Int, reason: String)
    fun onFailure(socket: ControlSocket, error: Throwable)
    fun onClosed(socket: ControlSocket, code: Int, reason: String)
}

/** Runs the channel's work in order, now or after a delay. Work handed over after [shutdown] is dropped. */
internal interface ControlScheduler {
    fun execute(task: () -> Unit)

    /** Runs [task] after [delayMs]; null when the scheduler has already shut down. */
    fun schedule(delayMs: Long, task: () -> Unit): ScheduledTask?

    fun shutdown()
}

/** A task waiting in a [ControlScheduler]. */
internal fun interface ScheduledTask {
    /** Stops the task from running if it has not started yet. */
    fun cancel()
}

/** The production transport: OkHttp WebSockets with a client-side ping. */
internal class OkHttpControlTransport(
    pingIntervalSeconds: Long = CONTROL_PING_INTERVAL_SECONDS,
) : ControlTransport {
    private val client = OkHttpClient.Builder()
        .pingInterval(pingIntervalSeconds, TimeUnit.SECONDS)
        .build()

    override fun open(url: String, events: ControlSocketEvents): ControlSocket {
        val request = Request.Builder().url(url).build()
        val webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                events.onOpen(OkHttpControlSocket(webSocket))
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                events.onMessage(OkHttpControlSocket(webSocket), text)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                events.onClosing(OkHttpControlSocket(webSocket), code, reason)
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                events.onFailure(OkHttpControlSocket(webSocket), t)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                events.onClosed(OkHttpControlSocket(webSocket), code, reason)
            }
        })
        return OkHttpControlSocket(webSocket)
    }

    override fun shutdown() {
        client.dispatcher.executorService.shutdown()
    }
}

private class OkHttpControlSocket(private val webSocket: WebSocket) : ControlSocket {
    override fun send(text: String): Boolean = webSocket.send(text)

    override fun close(code: Int, reason: String?): Boolean = webSocket.close(code, reason)

    override fun cancel() = webSocket.cancel()
}

/** The production scheduler: one background thread, so the channel's sends keep their order. */
internal class ExecutorControlScheduler : ControlScheduler {
    private val executor = Executors.newSingleThreadScheduledExecutor()

    override fun execute(task: () -> Unit) {
        try {
            executor.execute(Runnable { task() })
        } catch (_: RejectedExecutionException) {
            // Shut down: the Activity is gone and nothing is listening any more.
        }
    }

    override fun schedule(delayMs: Long, task: () -> Unit): ScheduledTask? = try {
        val future = executor.schedule(Runnable { task() }, delayMs, TimeUnit.MILLISECONDS)
        ScheduledTask { future.cancel(false) }
    } catch (_: RejectedExecutionException) {
        null
    }

    override fun shutdown() {
        executor.shutdownNow()
    }
}
