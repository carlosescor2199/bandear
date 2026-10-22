import Testing

@testable import BandEarPlayback

/// What local protection does beyond the Android parity suite: Kotlin's rounding reproduced where
/// Swift's own would differ, the samples processed where they lie, and controls out of range
/// clamped as Android clamps them, which its own tests never exercise.
@Suite("Local protection")
struct Pcm16ProcessorTests {

    /// Kotlin's `roundToInt` is `Math.round`, which sends a half towards positive infinity. Swift's
    /// `rounded()` sends it away from zero, so the two disagree on every negative half: -1.5 is -1
    /// on Android and would be -2 here.
    @Test("rounds a half towards positive infinity, as Kotlin's roundToInt does")
    func halvesRoundUp() {
        // At half volume these are -1.5, -0.5, 0.5 and 1.5.
        var samples: [Int16] = [-3, -1, 1, 3]

        Pcm16Processor.apply(&samples, volumePercent: 50, maxLevelPercent: 100, muted: false)

        #expect(samples == [-1, 0, 1, 2])
    }

    @Test("processes an array where it lies, keeping its storage")
    func arrayInPlace() {
        var samples: [Int16] = [20_000, -20_000]
        let before = samples.withUnsafeBufferPointer { $0.baseAddress }

        Pcm16Processor.applyLocalProtection(
            &samples, volumePercent: 50, maxLevelPercent: 100, muted: false, serverOwnsMix: false)

        #expect(samples == [10_000, -10_000])
        #expect(samples.withUnsafeBufferPointer { $0.baseAddress } == before)
    }

    /// The player hands over the region of its own buffer a packet was copied into: only that
    /// region may change.
    @Test("processes only the region of a buffer it is given")
    func bufferRegion() {
        var buffer: [Int16] = [1_000, 2_000, 3_000, 4_000, 5_000, 6_000]

        buffer.withUnsafeMutableBufferPointer { whole in
            let region = UnsafeMutableBufferPointer(rebasing: whole[2..<4])
            Pcm16Processor.applyLocalProtection(
                region, volumePercent: 50, maxLevelPercent: 100, muted: false, serverOwnsMix: false)
        }

        #expect(buffer == [1_000, 2_000, 1_500, 2_000, 5_000, 6_000])
    }

    @Test("leaves a buffer it is given alone when the desk owns the mix")
    func bufferPassThrough() {
        var buffer: [Int16] = [-16_000, 0, 16_000]

        buffer.withUnsafeMutableBufferPointer {
            Pcm16Processor.applyLocalProtection(
                $0, volumePercent: 0, maxLevelPercent: 25, muted: true, serverOwnsMix: true)
        }

        #expect(buffer == [-16_000, 0, 16_000])
    }

    /// Unclamped, a volume of 150 % would amplify a mix that local protection may only attenuate,
    /// and a ceiling of 150 % would be 49 150, past full scale.
    @Test("clamps volume and ceiling to 0-100 %")
    func controlsAreClamped() {
        var loud: [Int16] = [Int16.min, -10_000, 10_000, Int16.max]
        Pcm16Processor.apply(&loud, volumePercent: 150, maxLevelPercent: 150, muted: false)
        #expect(loud == [-32_767, -10_000, 10_000, 32_767])

        var silent: [Int16] = [Int16.min, -10_000, 10_000, Int16.max]
        Pcm16Processor.apply(&silent, volumePercent: -20, maxLevelPercent: 100, muted: false)
        #expect(silent == [0, 0, 0, 0])

        var capped: [Int16] = [Int16.min, -10_000, 10_000, Int16.max]
        Pcm16Processor.apply(&capped, volumePercent: 100, maxLevelPercent: -5, muted: false)
        #expect(capped == [0, 0, 0, 0])
    }
}
