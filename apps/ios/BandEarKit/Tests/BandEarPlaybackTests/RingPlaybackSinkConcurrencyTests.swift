import Foundation
import Testing

@testable import BandEarPlayback

/// Frame `index` of the stream the producer writes: never silent, and each side tied to the other
/// so a frame read half old and half new shows.
private func streamFrame(_ index: Int) -> (left: Int16, right: Int16) {
    let level = Int16(index % Int(Int16.max) + 1)
    return (level, -level)
}

/// What the consumer thread saw, handed to the test once the thread has signalled.
private final class ConsumerReport: @unchecked Sendable {
    private let lock = NSLock()
    private var storedPlayed = 0
    private var storedMismatch: String?

    func record(played: Int, mismatch: String?) {
        lock.lock()
        defer { lock.unlock() }
        storedPlayed = played
        storedMismatch = mismatch
    }

    var played: Int {
        lock.lock()
        defer { lock.unlock() }
        return storedPlayed
    }

    var mismatch: String? {
        lock.lock()
        defer { lock.unlock() }
        return storedMismatch
    }
}

/// Starts `body` on a thread of its own and returns a semaphore it signals when done.
///
/// Swift Testing cancels cooperatively, so a synchronous loop that never yields ignores its time
/// limit and stalls the whole run. Waiting on the semaphore with a timeout turns that hang into a
/// failed expectation; the stuck thread is left behind and dies when the test process exits.
private func onThread(_ body: @escaping @Sendable () -> Void) -> DispatchSemaphore {
    let done = DispatchSemaphore(value: 0)
    Thread {
        body()
        done.signal()
    }.start()
    return done
}

/// The receive loop and the render callback at once, as the app runs them: one thread writes,
/// another renders, with nothing between them but the ring's atomics.
@Suite("Ring playback sink across threads")
struct RingPlaybackSinkConcurrencyTests {

    /// Uneven chunk sizes on both sides and a capacity that divides none of them, so the two
    /// positions meet at every slot, the producer often finds the ring full and the consumer often
    /// finds it dry. The producer offers again whatever was not taken, so the stream it writes has
    /// no gap: every frame the consumer plays, silence aside, must be the next one, whole.
    ///
    /// Under Thread Sanitizer this is also the check of the ring's memory ordering: a release or an
    /// acquire missing from the ring shows as a race between the producer's stores into a slot and
    /// the consumer's reads of it. A full ring, or a dry one, yields rather than spins.
    @Test("plays every frame written, in order and whole, while another thread writes")
    func concurrentWriteAndRender() {
        let totalFrames = 400_000
        let stream = (0..<totalFrames).flatMap {
            let frame = streamFrame($0)
            return [frame.left, frame.right]
        }
        let sink = RingPlaybackSink(capacityFrames: 1_021)
        _ = sink.setTargetFrames(97)
        let report = ConsumerReport()
        let deadline = DispatchTime.now() + .seconds(10)

        let producer = onThread {
            let chunks = [1, 37, 256, 5, 480, 113, 2, 64]
            var next = 0
            var turn = 0
            stream.withUnsafeBufferPointer { stream in
                while next < totalFrames && DispatchTime.now() < deadline {
                    let frames = min(chunks[turn % chunks.count], totalFrames - next)
                    turn += 1
                    let taken = sink.write(
                        UnsafeBufferPointer(rebasing: stream[2 * next..<2 * (next + frames)]))
                    next += taken
                    if taken < frames { sched_yield() }
                }
            }
            // The last frames may sit below the target; a target of one plays them.
            _ = sink.setTargetFrames(1)
        }

        let consumer = onThread {
            let requests = [128, 3, 512, 41, 256, 1, 999, 77]
            let longest = requests.max()!
            var left = [Float](repeating: 0, count: longest)
            var right = [Float](repeating: 0, count: longest)
            var played = 0
            var mismatch: String?
            var turn = 0
            while played < totalFrames && mismatch == nil && DispatchTime.now() < deadline {
                let frames = requests[turn % requests.count]
                turn += 1
                left.withUnsafeMutableBufferPointer { left in
                    right.withUnsafeMutableBufferPointer { right in
                        sink.render(
                            left: left.baseAddress!, right: right.baseAddress!, frameCount: frames)
                    }
                }
                let before = played
                for offset in 0..<frames {
                    let heard = (left: Int(left[offset] * 32_768), right: Int(right[offset] * 32_768))
                    if heard.left == 0 && heard.right == 0 { continue }
                    let expected = streamFrame(played)
                    guard heard.left == Int(expected.left) && heard.right == Int(expected.right)
                    else {
                        mismatch = "frame \(played): heard \(heard), expected \(expected)"
                        break
                    }
                    played += 1
                }
                if played == before { sched_yield() }
            }
            report.record(played: played, mismatch: mismatch)
        }

        #expect(producer.wait(timeout: .now() + 15) == .success)
        #expect(consumer.wait(timeout: .now() + 15) == .success)
        #expect(report.mismatch == nil)
        #expect(report.played == totalFrames)
    }
}
