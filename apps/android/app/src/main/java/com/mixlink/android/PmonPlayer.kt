package com.mixlink.android

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTimestamp
import android.media.AudioTrack
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketException
import java.util.concurrent.atomic.AtomicBoolean

/** Packets between adaptive playback-buffer adjustments: about one second at 100 packets/s. */
private const val ADJUST_INTERVAL_PACKETS = 100

private const val NANOS_PER_MILLISECOND = 1_000_000L

/**
 * The UDP receive loop, AudioTrack playback and the receiver metrics.
 *
 * The player owns the socket, the receiver thread and the audio track, so it needs no `Context`.
 * It reports metrics, errors, completion and the `server owns the mix` decision through [Listener];
 * it never touches the UI itself. [running] is the shared session flag owned by the caller, so the
 * start/stop gating is identical to before the split.
 */
internal class PmonPlayer(
    private val listener: Listener,
    private val running: AtomicBoolean,
    private val mixState: ClientMixState,
) {
    interface Listener {
        /** True once the server has acknowledged a mix, so local processing must not run. */
        fun serverOwnsMix(): Boolean

        fun onStats(
            packets: Long,
            lost: Long,
            sampleRate: Int?,
            packetsPerSecond: Double,
            interArrivalMs: Double,
            jitterMs: Double,
            discardedSamples: Long,
            packetsWithDiscardedSamples: Long,
            playbackBacklogMs: Double,
        )

        /**
         * One link report for the server, about once a second once the audio starts or a startup
         * grace passes without it, packets or not (see [LinkStatsAccumulator]). Called on the
         * receive thread, which also feeds the audio track, so it must only hand the report over
         * and never block.
         */
        fun onLinkStats(report: LinkStatsReport)

        fun onError(message: String)

        fun onStopped()
    }

    @Volatile private var socket: DatagramSocket? = null
    @Volatile private var receiverThread: Thread? = null

    fun isReceiverAlive(): Boolean = receiverThread?.isAlive == true

    fun launch(host: String, port: Int) {
        val thread = Thread({ receiveLoop(host, port) }, "pmon-udp-receiver")
        receiverThread = thread
        thread.start()
    }

    fun stopReceiving() {
        socket?.close()
        receiverThread?.interrupt()
    }

    private fun receiveLoop(host: String, port: Int) {
        var packetsReceived = 0L
        val sequenceTracker = SequenceTracker()
        var lastSampleRate: Int? = null
        var sourceSampleRate: Int? = null
        var audioTrack: AudioTrack? = null
        var playbackRate: Int? = null
        val metricsStartedAtNanos = System.nanoTime()
        val linkStats = LinkStatsAccumulator(metricsStartedAtNanos / NANOS_PER_MILLISECOND)
        var previousPacketAtNanos: Long? = null
        var interArrivalMs = 0.0
        var jitterMs = 0.0
        var packetsPerSecond = 0.0
        var discardedSamples = 0L
        var packetsWithDiscardedSamples = 0L
        var playbackBacklogMs = 0.0
        var framesWritten = 0L
        var packetFrames = 0
        var cushionPackets = PlaybackLatency.START_CUSHION_PACKETS
        var cleanIntervals = 0
        var lastUnderruns = 0
        var packetsSinceAdjust = 0
        val audioTimestamp = AudioTimestamp()
        val buffer = ByteArray(65_507)

        try {
            try {
                InetAddress.getByName(host)
            } catch (error: Exception) {
                listener.onError("Could not resolve server host: ${error.message ?: "unknown error"}")
                return
            }
            DatagramSocket(port).also { openedSocket ->
                socket = openedSocket
                openedSocket.soTimeout = 250
            }.use { openedSocket ->
                while (running.get()) {
                    // The link report rides on the loop's own wake-ups: every packet and every
                    // receive timeout passes here, so a report is at most one timeout late, and the
                    // track's underrun count is read only when one is due.
                    val nowMs = System.nanoTime() / NANOS_PER_MILLISECOND
                    if (linkStats.isDue(nowMs)) {
                        linkStats.poll(nowMs, audioTrack?.underrunCount ?: 0)?.let { report ->
                            listener.onLinkStats(report)
                        }
                    }

                    val datagram = DatagramPacket(buffer, buffer.size)
                    datagram.setLength(buffer.size)
                    try {
                        openedSocket.receive(datagram)
                    } catch (_: java.net.SocketTimeoutException) {
                        linkStats.onReceiveTimeout()
                        continue
                    }

                    try {
                        val packet = PmonPacketParser.parse(datagram.data, datagram.length)
                        if (packet.channels > MAX_CHANNELS) {
                            throw PmonPacketParseException(
                                "PMON packet announces ${packet.channels} channels, the limit is $MAX_CHANNELS",
                            )
                        }
                        val lostBefore = sequenceTracker.accept(packet.sequence)
                        if (lostBefore == SequenceTracker.DROPPED) {
                            // A duplicate, or a packet that arrived after a newer one was played:
                            // playing it now would be out of order, and its gap is already counted.
                            continue
                        }
                        packetsReceived++
                        lastSampleRate = packet.sampleRate
                        val packetReceivedAtNanos = System.nanoTime()
                        linkStats.onPacket(lostBefore, packetReceivedAtNanos / NANOS_PER_MILLISECOND)
                        previousPacketAtNanos?.let { previousAtNanos ->
                            val intervalMs = (packetReceivedAtNanos - previousAtNanos) / 1_000_000.0
                            if (interArrivalMs == 0.0) {
                                interArrivalMs = intervalMs
                            } else {
                                interArrivalMs += 0.1 * (intervalMs - interArrivalMs)
                            }
                            jitterMs += 0.1 * (kotlin.math.abs(intervalMs - interArrivalMs) - jitterMs)
                        }
                        previousPacketAtNanos = packetReceivedAtNanos
                        val elapsedSeconds = (packetReceivedAtNanos - metricsStartedAtNanos) / 1_000_000_000.0
                        packetsPerSecond = if (elapsedSeconds > 0.0) packetsReceived / elapsedSeconds else 0.0

                        if (audioTrack == null) {
                            sourceSampleRate = packet.sampleRate
                            playbackRate = selectPlaybackRate(packet.sampleRate)
                            packetFrames = packet.samples.size / PlaybackLatency.CHANNELS
                            val track = createAudioTrack(playbackRate)
                            track.play()
                            applyPlaybackBufferTarget(track, packetFrames, cushionPackets)
                            audioTrack = track
                        } else if (packet.sampleRate != sourceSampleRate) {
                            throw PmonPacketParseException(
                                "sample rate changed from $sourceSampleRate to ${packet.sampleRate}",
                            )
                        }

                        val processedSamples = Pcm16Processor.applyLocalProtection(
                            samples = packet.samples,
                            volumePercent = mixState.volumePercent,
                            maxLevelPercent = mixState.maxLevelPercent,
                            muted = mixState.muted,
                            serverOwnsMix = listener.serverOwnsMix(),
                        )
                        val writtenSamples = audioTrack.write(
                            processedSamples,
                            0,
                            processedSamples.size,
                            AudioTrack.WRITE_BLOCKING,
                        )
                        // A blocking write returns only once every sample is queued, so a short count
                        // now means a write error. The guard stays as the honest signal that audio
                        // was lost instead of being played.
                        if (writtenSamples < processedSamples.size) {
                            discardedSamples += if (writtenSamples >= 0) {
                                (processedSamples.size - writtenSamples).toLong()
                            } else {
                                processedSamples.size.toLong()
                            }
                            packetsWithDiscardedSamples++
                        }
                        // Only samples that actually entered the track count as written. A partial
                        // write leaves the rest dropped, and counting it anyway would understate how
                        // far the writer has run ahead of the render position.
                        if (writtenSamples > 0) {
                            framesWritten += writtenSamples / PlaybackLatency.CHANNELS
                        }
                        if (audioTrack.getTimestamp(audioTimestamp)) {
                            val instantBacklog = PlaybackLatency.backlogMs(
                                framesWritten,
                                audioTimestamp.framePosition,
                                playbackRate ?: packet.sampleRate,
                            )
                            playbackBacklogMs = if (playbackBacklogMs == 0.0) {
                                instantBacklog
                            } else {
                                playbackBacklogMs + 0.1 * (instantBacklog - playbackBacklogMs)
                            }
                        }
                        listener.onStats(
                            packetsReceived,
                            sequenceTracker.lost,
                            lastSampleRate,
                            packetsPerSecond,
                            interArrivalMs,
                            jitterMs,
                            discardedSamples,
                            packetsWithDiscardedSamples,
                            playbackBacklogMs,
                        )
                        packetsSinceAdjust++
                        if (packetsSinceAdjust >= ADJUST_INTERVAL_PACKETS) {
                            packetsSinceAdjust = 0
                            val underruns = audioTrack.underrunCount
                            val decision = PlaybackLatency.nextCushion(
                                cushionPackets,
                                cleanIntervals,
                                underruns - lastUnderruns,
                            )
                            lastUnderruns = underruns
                            cleanIntervals = decision.cleanIntervals
                            if (decision.cushion != cushionPackets) {
                                cushionPackets = decision.cushion
                                applyPlaybackBufferTarget(audioTrack, packetFrames, cushionPackets)
                            }
                        }
                    } catch (error: Exception) {
                        listener.onError(error.message ?: "invalid PMON packet")
                    }
                }
            }
        } catch (error: SocketException) {
            if (running.get()) listener.onError("UDP socket error: ${error.message ?: "unknown error"}")
        } catch (error: Exception) {
            if (running.get()) listener.onError("Receiver error: ${error.message ?: "unknown error"}")
        } finally {
            audioTrack?.run {
                pause()
                flush()
                release()
            }
            socket = null
            receiverThread = null
            running.set(false)
            listener.onStopped()
        }
    }

    private fun selectPlaybackRate(announcedRate: Int): Int {
        val minimum = AudioTrack.getMinBufferSize(
            announcedRate,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        return if (minimum > 0) announcedRate else 48_000
    }

    private fun createAudioTrack(sampleRate: Int): AudioTrack {
        val minimum = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        if (minimum <= 0) throw IllegalStateException("sample rate $sampleRate is not supported")
        return AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                    .build(),
            )
            .setBufferSizeInBytes(minimum)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()
            .also { track ->
                if (track.state != AudioTrack.STATE_INITIALIZED) {
                    track.release()
                    throw IllegalStateException("could not initialize AudioTrack")
                }
            }
    }

    /**
     * Asks the track for a playback buffer of [cushionPackets] packet intervals.
     *
     * The track's buffer occupancy is the playback latency, so a buffer that is merely the device's
     * default can sit tens of milliseconds behind the render position and pay that on every sample.
     * The platform clamps the request up to its own hardware minimum, so a target below the floor
     * degrades to the floor rather than failing; a refused call leaves the buffer as it was.
     */
    private fun applyPlaybackBufferTarget(track: AudioTrack, packetFrames: Int, cushionPackets: Int) {
        try {
            track.setBufferSizeInFrames(
                PlaybackLatency.targetBufferFrames(packetFrames, cushionPackets),
            )
        } catch (_: IllegalStateException) {
            // Keep the buffer the track was created with.
        }
    }
}
