/// Where the receive loop puts the audio it plays: this client's stand-in for Android's `AudioTrack`
/// (`PmonPlayer.kt:382-471`).
///
/// The loop calls every member from its own thread, never from the render callback. An
/// implementation hands the frames on to the render callback without locks, and its render side
/// allocates nothing.
///
/// Unlike `AudioTrack.write(..., WRITE_BLOCKING)`, ``write(_:)`` never waits. What does not fit is
/// discarded and reported, as Android reports a short write.
public protocol PlaybackSink: AnyObject {
    /// The most frames the sink holds at once (Android's `bufferCapacityInFrames`).
    var capacityFrames: Int { get }

    /// Takes interleaved stereo PCM16 frames without blocking, and returns how many frames it took.
    /// The frames after those did not fit and are discarded.
    func write(_ samples: UnsafeBufferPointer<Int16>) -> Int

    /// Asks the sink to have this many frames queued before it plays, again after it runs dry: the
    /// cushion. Returns the frames it applied, or `nil` when it could not apply any (Android's
    /// `setBufferSizeInFrames`, which returns -1 then).
    func setTargetFrames(_ frames: Int) -> Int?

    /// The times the output has run dry since the sink opened (Android's `underrunCount`).
    var underrunCount: Int { get }

    /// The frame reaching the listener now, counted from the first frame written, or `nil` until the
    /// sink knows it (Android's `AudioTimestamp.framePosition` when `getTimestamp` succeeds).
    var presentedFrame: Int64? { get }

    /// Stops the output and lets it go. Nothing is written after this.
    func close()
}

/// Opens the sink once the first packet tells the stream's rate and packet size (Android opens its
/// track then, `PmonPlayer.kt:246-256`).
public protocol PlaybackSinkFactory {
    func open(sampleRate: Int, packetFrames: Int) throws -> any PlaybackSink
}
