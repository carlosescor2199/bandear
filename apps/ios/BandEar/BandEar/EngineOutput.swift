import AVFoundation
import BandEarPlayback

/// The app's ``PlaybackSinkFactory``: each output it opens plays the desk's frames through its own
/// `AVAudioEngine`.
///
/// It refuses a stream it cannot size an output for, before allocating anything, because
/// `RingPlaybackSink.init` stops the app when its allocation fails (advisory A5):
/// - ``sampleRates``: 8 to 192 kHz, every rate an audio interface is sold at. Larger rates would
///   only grow the ring, which holds the desk's longest pinned buffer (500 ms) at the stream's rate.
/// - ``packetFrames``: 1 to 16 384 frames. A UDP datagram carries at most 65 527 bytes, so a
///   stereo PCM16 packet after the 20-byte PMON header holds at most 16 376 frames.
///
/// The largest output it opens holds 98 304 frames (500 ms at 192 kHz, rounded up to whole packets
/// of 16 384 frames), 384 KiB of samples.
///
/// The app target defaults to the main actor (`SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`), but the
/// receive loop calls ``open(sampleRate:packetFrames:)`` on its own thread. The type is
/// `nonisolated`, so the call needs no hop and the Swift 6 runtime check finds no main-actor code
/// running off the main thread.
nonisolated struct EngineOutputFactory: PlaybackSinkFactory, Sendable {
    /// The stream rates an output opens at, in hertz.
    static let sampleRates = 8_000...192_000

    /// The packet sizes an output opens for, in frames.
    static let packetFrames = 1...16_384

    /// Sets the app's audio session to playback, activates it, and starts an engine that plays
    /// `sampleRate` stereo. Throws ``EngineOutputError`` for a rate or packet size out of range,
    /// and the audio session's or engine's own error when either refuses.
    func open(sampleRate: Int, packetFrames: Int) throws -> any PlaybackSink {
        guard Self.sampleRates.contains(sampleRate) else {
            throw EngineOutputError.unsupportedSampleRate(sampleRate)
        }
        guard Self.packetFrames.contains(packetFrames) else {
            throw EngineOutputError.unsupportedPacketFrames(packetFrames)
        }
        return try EngineOutput(sampleRate: sampleRate, packetFrames: packetFrames)
    }
}

/// Why ``EngineOutputFactory`` opened no output.
nonisolated enum EngineOutputError: Error, Equatable {
    /// The stream's rate is outside ``EngineOutputFactory/sampleRates``.
    case unsupportedSampleRate(Int)
    /// The stream's packets are outside ``EngineOutputFactory/packetFrames``.
    case unsupportedPacketFrames(Int)
    /// `AVAudioFormat` could not describe the stream at this rate.
    case formatUnavailable(sampleRate: Int)
}

/// One output: a ``RingPlaybackSink`` the receive loop writes into, and the `AVAudioEngine` whose
/// render callback takes the frames out of it.
///
/// The engine plays an `AVAudioSourceNode` in the desk's format: Float32, deinterleaved, two
/// channels, at the stream's rate. Its main mixer converts that to the hardware's rate.
///
/// Three kinds of thread use it:
/// - The receive loop calls every ``PlaybackSink`` member, ``close()`` included, on its own thread.
///   The type is `nonisolated`, like its factory, so none of them belongs to the main actor.
/// - The engine's audio thread calls the render block. The block is made in a `nonisolated`
///   static function and captures only the ring, so it is not main-actor code and holds no
///   main-actor state. It calls `RingPlaybackSink.render(left:right:frameCount:)`, which takes no
///   lock and allocates nothing, and the block itself only reads the buffer list it is handed.
///   It logs nothing. The bridge from block to Swift closure retains and releases the closure's
///   context once per call: an atomic count on an object the block keeps alive, so it never waits
///   and never frees anything.
/// - Notification handlers, for a route change, an engine configuration change and the end of an
///   interruption, hand their work to a private serial queue, where they update
///   ``RingPlaybackSink/outputLatencyFrames`` and restart an engine that stopped. They reach the
///   output through a weak reference, so the
///   engine is never released on the engine's own notification queue, which its documentation
///   says can deadlock.
///
/// The ring outlives every render callback (advisory A4): the render block holds the ring strongly,
/// the source node holds the block, the engine holds the node, and this output holds the engine.
/// The ring's storage is freed only in its `deinit`, so it cannot be freed while the engine still
/// holds a block that can call `render`. ``close()`` stops the engine first and only then closes
/// the ring; a callback that still ran in between would find the ring's storage intact, and one on
/// a closed ring renders silence.
nonisolated final class EngineOutput: PlaybackSink, @unchecked Sendable {
    /// Where the notification handlers run, one at a time.
    private static let notifications = DispatchQueue(label: "com.bandear.ios.engine-output")

    private let ring: RingPlaybackSink
    private let engine: AVAudioEngine
    private let sampleRate: Double

    private let lock = NSLock()
    /// Guarded by `lock`.
    private var closed = false
    /// Guarded by `lock`.
    private var observers: [any NSObjectProtocol] = []

    /// Activates the audio session for playback, builds the ring and the engine for `sampleRate`
    /// and starts the engine. The caller has checked both values against the factory's ranges.
    fileprivate init(sampleRate: Int, packetFrames: Int) throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playback)
        // The shortest buffer the hardware gives, as Android asks for a low-latency track. A longer
        // one granted instead is counted in `outputLatencyFrames`, so a refusal is not an error.
        try? session.setPreferredIOBufferDuration(Self.preferredIOBufferDuration)
        try session.setActive(true)
        guard
            let format = AVAudioFormat(
                commonFormat: .pcmFormatFloat32, sampleRate: Double(sampleRate), channels: 2,
                interleaved: false)
        else {
            throw EngineOutputError.formatUnavailable(sampleRate: sampleRate)
        }
        let ring = RingPlaybackSink(
            capacityFrames: CushionPolicy.capacityFrames(
                sampleRate: sampleRate, packetFrames: packetFrames))
        let engine = AVAudioEngine()
        let node = AVAudioSourceNode(format: format, renderBlock: Self.renderBlock(playing: ring))
        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        self.ring = ring
        self.engine = engine
        self.sampleRate = Double(sampleRate)
        updateOutputLatency()
        observeChanges()
        // If this throws, `deinit` closes what was built.
        try engine.start()
    }

    deinit { close() }

    var capacityFrames: Int { ring.capacityFrames }

    func write(_ samples: UnsafeBufferPointer<Int16>) -> Int { ring.write(samples) }

    func setTargetFrames(_ frames: Int) -> Int? { ring.setTargetFrames(frames) }

    var underrunCount: Int { ring.underrunCount }

    var presentedFrame: Int64? { ring.presentedFrame }

    /// Stops the engine, then closes the ring, once, and lets other apps' audio resume. The engine
    /// is not restarted after this.
    func close() {
        let removed: [any NSObjectProtocol]? = lock.withLock {
            guard !closed else { return nil }
            closed = true
            engine.stop()
            ring.close()
            let current = observers
            observers = []
            return current
        }
        guard let removed else { return }
        for observer in removed {
            NotificationCenter.default.removeObserver(observer)
        }
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    /// 5 ms, a quarter of what iOS gives by default.
    private static let preferredIOBufferDuration: TimeInterval = 0.005

    /// The block the audio thread calls for each buffer. Made here, outside the main actor, so it
    /// is not main-actor code; see the type's documentation.
    ///
    /// It reads the buffer list through plain pointers rather than
    /// `UnsafeMutableAudioBufferListPointer`, whose accessors are not inlined across modules, so the
    /// only call it makes is `RingPlaybackSink.render`.
    private static func renderBlock(playing ring: RingPlaybackSink) -> AVAudioSourceNodeRenderBlock {
        { _, _, frameCount, outputData in
            guard outputData.pointee.mNumberBuffers >= 2 else { return kAudio_ParamError }
            // The list's buffers follow one another from `mBuffers`, its first.
            return withUnsafeMutablePointer(to: &outputData.pointee.mBuffers) { first in
                let second = first + 1
                guard let left = first.pointee.mData?.assumingMemoryBound(to: Float.self),
                    let right = second.pointee.mData?.assumingMemoryBound(to: Float.self)
                else { return kAudio_ParamError }
                let room =
                    Int(min(first.pointee.mDataByteSize, second.pointee.mDataByteSize))
                    / MemoryLayout<Float>.stride
                ring.render(left: left, right: right, frameCount: min(Int(frameCount), room))
                return noErr
            }
        }
    }

    /// The frames between the render callback and the listener: the hardware's output latency and
    /// one I/O buffer, at the stream's rate.
    private func updateOutputLatency() {
        let session = AVAudioSession.sharedInstance()
        ring.outputLatencyFrames = Int(
            ((session.outputLatency + session.ioBufferDuration) * sampleRate).rounded())
    }

    /// The engine stops itself when the hardware's rate or channel count changes, for example when
    /// headphones are plugged in, and the system stops it for an interruption such as a call. It is
    /// started again unless the output was closed; if that fails the output stays silent, the ring
    /// fills, and the receive loop's writes come back short.
    private func restartIfStopped() {
        lock.withLock {
            guard !closed else { return }
            try? engine.start()
        }
        updateOutputLatency()
    }

    private func observeChanges() {
        let target = WeakEngineOutput(self)
        let center = NotificationCenter.default
        let routeChange = center.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: nil
        ) { _ in
            Self.notifications.async { target.output?.updateOutputLatency() }
        }
        let configurationChange = center.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil
        ) { _ in
            Self.notifications.async { target.output?.restartIfStopped() }
        }
        let interruption = center.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: nil
        ) { notification in
            guard Self.interruptionEndedAskingToResume(notification) else { return }
            Self.notifications.async { target.output?.restartIfStopped() }
        }
        lock.withLock { observers = [routeChange, configurationChange, interruption] }
    }

    /// Whether `notification` ends an interruption after which the system expects playback to
    /// resume (`AVAudioSession.InterruptionOptions.shouldResume`).
    private static func interruptionEndedAskingToResume(_ notification: Notification) -> Bool {
        let info = notification.userInfo
        guard let type = info?[AVAudioSessionInterruptionTypeKey] as? UInt,
            AVAudioSession.InterruptionType(rawValue: type) == .ended,
            let options = info?[AVAudioSessionInterruptionOptionKey] as? UInt
        else { return false }
        return AVAudioSession.InterruptionOptions(rawValue: options).contains(.shouldResume)
    }
}

/// How a notification handler reaches an ``EngineOutput`` without keeping it alive.
nonisolated private final class WeakEngineOutput: Sendable {
    weak let output: EngineOutput?

    init(_ output: EngineOutput) { self.output = output }
}
