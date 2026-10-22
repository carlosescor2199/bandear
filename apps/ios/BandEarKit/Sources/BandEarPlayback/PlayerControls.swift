import BandEarProtocol
import Foundation

/// What the receive loop plays by that is set elsewhere: the musician's master controls, whether
/// the desk owns the mix, and the desk's playback and redundancy settings.
///
/// Android's player reads each from a volatile field of `ClientMixState` wherever it uses it
/// (`PmonPlayer.kt:207,253,263-268,321`), and asks its listener whether the server owns the mix. The
/// defaults are `ClientMixState`'s: full volume, no cap, not muted, Auto, no hold, and a desk that
/// does not own the mix yet.
public struct PlayerSettings: Equatable, Sendable {
    /// The phone's master volume, in percent; local protection clamps it to 0-100.
    public var volumePercent: Int
    /// The highest level the phone plays, in percent of full scale; local protection clamps it to
    /// 0-100. The caller sets the lower of the musician's cap and the engineer's limit.
    public var maxLevelPercent: Int
    public var muted: Bool
    /// True once the desk acknowledged a mix: it then applies the master controls itself, so the
    /// phone leaves the audio as it comes (``Pcm16Processor/applyLocalProtection(_:volumePercent:maxLevelPercent:muted:serverOwnsMix:)``).
    public var serverOwnsMix: Bool
    /// The desk's playback buffer, which the cushion follows (``CushionController``).
    public var playbackBuffer: PlaybackBufferSetting
    /// The packets held back for the desk's delayed copies (``ReorderHold/holdPackets``).
    public var redundancyHoldPackets: Int

    public init(
        volumePercent: Int = 100, maxLevelPercent: Int = 100, muted: Bool = false,
        serverOwnsMix: Bool = false, playbackBuffer: PlaybackBufferSetting = .auto,
        redundancyHoldPackets: Int = 0
    ) {
        self.volumePercent = volumePercent
        self.maxLevelPercent = maxLevelPercent
        self.muted = muted
        self.serverOwnsMix = serverOwnsMix
        self.playbackBuffer = playbackBuffer
        self.redundancyHoldPackets = redundancyHoldPackets
    }
}

/// The ``PlayerSettings`` the main thread hands the receive loop.
///
/// The loop reads one ``snapshot`` per wake-up, so every value it plays a wake-up's packets with
/// was set together, and a change applies from the next wake-up: at once while packets flow, at
/// most one receive timeout later while they do not. Android reads each field as it needs it, so a
/// change there can apply between two packets of one wake-up; that difference is a few
/// milliseconds of audio at most.
///
/// A lock guards the settings. The receive thread is not real time, so a lock taken once per
/// wake-up costs it nothing; the render callback never reads these.
public final class PlayerControls: @unchecked Sendable {
    private let lock = NSLock()
    private var settings: PlayerSettings

    public init(_ settings: PlayerSettings = PlayerSettings()) {
        self.settings = settings
    }

    /// The settings now, all read together.
    public var snapshot: PlayerSettings {
        lock.lock()
        defer { lock.unlock() }
        return settings
    }

    /// Changes the settings in one step, so the loop never reads half a change.
    public func update(_ change: (inout PlayerSettings) -> Void) {
        lock.lock()
        defer { lock.unlock() }
        change(&settings)
    }
}
