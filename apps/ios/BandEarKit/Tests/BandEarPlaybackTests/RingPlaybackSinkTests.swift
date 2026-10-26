import Testing

@testable import BandEarPlayback

/// One rendered stereo frame, read back as whole PCM16 steps so a test reads like the samples it
/// wrote. Silence is `(0, 0)`; no frame a test writes is.
private struct Frame: Equatable, CustomStringConvertible {
    let left: Int
    let right: Int

    var description: String { "(\(left), \(right))" }
}

/// Frames `indices` of a ramp: frame `n` is `(n + 1, -(n + 1))`, never silent, and its two sides
/// tell a torn frame apart from a whole one.
private func ramp(_ indices: Range<Int>) -> [Frame] {
    indices.map { Frame(left: $0 + 1, right: -($0 + 1)) }
}

private func silence(_ count: Int) -> [Frame] {
    Array(repeating: Frame(left: 0, right: 0), count: count)
}

/// Writes `frames` interleaved, as the receive loop hands a packet over, and returns what the sink
/// took.
private func write(_ sink: RingPlaybackSink, _ frames: [Frame]) -> Int {
    let samples = frames.flatMap { [Int16($0.left), Int16($0.right)] }
    return samples.withUnsafeBufferPointer { sink.write($0) }
}

/// Renders `count` frames the way the source node asks for them. The channels start at a value no
/// rendered sample takes, so a render that leaves them untouched cannot pass for silence.
private func render(_ sink: RingPlaybackSink, _ count: Int) -> [Frame] {
    var left = [Float](repeating: 0.5, count: count)
    var right = [Float](repeating: 0.5, count: count)
    left.withUnsafeMutableBufferPointer { left in
        right.withUnsafeMutableBufferPointer { right in
            sink.render(left: left.baseAddress!, right: right.baseAddress!, frameCount: count)
        }
    }
    return zip(left, right).map {
        Frame(left: Int(($0 * 32_768).rounded()), right: Int(($1 * 32_768).rounded()))
    }
}

/// The rules of the output the render callback plays from: priming, underruns, the target, what
/// does not fit, the order across the end of the storage, the conversion, the presented frame and
/// closing.
@Suite("Ring playback sink")
struct RingPlaybackSinkTests {

    @Test("holds at least one frame", arguments: [0, -5])
    func capacityIsAtLeastOneFrame(capacity: Int) {
        #expect(RingPlaybackSink(capacityFrames: capacity).capacityFrames == 1)
    }

    @Test("holds the frames it was created for")
    func capacityIsWhatWasAsked() {
        let capacity = CushionPolicy.capacityFrames(sampleRate: 48_000, packetFrames: 256)

        #expect(RingPlaybackSink(capacityFrames: capacity).capacityFrames == capacity)
    }

    @Test("plays silence until the target is queued, then plays it")
    func primesUntilTheTargetIsQueued() {
        let sink = RingPlaybackSink(capacityFrames: 16)
        #expect(sink.setTargetFrames(4) == 4)

        #expect(write(sink, ramp(0..<3)) == 3)
        #expect(render(sink, 2) == silence(2))
        #expect(write(sink, ramp(3..<4)) == 1)
        #expect(render(sink, 4) == ramp(0..<4))
        #expect(sink.underrunCount == 0)
    }

    @Test("counts no underrun while it primes")
    func primingIsNotAnUnderrun() {
        let sink = RingPlaybackSink(capacityFrames: 16)
        _ = sink.setTargetFrames(4)

        #expect(render(sink, 8) == silence(8))
        #expect(render(sink, 8) == silence(8))
        #expect(write(sink, ramp(0..<2)) == 2)
        #expect(render(sink, 8) == silence(8))
        #expect(sink.underrunCount == 0)
    }

    @Test("counts one underrun when the output runs dry, then primes again")
    func runningDryCountsOnceAndPrimesAgain() {
        let sink = RingPlaybackSink(capacityFrames: 16)
        _ = sink.setTargetFrames(2)
        _ = write(sink, ramp(0..<3))

        #expect(render(sink, 5) == ramp(0..<3) + silence(2))
        #expect(sink.underrunCount == 1)

        #expect(render(sink, 4) == silence(4))
        _ = write(sink, ramp(3..<4))
        #expect(render(sink, 2) == silence(2))
        _ = write(sink, ramp(4..<5))
        #expect(render(sink, 2) == ramp(3..<5))
        #expect(sink.underrunCount == 1)
    }

    /// Taking exactly what is queued does not run dry, so the next frame plays as soon as it is
    /// written instead of waiting for the target again.
    @Test("keeps playing after a render takes exactly what is queued")
    func anExactDrainIsNotAnUnderrun() {
        let sink = RingPlaybackSink(capacityFrames: 16)
        _ = sink.setTargetFrames(4)
        _ = write(sink, ramp(0..<4))

        #expect(render(sink, 4) == ramp(0..<4))
        _ = write(sink, ramp(4..<5))
        #expect(render(sink, 1) == ramp(4..<5))
        #expect(sink.underrunCount == 0)
    }

    @Test(
        "clamps the target to between one frame and the capacity",
        arguments: [(0, 1), (-3, 1), (1, 1), (5, 5), (8, 8), (9, 8), (Int.max, 8)])
    func targetIsClamped(asked: Int, applied: Int) {
        let sink = RingPlaybackSink(capacityFrames: 8)

        #expect(sink.setTargetFrames(asked) == applied)
    }

    /// A target beyond the capacity could never be queued, so the sink would prime for ever.
    @Test("plays a full ring when the target asked for is larger")
    func aTargetBeyondTheCapacityPlaysWhenFull() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(100)
        _ = write(sink, ramp(0..<8))

        #expect(render(sink, 8) == ramp(0..<8))
    }

    @Test("takes only the free space and discards the rest")
    func overflowIsDiscardedAndCounted() {
        let sink = RingPlaybackSink(capacityFrames: 4)
        _ = sink.setTargetFrames(1)

        #expect(write(sink, ramp(0..<6)) == 4)
        #expect(write(sink, ramp(6..<7)) == 0)
        #expect(render(sink, 2) == ramp(0..<2))
        #expect(write(sink, ramp(10..<15)) == 2)
        #expect(render(sink, 5) == ramp(2..<4) + ramp(10..<12) + silence(1))
    }

    @Test("ignores a trailing half frame")
    func aTrailingHalfFrameIsIgnored() {
        let sink = RingPlaybackSink(capacityFrames: 4)
        _ = sink.setTargetFrames(1)
        let samples: [Int16] = [1, -1, 2]

        #expect(samples.withUnsafeBufferPointer { sink.write($0) } == 1)
        #expect(render(sink, 2) == ramp(0..<1) + silence(1))
    }

    /// Five slots and chunks that do not divide them, so writes and reads both start and end at
    /// every slot and several of each straddle the end of the storage.
    @Test("keeps the order across the end of its storage")
    func fifoAcrossTheWrap() {
        let sink = RingPlaybackSink(capacityFrames: 5)
        _ = sink.setTargetFrames(1)
        var next = 0
        var played: [Frame] = []

        for size in [3, 4, 2, 5, 1, 4, 3, 5, 2] {
            #expect(write(sink, ramp(next..<next + size)) == size)
            played += render(sink, size)
            next += size
        }

        #expect(played == ramp(0..<next))
        #expect(sink.underrunCount == 0)
    }

    @Test("reads a frame split by the end of its storage whole")
    func oneReadAcrossTheWrap() {
        let sink = RingPlaybackSink(capacityFrames: 5)
        _ = sink.setTargetFrames(1)
        _ = write(sink, ramp(0..<4))
        #expect(render(sink, 4) == ramp(0..<4))

        #expect(write(sink, ramp(4..<9)) == 5)
        #expect(render(sink, 5) == ramp(4..<9))
    }

    /// The exact quotients: `Int16.min` is full scale, `Int16.max` one step short of it.
    @Test("converts each sample to a float over 32 768, left and right apart")
    func conversionToFloat() {
        let sink = RingPlaybackSink(capacityFrames: 4)
        _ = sink.setTargetFrames(1)
        let samples: [Int16] = [.min, .max, 0, .min, .max, 0]
        _ = samples.withUnsafeBufferPointer { sink.write($0) }
        var left = [Float](repeating: 0.5, count: 3)
        var right = [Float](repeating: 0.5, count: 3)

        left.withUnsafeMutableBufferPointer { left in
            right.withUnsafeMutableBufferPointer { right in
                sink.render(left: left.baseAddress!, right: right.baseAddress!, frameCount: 3)
            }
        }

        #expect(left == [-1, 0, 0.999_969_482_421_875])
        #expect(right == [0.999_969_482_421_875, -1, 0])
    }

    @Test("knows no presented frame until a frame was consumed")
    func noPresentedFrameBeforeTheFirstFrame() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(2)
        #expect(sink.presentedFrame == nil)

        _ = write(sink, ramp(0..<1))
        _ = render(sink, 4)
        #expect(sink.presentedFrame == nil)

        _ = write(sink, ramp(1..<2))
        _ = render(sink, 1)
        #expect(sink.presentedFrame == 1)
    }

    /// Silence rendered after running dry is no frame of the stream, so it does not move the
    /// position on.
    @Test("presents the frames consumed when the output adds no latency")
    func presentedFrameCountsConsumedFrames() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(1)
        _ = write(sink, ramp(0..<5))
        #expect(sink.outputLatencyFrames == 0)

        _ = render(sink, 3)
        #expect(sink.presentedFrame == 3)
        _ = render(sink, 4)
        #expect(sink.presentedFrame == 5)
    }

    /// Frames handed to the hardware are not heard yet: the output latency is what turns the
    /// frames consumed into the frame reaching the listener, Android's presented position.
    @Test(
        "presents the frames consumed less the output latency, never below zero",
        arguments: [(0, 5), (2, 3), (5, 0), (10, 0)])
    func presentedFrameSubtractsTheOutputLatency(latency: Int, presented: Int64) {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(1)
        _ = write(sink, ramp(0..<5))
        _ = render(sink, 5)

        sink.outputLatencyFrames = latency

        #expect(sink.outputLatencyFrames == latency)
        #expect(sink.presentedFrame == presented)
    }

    @Test("takes nothing, refuses a target and renders silence once closed")
    func closedSinkIsSilent() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(1)
        _ = write(sink, ramp(0..<4))
        #expect(render(sink, 2) == ramp(0..<2))

        sink.close()

        #expect(write(sink, ramp(4..<6)) == 0)
        #expect(sink.setTargetFrames(1) == nil)
        #expect(render(sink, 4) == silence(4))
        #expect(sink.underrunCount == 0)
        #expect(sink.presentedFrame == 2)
        #expect(sink.capacityFrames == 8)
    }

    @Test("keeps its underruns and presented frame once closed")
    func closedSinkKeepsItsCounts() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(1)
        _ = write(sink, ramp(0..<2))
        _ = render(sink, 3)
        sink.outputLatencyFrames = 1

        sink.close()
        _ = render(sink, 3)

        #expect(sink.underrunCount == 1)
        #expect(sink.presentedFrame == 1)
        #expect(sink.outputLatencyFrames == 1)
    }

    @Test("renders silence once closed, even while it primes")
    func closedWhilePrimingIsSilent() {
        let sink = RingPlaybackSink(capacityFrames: 8)
        _ = sink.setTargetFrames(2)
        _ = write(sink, ramp(0..<4))

        sink.close()

        #expect(render(sink, 4) == silence(4))
        #expect(sink.presentedFrame == nil)
    }
}
