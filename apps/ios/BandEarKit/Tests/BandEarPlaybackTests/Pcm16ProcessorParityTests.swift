import Testing

@testable import BandEarPlayback

/// The Android client's own tests for local protection, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/Pcm16ProcessorTest.kt`: same samples, same
/// controls, same expected samples.
///
/// One translation, deliberate: Android's `apply` returns a new `ShortArray` and leaves its input
/// alone. Here the samples are processed where they lie, so the receive path allocates nothing per
/// packet, and each test reads the expected values back out of the buffer it passed in.
/// `Pcm16ProcessorTests` pins the in-place part and the Kotlin rounding.
@Suite("Local protection: Android parity")
struct Pcm16ProcessorParityTests {

    // Pcm16ProcessorTest.kt:7-17
    @Test func appliesVolumeAndHardMaximumWithoutClipping() {
        var samples: [Int16] = [Int16.min, -16_000, 16_000, Int16.max]

        Pcm16Processor.apply(&samples, volumePercent: 50, maxLevelPercent: 25, muted: false)

        #expect(samples == [-8_191, -8_000, 8_000, 8_191])
    }

    // Pcm16ProcessorTest.kt:19-29
    @Test func muteOverridesVolumeAndMaximum() {
        var samples: [Int16] = [Int16.min, 0, Int16.max]

        Pcm16Processor.apply(&samples, volumePercent: 100, maxLevelPercent: 100, muted: true)

        #expect(samples == [0, 0, 0])
    }

    // Pcm16ProcessorTest.kt:31-45
    @Test func alreadyMixedStereoStreamIsOnlyAffectedByMasterControls() {
        // The received stream is the server's finished stereo mix, so the local fallback must
        // scale every sample by the same master volume instead of applying per-channel gains.
        var samples: [Int16] = [20_000, 10_000, -20_000, -10_000]

        Pcm16Processor.apply(&samples, volumePercent: 50, maxLevelPercent: 100, muted: false)

        #expect(samples == [10_000, 5_000, -10_000, -5_000])
    }

    // Pcm16ProcessorTest.kt:47-59
    @Test func leavesAnAlreadyMixedStreamUnchangedWhenMasterControlsAreNeutral() {
        let original: [Int16] = [12_000, 4_000, -12_000, -4_000]
        var samples = original

        Pcm16Processor.apply(&samples, volumePercent: 100, maxLevelPercent: 100, muted: false)

        #expect(samples == original)
    }

    // Pcm16ProcessorTest.kt:61-74
    @Test func skipsLocalProcessingWhenTheServerOwnsTheMix() {
        let original: [Int16] = [-16_000, 0, 16_000]
        var samples = original

        Pcm16Processor.applyLocalProtection(
            &samples, volumePercent: 0, maxLevelPercent: 25, muted: true, serverOwnsMix: true)

        #expect(samples == original)
    }

    // Pcm16ProcessorTest.kt:76-87
    @Test func appliesLocalProcessingWhenTheServerNeverOwnedTheMix() {
        var samples: [Int16] = [-16_000, 16_000]

        Pcm16Processor.applyLocalProtection(
            &samples, volumePercent: 50, maxLevelPercent: 100, muted: false, serverOwnsMix: false)

        #expect(samples == [-8_000, 8_000])
    }
}
