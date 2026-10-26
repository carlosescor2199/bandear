import BandEarRing

/// The ``PlaybackSink`` the app plays from: the receive loop writes the desk's frames into a
/// lock-free ring, and the audio engine's render callback takes them out through
/// ``render(left:right:frameCount:)``.
///
/// The render side is a two-state machine:
/// - Priming, where it starts: silence until at least the target (``setTargetFrames(_:)``) is
///   queued, then playing.
/// - Playing: the frames asked for, each sample as `Float(sample) / 32 768`. When fewer are queued
///   it plays those, fills the rest with silence, counts one underrun and primes again, which
///   rebuilds the cushion after a dropout instead of playing each frame the moment it lands.
///
/// Once closed it takes nothing and renders silence.
///
/// Two threads, each with its own members, and nothing between them but the ring's atomics:
/// - The receive loop: every ``PlaybackSink`` member.
/// - The render callback: ``render(left:right:frameCount:)`` only. It takes no lock, allocates
///   nothing and touches no array or closure, so the audio thread never waits on the loop.
/// - ``outputLatencyFrames`` may be set from any thread.
///
/// The storage is allocated once, here, and released when the sink is: ``close()`` keeps it, so a
/// render callback already running when the loop closes the sink can finish.
public final class RingPlaybackSink: PlaybackSink, @unchecked Sendable {
    /// The most frames queued at once, at least one.
    public let capacityFrames: Int

    private let ring: OpaquePointer

    /// Whether the render side is waiting for the target. Only the render callback touches it.
    private var priming = true

    /// The frames between the render callback and the listener: the output's own buffers and the
    /// hardware's. 0 until the app reads it from the audio session.
    public var outputLatencyFrames: Int {
        get { Int(bandear_ring_output_latency(ring)) }
        set { bandear_ring_set_output_latency(ring, Int64(newValue)) }
    }

    /// A sink holding at most `capacityFrames` frames (at least one), sized by
    /// ``CushionPolicy/capacityFrames(sampleRate:packetFrames:)``, with a target of one frame.
    public init(capacityFrames: Int) {
        let capacity = max(capacityFrames, 1)
        guard let ring = bandear_ring_create(Int64(capacity)) else {
            fatalError("could not allocate a playback ring of \(capacity) frames")
        }
        self.capacityFrames = capacity
        self.ring = ring
    }

    deinit { bandear_ring_destroy(ring) }

    /// Takes as many whole frames as there is free space for. A trailing half frame is not a
    /// frame and is ignored.
    public func write(_ samples: UnsafeBufferPointer<Int16>) -> Int {
        guard let base = samples.baseAddress else { return 0 }
        return Int(bandear_ring_write(ring, base, Int64(samples.count / 2)))
    }

    /// Applies `frames` clamped to one frame through ``capacityFrames``: a target the ring cannot
    /// hold would prime for ever.
    public func setTargetFrames(_ frames: Int) -> Int? {
        guard !bandear_ring_is_closed(ring) else { return nil }
        let applied = min(max(frames, 1), capacityFrames)
        bandear_ring_set_target(ring, Int64(applied))
        return applied
    }

    public var underrunCount: Int { Int(bandear_ring_underruns(ring)) }

    /// The frames consumed less ``outputLatencyFrames``, never below zero, once the render side has
    /// consumed one. Frames handed to the hardware are not heard yet; taking the output latency
    /// off is what makes this Android's presented position. Silence is not counted.
    public var presentedFrame: Int64? {
        let consumed = bandear_ring_frames_consumed(ring)
        guard consumed > 0 else { return nil }
        return max(0, consumed - Int64(outputLatencyFrames))
    }

    public func close() {
        bandear_ring_close(ring)
    }

    /// Fills `frameCount` frames of `left` and `right`. The audio engine's render callback calls it;
    /// nothing else may.
    public func render(
        left: UnsafeMutablePointer<Float>, right: UnsafeMutablePointer<Float>, frameCount: Int
    ) {
        guard frameCount > 0 else { return }
        guard !bandear_ring_is_closed(ring) else {
            silence(left, right, from: 0, to: frameCount)
            return
        }
        if priming {
            guard bandear_ring_queued(ring) >= bandear_ring_target(ring) else {
                silence(left, right, from: 0, to: frameCount)
                return
            }
            priming = false
        }
        var played = 0
        while played < frameCount {
            var start: UnsafePointer<Int16>?
            let run = Int(bandear_ring_peek(ring, Int64(frameCount - played), &start))
            guard run > 0, let start else { break }
            for frame in 0..<run {
                left[played + frame] = Float(start[2 * frame]) / 32_768
                right[played + frame] = Float(start[2 * frame + 1]) / 32_768
            }
            bandear_ring_consume(ring, Int64(run))
            played += run
        }
        if played < frameCount {
            silence(left, right, from: played, to: frameCount)
            bandear_ring_count_underrun(ring)
            priming = true
        }
    }

    private func silence(
        _ left: UnsafeMutablePointer<Float>, _ right: UnsafeMutablePointer<Float>, from start: Int,
        to end: Int
    ) {
        (left + start).update(repeating: 0, count: end - start)
        (right + start).update(repeating: 0, count: end - start)
    }
}
