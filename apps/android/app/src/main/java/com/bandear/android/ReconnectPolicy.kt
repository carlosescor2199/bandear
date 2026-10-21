package com.bandear.android

import java.util.concurrent.atomic.AtomicLong

/**
 * How long the control channel waits before each attempt to reconnect a lost connection.
 *
 * The first retries come quickly, so a short Wi-Fi blip or a server restart costs the musician
 * well under a few seconds, then the wait grows and settles at [MAX_DELAY_MS]: a server that stays
 * away is retried every five seconds for as long as the session runs, which bounds how long the
 * phone takes to notice the network is back without hammering it meanwhile. A connection that
 * proves itself, by receiving its first message from the server, calls [reset], so the next loss
 * starts from the short end of the schedule again; a server that accepts and drops at once does not.
 *
 * Not thread-safe on its own: the channel only touches it while holding its connection lock.
 */
internal class ReconnectPolicy {
    private var attempt = 0

    /** The wait before the next attempt; each call moves one step along the schedule. */
    fun nextDelayMs(): Long {
        val delay = delayForAttempt(attempt)
        // The counter stops once the schedule is exhausted, so it can never overflow.
        if (attempt < SCHEDULE_MS.size) attempt++
        return delay
    }

    /** Starts the schedule again, after a connection heard from the server. */
    fun reset() {
        attempt = 0
    }

    companion object {
        /** The waits of the first attempts, in order. Every attempt after them waits the cap. */
        private val SCHEDULE_MS = longArrayOf(500L, 1_000L, 2_000L, 4_000L)

        /** The longest wait between two attempts. */
        const val MAX_DELAY_MS = 5_000L

        /** The wait before attempt [attempt], counted from 0 for the first retry after a loss. */
        fun delayForAttempt(attempt: Int): Long =
            SCHEDULE_MS.getOrElse(attempt.coerceAtLeast(0)) { MAX_DELAY_MS }

        /**
         * Whether a connection that failed or closed should be retried: only while the session is
         * still running and nobody stopped it. Stop, the receiver ending on its own and the
         * Activity going away all stop the session, and nothing reconnects after them.
         */
        fun shouldReconnect(sessionRunning: Boolean, stopped: Boolean): Boolean =
            sessionRunning && !stopped
    }
}

/**
 * Tells the current control connection apart from older ones, so a late callback from a socket that
 * was replaced, lost or stopped can never touch the connection that is live now.
 *
 * Every connection attempt takes a fresh generation from [begin], and its socket's callbacks carry
 * that generation. A callback acts only while [isCurrent] holds for it. The end of a connection is
 * claimed with [end], which succeeds exactly once, so a failure and a close racing on two threads
 * schedule one reconnect, not two. [invalidate] makes every existing generation stale at once, for
 * a stop. Values set by [end] and [invalidate] are never handed out by [begin], so no socket can
 * become current again.
 */
internal class ConnectionGenerations {
    private val current = AtomicLong(0L)

    /** Starts a new connection attempt and returns its generation; every earlier one becomes stale. */
    fun begin(): Long = current.incrementAndGet()

    /** True while [generation] belongs to the live connection attempt. */
    fun isCurrent(generation: Long): Boolean = current.get() == generation

    /**
     * Ends [generation] if it is still the current one. True only for the first caller; false for a
     * stale generation or one that already ended, which leaves the current connection untouched.
     */
    fun end(generation: Long): Boolean = current.compareAndSet(generation, generation + 1)

    /** Makes every generation handed out so far stale. */
    fun invalidate() {
        current.incrementAndGet()
    }
}
