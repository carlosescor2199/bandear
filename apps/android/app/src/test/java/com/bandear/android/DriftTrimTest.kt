package com.bandear.android

import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The clock-drift compensation: the policy that decides how many frames to remove from the next
 * packet ([DriftTrimmer]), the splice that removes them without a click ([PcmSplice]) and the queue
 * the receive loop keeps before the track ([PlayoutQueue]).
 */
class DriftTrimTest {
    private val rate = 48_000
    private val packet = 240
    private val target = 480L

    /**
     * Plays [seconds] of 240-frame packets through a trimmer. The surplus is the frames queued above
     * the target before each packet: it starts at [startSurplus], [grow] adds to it per packet (a
     * faster desk clock), [burst] adds a transient one, and every trim takes from it, as the real
     * queue would. Answers the frames trimmed per packet.
     */
    private fun simulate(
        seconds: Int,
        startSurplus: Double = 0.0,
        grow: Double = 0.0,
        burst: (Int) -> Double = { 0.0 },
        observe: (Double) -> Unit = {},
    ): List<Int> {
        val trimmer = DriftTrimmer(rate)
        var surplus = startSurplus
        val trims = ArrayList<Int>()
        val packets = seconds * rate / packet
        for (index in 0 until packets) {
            surplus += grow
            val seen = surplus + burst(index)
            val trimmed = trimmer.onPacket(packet, target + seen.toLong(), target)
            surplus -= trimmed
            trims += trimmed
            observe(surplus)
        }
        return trims
    }

    @Test
    fun aQueueAtTheTargetIsNeverTrimmed() {
        assertEquals(0, simulate(60).sum())
        // A track that is a packet short of its target (the paced steady state) is not trimmed either.
        assertEquals(0, simulate(60, startSurplus = -packet.toDouble()).sum())
    }

    @Test
    fun aSingleBurstNeverTriggersATrim() {
        // 20 packets (100 ms) of surplus once, three seconds in: the window's minimum stays at zero.
        val trims = simulate(30, burst = { index -> if (index in 600 until 620) 4_800.0 else 0.0 })
        assertEquals(0, trims.sum())
    }

    @Test
    fun aSmallSurplusBelowHalfAPacketIsLeftAlone() {
        assertEquals(0, simulate(60, startSurplus = 100.0).sum())
    }

    @Test
    fun aPersistentSurplusIsTrimmedOnlyAfterItHasPersisted() {
        val trims = simulate(30, startSurplus = 1_200.0)
        val perSecond = rate / packet
        assertEquals("nothing is removed in the first two seconds", 0, trims.take(2 * perSecond).sum())
        assertTrue("it is removed afterwards", trims.sum() > 0)
    }

    @Test
    fun trimmingIsSmallPerSpliceAndPerSecond() {
        val trims = simulate(30, startSurplus = 5_000.0)
        assertTrue("a splice is a few frames", trims.all { it <= 8 })
        val perSecond = rate / packet
        for (start in 0..trims.size - perSecond step perSecond / 4) {
            val removed = trims.subList(start, start + perSecond).sum()
            // Well under 0.1% of the 48 000 frames played in the second (at most 48), with one splice of slack.
            assertTrue("removed $removed frames in one second", removed <= 32)
        }
    }

    @Test
    fun trimmingStopsNearTheTargetAndNeverGoesBelowIt() {
        var lowest = Double.MAX_VALUE
        var last = 0.0
        simulate(150, startSurplus = 1_200.0) {
            lowest = minOf(lowest, it)
            last = it
        }
        assertTrue("went below the target: $lowest", lowest >= 0.0)
        assertTrue("stopped at $last", last <= packet / 2.0)
    }

    @Test
    fun itBeatsTheDriftOfAFasterClock() {
        // 200 ppm: the desk delivers 9.6 frames a second more than the phone plays.
        val growPerPacket = 200e-6 * packet
        var highest = 0.0
        var lowest = Double.MAX_VALUE
        var index = 0
        simulate(300, grow = growPerPacket) {
            if (index++ > 20 * rate / packet) {
                highest = maxOf(highest, it)
                lowest = minOf(lowest, it)
            }
        }
        assertTrue("latency kept growing: $highest frames over the target", highest <= 2.0 * packet)
        assertTrue("went below the target: $lowest", lowest >= 0.0)
    }

    @Test
    fun aSpliceIsAtMostAQuarterOfAPacket() {
        val trimmer = DriftTrimmer(rate)
        var biggest = 0
        repeat(rate * 20 / 16) {
            biggest = maxOf(biggest, trimmer.onPacket(16, target + 10_000L, target))
        }
        assertTrue("trimmed $biggest frames from a 16-frame packet", biggest in 1..4)
    }

    // The splice.

    private fun stereo(frames: Int, left: (Int) -> Int, right: (Int) -> Int) = ShortArray(frames * 2) { i ->
        (if (i % 2 == 0) left(i / 2) else right(i / 2)).toShort()
    }

    @Test
    fun removingZeroFramesChangesNothing() {
        val samples = stereo(240, { it }, { -it })
        assertArrayEquals(samples, PcmSplice.removeFrames(samples, 2, 0))
    }

    @Test
    fun removingFramesShortensThePacketByExactlyThatMany() {
        val samples = stereo(240, { it }, { -it })
        assertEquals((240 - 8) * 2, PcmSplice.removeFrames(samples, 2, 8).size)
    }

    @Test
    fun aSteadySignalStaysSteadyAcrossTheSplice() {
        val samples = stereo(240, { 1_000 }, { -500 })
        val out = PcmSplice.removeFrames(samples, 2, 8)
        assertEquals((240 - 8) * 2, out.size)
        for (frame in 0 until out.size / 2) {
            assertEquals(1_000, out[frame * 2].toInt())
            assertEquals(-500, out[frame * 2 + 1].toInt())
        }
    }

    @Test
    fun theSpliceMakesNoClickOnATone() {
        val tone = stereo(
            240,
            { (10_000 * sin(2 * PI * 1_000 * it / 48_000)).toInt() },
            { (10_000 * sin(2 * PI * 1_000 * it / 48_000)).toInt() },
        )
        fun biggestStep(samples: ShortArray): Int =
            (1 until samples.size / 2).maxOf { abs(samples[it * 2] - samples[(it - 1) * 2]) }
        val out = PcmSplice.removeFrames(tone, 2, 8)
        assertEquals((240 - 8) * 2, out.size)
        assertTrue(
            "step ${biggestStep(out)} against ${biggestStep(tone)} in the input",
            biggestStep(out) <= biggestStep(tone) * 3 / 2,
        )
    }

    @Test
    fun theEdgesOfThePacketAreKept() {
        val samples = stereo(240, { it + 1 }, { -(it + 1) })
        val out = PcmSplice.removeFrames(samples, 2, 8)
        assertEquals((240 - 8) * 2, out.size)
        assertEquals(samples[0], out[0])
        assertEquals(samples[1], out[1])
        assertEquals(samples[samples.size - 2], out[out.size - 2])
        assertEquals(samples[samples.size - 1], out[out.size - 1])
    }

    @Test
    fun aTinyPacketIsSplicedSafely() {
        val three = stereo(3, { it + 1 }, { it + 1 })
        // At most half the frames can go.
        assertEquals(2 * 2, PcmSplice.removeFrames(three, 2, 8).size)
        val one = stereo(1, { 7 }, { 7 })
        assertArrayEquals(one, PcmSplice.removeFrames(one, 2, 8))
        assertEquals(0, PcmSplice.removeFrames(ShortArray(0), 2, 8).size)
    }

    @Test
    fun aMonoPacketAndAnOddSampleCountAreHandled() {
        val mono = ShortArray(100) { 300 }
        assertEquals(92, PcmSplice.removeFrames(mono, 1, 8).size)
        val odd = ShortArray(7) { 1 }
        assertArrayEquals(odd, PcmSplice.removeFrames(odd, 2, 2))
    }

    // The queue before the track.

    @Test
    fun theQueueKeepsOrderAndCountsItsFrames() {
        val queue = PlayoutQueue<String>(1_000)
        assertEquals(0L, queue.add("a", 240))
        assertEquals(0L, queue.add("b", 240))
        assertEquals(480L, queue.queuedFrames)
        assertEquals("a", queue.poll())
        assertEquals(240L, queue.queuedFrames)
        assertEquals("b", queue.poll())
        assertNull(queue.poll())
        assertEquals(0L, queue.queuedFrames)
    }

    @Test
    fun aQueueOverItsLimitDropsTheOldestAndKeepsTheNewest() {
        val queue = PlayoutQueue<String>(500)
        queue.add("a", 240)
        queue.add("b", 240)
        assertEquals(240L, queue.add("c", 240))
        assertEquals(480L, queue.queuedFrames)
        assertEquals("b", queue.poll())
        assertEquals("c", queue.poll())
        // A single packet longer than the limit is still kept.
        val tiny = PlayoutQueue<String>(100)
        assertEquals(0L, tiny.add("big", 240))
        assertEquals("big", tiny.poll())
    }
}
