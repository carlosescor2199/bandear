/// Local protection: the master volume, ceiling and mute the phone applies to the desk's audio
/// itself, for as long as the desk does not own this phone's mix.
///
/// Ported from the Android client's `Pcm16Processor.kt`. The audio the phone receives is the desk's
/// finished stereo mix, so there is nothing per source left to scale: only the master controls
/// apply, and the per-source faders and pans do nothing without a working control channel.
///
/// - Volume is linear, `volumePercent` clamped to 0-100 over 100, with no dB curve and no ramp.
/// - The ceiling is full scale times `maxLevelPercent` clamped to 0-100, in integer division as
///   Android computes it: 25 % of 32 767 is 8 191.
/// - Muted, the gain is 0 whatever the volume.
/// - Each sample is scaled, clamped to the ceiling, then rounded with a half going towards positive
///   infinity, as Kotlin's `roundToInt` does (`Math.round`). Swift's `rounded()` sends a half away
///   from zero instead, which would turn -1.5 into -2 where Android plays -1.
///   `(x + 0.5).rounded(.down)` agrees with `Math.round` on every sample at every whole-percent
///   volume (all 6 619 136 of them, checked against a transcription of the JDK's).
///
/// Where Android returns a new array per packet, the samples are processed where they lie, so the
/// receive path allocates nothing: the player hands over the region of its own buffer a packet was
/// copied into. The array overloads are for callers that hold an array; they allocate nothing
/// either while the array's storage is not shared.
public enum Pcm16Processor {
    /// Applies the master controls to the interleaved PCM16 `samples`, in place. Neutral controls
    /// (100 %, 100 %, not muted) leave every sample as it was.
    public static func apply(
        _ samples: UnsafeMutableBufferPointer<Int16>, volumePercent: Int, maxLevelPercent: Int,
        muted: Bool
    ) {
        let gain = muted ? 0.0 : Double(clampedPercent(volumePercent)) / 100.0
        let ceiling = Double(Int(Int16.max) * clampedPercent(maxLevelPercent) / 100)
        for index in samples.indices {
            let limited = min(max(Double(samples[index]) * gain, -ceiling), ceiling)
            let rounded = (limited + 0.5).rounded(.down)
            samples[index] = Int16(min(max(rounded, Double(Int16.min)), Double(Int16.max)))
        }
    }

    /// Applies the master controls to an array of interleaved PCM16 samples, in place.
    public static func apply(
        _ samples: inout [Int16], volumePercent: Int, maxLevelPercent: Int, muted: Bool
    ) {
        samples.withUnsafeMutableBufferPointer {
            apply($0, volumePercent: volumePercent, maxLevelPercent: maxLevelPercent, muted: muted)
        }
    }

    /// The samples to play for one received packet, in place.
    ///
    /// Once the desk has acknowledged a mix for this phone it owns that mix: it applies the gains,
    /// pans, volume, ceiling and mute itself, and keeps applying them for the rest of the session,
    /// even while the control channel is down. Applied here as well they would count twice, so
    /// `serverOwnsMix` leaves the samples untouched. Until then the desk plays its neutral default,
    /// and only the phone can protect the musician's ears.
    public static func applyLocalProtection(
        _ samples: UnsafeMutableBufferPointer<Int16>, volumePercent: Int, maxLevelPercent: Int,
        muted: Bool, serverOwnsMix: Bool
    ) {
        guard !serverOwnsMix else { return }
        apply(samples, volumePercent: volumePercent, maxLevelPercent: maxLevelPercent, muted: muted)
    }

    /// The samples to play for one received packet held in an array, in place.
    public static func applyLocalProtection(
        _ samples: inout [Int16], volumePercent: Int, maxLevelPercent: Int, muted: Bool,
        serverOwnsMix: Bool
    ) {
        samples.withUnsafeMutableBufferPointer {
            applyLocalProtection(
                $0, volumePercent: volumePercent, maxLevelPercent: maxLevelPercent, muted: muted,
                serverOwnsMix: serverOwnsMix)
        }
    }

    private static func clampedPercent(_ percent: Int) -> Int {
        min(max(percent, 0), 100)
    }
}
