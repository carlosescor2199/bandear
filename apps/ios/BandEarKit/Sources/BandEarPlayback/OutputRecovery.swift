/// The pure rules for bringing the app's audio output back after the system stopped it: when to
/// start it again, and whether it is playing.
///
/// The system stops the output for an interruption, such as a call or another app's audio, and the
/// engine stops itself when the hardware's format changes, such as when headphones are plugged in.
/// The app forwards each of those as an ``Event``, starts the output again whenever
/// ``handle(_:)`` says so, and reports how that went with ``restartSucceeded()`` or
/// ``restartFailed()``. ``isPlaying`` then says whether the musician hears the desk, so a silent
/// output is shown rather than left behind a connected session.
///
/// It follows Apple's guidance for interruptions:
/// - Nothing starts while interrupted, because the interrupting app or call has the audio.
/// - An interruption that ends asking to resume (`AVAudioSession.InterruptionOptions.shouldResume`)
///   starts the output again at once.
/// - One that ends without asking leaves it stopped until the app becomes active again, because the
///   musician turned to another app's audio and has not come back yet.
///
/// The app becoming active also ends an interruption, since Apple promises no end for every one:
/// another app that keeps its session active never sends it. The musician coming back to the app is
/// then the end; a system that still holds the audio, such as a call in progress, refuses the
/// restart, and the output stays stopped until the next event.
///
/// A restart that fails leaves the output stopped, and the next event that allows a restart tries
/// again. It holds two flags and no clock, so the app decides when its events happen.
public struct OutputRecovery: Equatable, Sendable {
    /// What the app tells the recovery about its audio.
    public enum Event: Equatable, Sendable {
        /// The system stopped the output for an interruption.
        case interruptionBegan
        /// The interruption is over. `shouldResume` is whether the system expects playback to go on.
        case interruptionEnded(shouldResume: Bool)
        /// The engine stopped itself because the hardware's format changed.
        case configurationChanged
        /// The app came to the foreground, or back from a system alert.
        case becameActive
    }

    /// Whether the output is reaching the listener, as far as the recovery knows: `true` from the
    /// start, because the engine was just started; `false` from an event that stopped it until a
    /// restart succeeds.
    public private(set) var isPlaying = true

    /// Whether an interruption has begun and not ended.
    public private(set) var isInterrupted = false

    /// A recovery for an output that was just started: playing, and not interrupted.
    public init() {}

    /// Takes in `event` and answers whether to start the output again now. When it answers `true`,
    /// the caller tries, and reports the outcome with ``restartSucceeded()`` or ``restartFailed()``.
    public mutating func handle(_ event: Event) -> Bool {
        switch event {
        case .interruptionBegan:
            isPlaying = false
            isInterrupted = true
            return false
        case .interruptionEnded(let shouldResume):
            isInterrupted = false
            if !shouldResume { isPlaying = false }
            return shouldResume
        case .configurationChanged:
            isPlaying = false
            return !isInterrupted
        case .becameActive:
            isInterrupted = false
            return !isPlaying
        }
    }

    /// The output started again: it plays.
    public mutating func restartSucceeded() {
        isPlaying = true
    }

    /// The output did not start: it stays stopped until the next event that allows a restart.
    public mutating func restartFailed() {
        isPlaying = false
    }
}
