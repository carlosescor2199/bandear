import BandEarProtocol
import Foundation

/// A monotonic clock, read in nanoseconds: Android's `System.nanoTime()`.
///
/// The receive loop times arrivals in nanoseconds (``ArrivalTiming``) and everything else in
/// milliseconds of the same reading (``nowMilliseconds``), as Android divides `nanoTime` by a
/// million. Injected, so the loop's timing is tested without waiting.
public protocol MonotonicClock {
    var nowNanoseconds: UInt64 { get }
}

extension MonotonicClock {
    /// ``nowNanoseconds`` in whole milliseconds.
    public var nowMilliseconds: Int64 { Int64(nowNanoseconds / 1_000_000) }
}

/// The system's monotonic clock, which does not move while the device sleeps, as Android's
/// `nanoTime` does not.
public struct SystemMonotonicClock: MonotonicClock, Sendable {
    public init() {}

    public var nowNanoseconds: UInt64 { DispatchTime.now().uptimeNanoseconds }
}

/// The receiver's own metrics after each packet played: Android's nine `onStats` values
/// (`PmonPlayer.kt:77-87`), all since the loop started.
///
/// A value not measured yet is `nil`, where Android shows 0: the spacing and jitter until a second
/// packet arrived (``ArrivalTiming``), the backlog until the output said what it presented. The
/// sample rate is never missing, since only a played packet reports.
public struct ReceiverStats: Equatable, Sendable {
    /// Packets played.
    public let packets: Int64
    /// Packets the stream skipped, each counted once (``HoldPlayout/lost``).
    public let lost: Int64
    /// The rate the last packet played announced.
    public let sampleRate: Int
    /// Packets played per second since the loop started.
    public let packetsPerSecond: Double
    /// The smoothed spacing between arrivals (``ArrivalTiming/interArrivalMilliseconds``).
    public let interArrivalMilliseconds: Double?
    /// The smoothed jitter around that spacing (``ArrivalTiming/jitterMilliseconds``).
    public let jitterMilliseconds: Double?
    /// Samples the output did not take, so never played.
    public let discardedSamples: Int64
    /// Packets that lost any samples that way.
    public let packetsWithDiscardedSamples: Int64
    /// The audio queued ahead of the listener, smoothed by a tenth per packet.
    public let playbackBacklogMilliseconds: Double?
    /// Whether the output was reaching the listener as this packet was written
    /// (``PlaybackSink/isPlaying``). Not one of Android's values. While it is `false` the packets
    /// still count as played, but nobody hears them.
    public let outputPlaying: Bool

    public init(
        packets: Int64, lost: Int64, sampleRate: Int, packetsPerSecond: Double,
        interArrivalMilliseconds: Double?, jitterMilliseconds: Double?, discardedSamples: Int64,
        packetsWithDiscardedSamples: Int64, playbackBacklogMilliseconds: Double?,
        outputPlaying: Bool
    ) {
        self.packets = packets
        self.lost = lost
        self.sampleRate = sampleRate
        self.packetsPerSecond = packetsPerSecond
        self.interArrivalMilliseconds = interArrivalMilliseconds
        self.jitterMilliseconds = jitterMilliseconds
        self.discardedSamples = discardedSamples
        self.packetsWithDiscardedSamples = packetsWithDiscardedSamples
        self.playbackBacklogMilliseconds = playbackBacklogMilliseconds
        self.outputPlaying = outputPlaying
    }
}

/// What the receiver tells the rest of the app: Android's `PmonPlayer.Listener`
/// (`PmonPlayer.kt:73-106`). Every call comes on the receive thread, which also feeds the output,
/// so an implementation only hands the value over and never blocks.
///
/// Android's `serverOwnsMix()` is not here: it is one of the ``PlayerSettings``.
public protocol ReceiverListener: AnyObject {
    /// The receiver's metrics, after every packet played.
    func onStats(_ stats: ReceiverStats)

    /// One link report for the desk, about once a second once the audio starts or a startup grace
    /// passes without it, packets or not (``LinkStatsAccumulator``).
    func onLinkStats(_ report: LinkStatsReport)

    /// A packet that could not be played, an output that could not open, or the socket failing.
    /// Android passes the message; here the error itself comes.
    func onError(_ error: any Error)

    /// One launch's loop ended, on request or on its own: once per launch, after the session flag
    /// went down if this was the latest launch. The receiver counts as alive until this returns
    /// (``AudioReceiver/isAlive``).
    func onStopped()
}

/// Why the loop refused a packet that decoded.
public enum ReceiveError: Error, Equatable, Sendable {
    /// The packet announced more channels than the desk mixes (``ReceiveLoop/maximumChannels``).
    case tooManyChannels(Int)
    /// The packet's rate is not the one the output opened at.
    case sampleRateChanged(from: Int, to: Int)
}

/// The receive loop: one wake-up per ``step()``, from the socket to the output.
///
/// A port of the Android client's `PmonPlayer.receiveLoop` (`PmonPlayer.kt:124-357`), written
/// against the ``DatagramSource`` and the ``PlaybackSink``, so it runs here with a fake socket, a
/// fake output and a clock the caller sets. It reuses the module's ports for every rule:
/// ``AudioDatagram`` reads the packet, ``HoldPlayout`` orders and reports it, ``ArrivalTiming``
/// times it, ``Pcm16Processor`` protects it, ``CushionController`` sizes the cushion, and
/// ``PlaybackLatency`` measures it.
///
/// Each wake-up, in order:
/// 1. A link report, when one is due: its extras from the cushion the output applied and the
///    frame it presented, read only then.
/// 2. One receive, which answers a datagram or times out. When the session flag went down while
///    it waited, the wake-up ends here: the datagram that woke it for the stop
///    (``DatagramSource/wake()``) is never read, so it is never an error. Otherwise a timeout
///    reaches the playout.
/// 3. The redundancy hold, read from the controls, then the packet: decoded, refused above
///    ``maximumChannels``, skipped when the playout refuses it (a duplicate, or too late), else
///    timed as it arrived.
/// 4. Every packet now due, in sequence order: the first opens the output at its rate and size
///    and asks it for the cushion; each is protected while the desk does not own the mix, written,
///    measured and reported, and moves the cushion.
///
/// As on Android, a packet that cannot be played is reported (``ReceiverListener/onError(_:)``)
/// and ends only its own wake-up: a refused packet leaves what is due for the next wake-up, a few
/// milliseconds later. Only the socket failing ends the loop.
///
/// As on Android, the output opens once, at the first packet's rate. A packet at any other rate is
/// an error, every one of them, and the output is never reopened at the new rate
/// (`PmonPlayer.kt:257-260`): a desk does not change its rate mid-session.
///
/// Single-threaded, on the receive thread. One receive buffer and one sample buffer are kept for
/// the loop's life; the decoded packet is the one allocation per datagram.
public final class ReceiveLoop {
    /// The most channels a packet may announce: Android's `MAX_CHANNELS` (`ClientMixState.kt:143`),
    /// the source channels a mix can hold.
    public static let maximumChannels = 32
    /// The largest UDP payload over IPv4, the size of Android's receive buffer.
    public static let maximumDatagramBytes = 65_507
    /// The output plays stereo: Android counts a frame as two samples whatever the packet says.
    static let outputChannels = 2
    /// The weight each new backlog reading gets in the smoothed one.
    static let backlogSmoothing = 0.1

    private let source: any DatagramSource
    private let sinkFactory: any PlaybackSinkFactory
    private let controls: PlayerControls
    private let listener: any ReceiverListener
    private let clock: any MonotonicClock

    private let playout: HoldPlayout<AudioDatagram>
    private let cushion = CushionController()
    private var arrival = ArrivalTiming()
    private var receiveBuffer = [UInt8](repeating: 0, count: ReceiveLoop.maximumDatagramBytes)
    private var samples = [Int16](
        repeating: 0,
        count: (ReceiveLoop.maximumDatagramBytes - AudioDatagramHeader.byteCount) / 2)

    private let startedAtNanoseconds: UInt64
    private var sink: (any PlaybackSink)?
    private var sourceSampleRate: Int?
    private var playbackRate: Int?
    private var packetFrames = 0
    private var capacityFrames = 0
    /// The target the output applied, as it answered (0 when it could not say).
    private var appliedTargetFrames = 0
    private var framesWritten: Int64 = 0

    private var packetsPlayed: Int64 = 0
    private var lastSampleRate = 0
    private var packetsPerSecond = 0.0
    private var discardedSamples: Int64 = 0
    private var packetsWithDiscardedSamples: Int64 = 0
    private var backlogMilliseconds: Double?

    /// A loop that reads `source` and plays through an output `sinkFactory` opens at the first
    /// packet, by the `controls`, telling `listener`, timed by `clock` from now.
    public init(
        source: any DatagramSource, sinkFactory: any PlaybackSinkFactory,
        controls: PlayerControls, listener: any ReceiverListener, clock: any MonotonicClock
    ) {
        self.source = source
        self.sinkFactory = sinkFactory
        self.controls = controls
        self.listener = listener
        self.clock = clock
        startedAtNanoseconds = clock.nowNanoseconds
        playout = HoldPlayout(
            hold: ReorderHold(),
            linkStats: LinkStatsAccumulator(
                startedAtMilliseconds: Self.milliseconds(startedAtNanoseconds)))
    }

    /// Steps while `running` is up, then closes the output. The socket failing ends it too, and is
    /// reported while the session still runs; a socket closed by a stop is not an error.
    public func run(while running: RunningFlag) {
        defer { close() }
        do {
            while running.value { try step(while: running) }
        } catch {
            if running.value { listener.onError(error) }
        }
    }

    /// One wake-up. Throws only when the source does: the loop is over.
    ///
    /// With `running`, a wake-up whose receive answers after the flag went down ends there,
    /// whatever it received; an empty datagram from anyone else while the session runs is a packet
    /// like any other, and an error, as on Android.
    public func step(while running: RunningFlag? = nil) throws {
        reportIfDue()
        let byteCount = try receiveBuffer.withUnsafeMutableBytes { try source.receive(into: $0) }
        // Stopped while waiting: what woke the receive is not audio to play.
        if let running, !running.value { return }
        if byteCount == nil {
            // Nothing came for a while: what is held is not worth waiting for any longer.
            playout.onTimeout()
        }
        // Everything this wake-up plays by, read together once.
        let settings = controls.snapshot
        do {
            try offerAndPlay(byteCount, settings: settings)
        } catch {
            listener.onError(error)
        }
    }

    /// Stops the output and lets it go, once; nothing is written after this.
    public func close() {
        sink?.close()
        sink = nil
    }

    /// The link report rides on the loop's wake-ups, so it is at most one receive timeout late,
    /// and the output is asked for its underruns and presented frame only when one is due.
    private func reportIfDue() {
        let now = clock.nowMilliseconds
        guard playout.linkStats.isDue(nowMilliseconds: now) else { return }
        let extras = PlaybackLatency.reportExtras(
            packetFrames: packetFrames, cushionPackets: cushion.cushionPackets,
            playbackRate: playbackRate, framesWritten: framesWritten,
            presentedFrame: sink?.presentedFrame, appliedBufferFrames: appliedTargetFrames,
            holdPackets: playout.holdPackets)
        if let report = playout.poll(
            nowMilliseconds: now, cumulativeUnderruns: sink?.underrunCount ?? 0,
            cushionMilliseconds: extras.cushionMilliseconds,
            backlogMilliseconds: extras.backlogMilliseconds)
        {
            listener.onLinkStats(report)
        }
    }

    private func offerAndPlay(_ byteCount: Int?, settings: PlayerSettings) throws {
        // The desk's hold, read once per wake-up, so a change applies from the next packet.
        playout.wake(holdPackets: settings.redundancyHoldPackets)
        if let byteCount {
            let length = min(max(byteCount, 0), receiveBuffer.count)
            let packet = try AudioDatagram(decoding: receiveBuffer[..<length])
            let channels = Int(packet.header.channels)
            guard channels <= Self.maximumChannels else {
                throw ReceiveError.tooManyChannels(channels)
            }
            // A duplicate, or a packet that arrived after a newer one was played or given up on:
            // playing it now would be out of order, and its gap is already counted. As Android's
            // `continue` does, this ends the wake-up: what is due plays on the next one.
            guard playout.offer(sequence: packet.header.sequence, packet: packet) else { return }
            // The arrival's spacing, as it arrived, whatever the hold does with it.
            if let deviation = arrival.onArrival(nowNanoseconds: clock.nowNanoseconds) {
                playout.onArrivalDeviation(milliseconds: deviation)
            }
        }
        // Every packet now due, in sequence order: at once with no hold, else once the hold has
        // passed or a late copy filled its gap.
        while true {
            let playedAt = clock.nowNanoseconds
            guard let packet = playout.nextToPlay(nowMilliseconds: Self.milliseconds(playedAt))
            else { break }
            try play(packet, playedAtNanoseconds: playedAt, settings: settings)
        }
    }

    private func play(
        _ packet: AudioDatagram, playedAtNanoseconds: UInt64, settings: PlayerSettings
    ) throws {
        packetsPlayed += 1
        let rate = Int(packet.header.sampleRate)
        lastSampleRate = rate
        let elapsedSeconds =
            Double(Int64(bitPattern: playedAtNanoseconds &- startedAtNanoseconds)) / 1_000_000_000
        packetsPerSecond = elapsedSeconds > 0 ? Double(packetsPlayed) / elapsedSeconds : 0

        let count = packet.samples.count
        let frames = count / Self.outputChannels
        let sink = try openedSink(rate: rate, frames: frames, settings: settings)

        let takenFrames = samples.withUnsafeMutableBufferPointer { buffer in
            let region = UnsafeMutableBufferPointer(rebasing: buffer[..<count])
            _ = region.update(fromContentsOf: packet.samples)
            Pcm16Processor.applyLocalProtection(
                region, volumePercent: settings.volumePercent,
                maxLevelPercent: settings.maxLevelPercent, muted: settings.muted,
                serverOwnsMix: settings.serverOwnsMix)
            return min(max(sink.write(UnsafeBufferPointer(region)), 0), frames)
        }
        // A short write is audio lost instead of played. Only what the output took counts as
        // written: counting the rest would understate how far the writer ran ahead of the listener.
        let takenSamples = takenFrames * Self.outputChannels
        if takenSamples < count {
            discardedSamples += Int64(count - takenSamples)
            packetsWithDiscardedSamples += 1
        }
        framesWritten += Int64(takenFrames)

        if let presented = sink.presentedFrame {
            let instant = PlaybackLatency.backlogMilliseconds(
                framesWritten: framesWritten, framePosition: presented,
                sampleRate: playbackRate ?? rate)
            backlogMilliseconds =
                backlogMilliseconds.map { $0 + Self.backlogSmoothing * (instant - $0) } ?? instant
        }

        listener.onStats(
            ReceiverStats(
                packets: packetsPlayed, lost: playout.lost, sampleRate: lastSampleRate,
                packetsPerSecond: packetsPerSecond,
                interArrivalMilliseconds: arrival.interArrivalMilliseconds,
                jitterMilliseconds: arrival.jitterMilliseconds, discardedSamples: discardedSamples,
                packetsWithDiscardedSamples: packetsWithDiscardedSamples,
                playbackBacklogMilliseconds: backlogMilliseconds, outputPlaying: sink.isPlaying))

        // The desk's buffer applies at once; once per second of audio the output's underruns move
        // an Auto cushion; a fixed one never moves.
        let step = cushion.onPacket(
            setting: settings.playbackBuffer, packetFrames: packetFrames, playedFrames: frames,
            playbackRate: playbackRate, packetRate: rate, capacityFrames: capacityFrames)
        var resize = step.contains(.apply)
        if step.contains(.adjust) && cushion.adjust(underrunsTotal: sink.underrunCount) {
            resize = true
        }
        if resize { appliedTargetFrames = askForCushion(sink) }
    }

    /// The output, opened at the first packet's rate and size with the cushion asked of it; the
    /// same output for every later packet at that rate, and an error at any other.
    private func openedSink(rate: Int, frames: Int, settings: PlayerSettings) throws
        -> any PlaybackSink
    {
        if let sink {
            guard rate == sourceSampleRate else {
                throw ReceiveError.sampleRateChanged(from: sourceSampleRate ?? 0, to: rate)
            }
            return sink
        }
        sourceSampleRate = rate
        playbackRate = rate
        packetFrames = frames
        let opened = try sinkFactory.open(sampleRate: rate, packetFrames: frames)
        capacityFrames = opened.capacityFrames
        // A buffer the desk set before the audio started applies from the first packet.
        _ = cushion.follow(
            settings.playbackBuffer, packetFrames: frames, sampleRate: rate,
            capacityFrames: capacityFrames)
        appliedTargetFrames = askForCushion(opened)
        sink = opened
        return opened
    }

    /// Asks `sink` to queue the cushion before it plays, and answers the frames it applied.
    private func askForCushion(_ sink: any PlaybackSink) -> Int {
        sink.setTargetFrames(
            PlaybackLatency.targetBufferFrames(
                packetFrames: packetFrames, cushionPackets: cushion.cushionPackets)) ?? 0
    }

    private static func milliseconds(_ nanoseconds: UInt64) -> Int64 {
        Int64(nanoseconds / 1_000_000)
    }
}
