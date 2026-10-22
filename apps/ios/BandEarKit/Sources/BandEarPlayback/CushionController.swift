import BandEarProtocol

/// The pure rules behind the playback cushion: how many packets of audio the player keeps queued
/// ahead of what it is playing.
///
/// Ported from the Android client's `PlaybackLatency.kt`. The cushion *is* the playback latency, so
/// it is kept as small as this phone and this Wi-Fi can sustain: walked down while the player
/// reports no underruns, pushed back up the moment it does. Only the parts that are about the
/// cushion live here. What Android measures off `AudioTrack` (its capacity rules, its timestamp
/// backlog) belongs to the iOS audio output, which measures differently.
public enum CushionPolicy {
    /// The smallest cushion, in packets.
    public static let minimumPackets = 1
    /// The largest cushion the adaptive controller will grow to, in packets.
    public static let maximumPackets = 4
    /// Where a new session starts, in packets.
    public static let startPackets = 2
    /// Clean intervals needed before the cushion may shrink by one packet, so one quiet second
    /// cannot ratchet it to the floor and a burst right after does not undo it at once.
    static let cleanIntervalsBeforeShrink = 3
    /// How much audio is played between two adjustments.
    public static let adjustIntervalMilliseconds = 1_000

    /// One adjustment's outcome: the cushion to use next and the clean intervals to carry.
    public struct Decision: Equatable, Sendable {
        public let cushionPackets: Int
        public let cleanIntervals: Int

        public init(cushionPackets: Int, cleanIntervals: Int) {
            self.cushionPackets = cushionPackets
            self.cleanIntervals = cleanIntervals
        }
    }

    /// The cushion after one adjustment interval: one packet more the moment an underrun happened,
    /// one packet less after ``cleanIntervalsBeforeShrink`` clean intervals in a row, otherwise
    /// unchanged. At the floor the clean count keeps climbing rather than resetting, exactly as
    /// Android's `nextCushion` does.
    public static func next(
        cushionPackets: Int, cleanIntervals: Int, underrunsInInterval: Int
    ) -> Decision {
        if underrunsInInterval > 0 {
            return Decision(
                cushionPackets: min(cushionPackets + 1, maximumPackets), cleanIntervals: 0)
        }
        let clean = cleanIntervals + 1
        if clean >= cleanIntervalsBeforeShrink && cushionPackets > minimumPackets {
            return Decision(cushionPackets: cushionPackets - 1, cleanIntervals: 0)
        }
        return Decision(cushionPackets: cushionPackets, cleanIntervals: clean)
    }

    /// The cushion, in packets, for a buffer the desk pinned at `milliseconds`: rounded *up* to
    /// whole packets so the phone never plays less than the engineer set, at least one, and at most
    /// the whole packets the player holds. One packet before the first packet sized anything.
    public static func fixedCushionPackets(
        milliseconds: Int, packetFrames: Int, sampleRate: Int, capacityFrames: Int
    ) -> Int {
        guard packetFrames > 0, sampleRate > 0 else { return 1 }
        let packets = ceilingDivision(max(milliseconds, 0) * sampleRate, 1_000 * packetFrames)
        let most = max(capacityFrames / packetFrames, 1)
        return min(max(packets, 1), most)
    }

    /// The frames played between two adjustments: ``adjustIntervalMilliseconds`` at `sampleRate`,
    /// or at 48 kHz when the rate cannot be used. Counted in frames, so a second is a second
    /// whatever block size the desk sends (256-frame blocks are 187.5 packets a second, 480-frame
    /// blocks 100).
    public static func adjustIntervalFrames(sampleRate: Int) -> Int {
        let rate = sampleRate > 0 ? sampleRate : 48_000
        return max(rate * adjustIntervalMilliseconds / 1_000, 1)
    }

    /// The frames a player must be able to hold so the longest buffer the desk can pin fits, in
    /// whole packets so its last packet fits too (just the frames when the packet size is unknown).
    ///
    /// Android opens its `AudioTrack` this large and then sets the buffer size *down*; the iOS
    /// player sizes its own buffer with it. Either way room costs memory (about 94 KB at 48 kHz),
    /// not latency: what is queued is the cushion, not the capacity.
    public static func capacityFrames(sampleRate: Int, packetFrames: Int) -> Int {
        let frames = ceilingDivision(
            Int(PlaybackBufferSetting.maximumFixedMilliseconds) * max(sampleRate, 0), 1_000)
        guard packetFrames > 0 else { return frames }
        return ceilingDivision(frames, packetFrames) * packetFrames
    }

    private static func ceilingDivision(_ value: Int, _ divisor: Int) -> Int {
        (value + divisor - 1) / divisor
    }
}

/// What the receive loop must do after one packet. Android returns these as bit flags in an `Int`.
public struct CushionStep: OptionSet, Sendable {
    public let rawValue: Int

    public init(rawValue: Int) { self.rawValue = rawValue }

    /// The cushion changed: ask the player for it.
    public static let apply = CushionStep(rawValue: 1 << 0)
    /// A second of audio was played: read the player's underrun count and call
    /// ``CushionController/adjust(underrunsTotal:)``.
    public static let adjust = CushionStep(rawValue: 1 << 1)
}

/// The cushion one receive loop keeps, in packets: adapted on Auto, held at the desk's length
/// otherwise. Ported from the Android client's `CushionController.kt`.
///
/// - Auto: once per second of audio played the controller takes the player's underrun count and
///   moves the cushion as ``CushionPolicy/next(cushionPackets:cleanIntervals:underrunsInInterval:)``
///   decides, within 1 to 4 packets.
/// - Fixed: the cushion is the desk's length in whole packets, and nothing moves it. Underruns are
///   still noted, so the ones a fixed buffer had never count against Auto afterwards.
/// - Back to Auto: adaptation resumes from the cushion played, brought within the adaptive bounds,
///   with its clean intervals counted anew.
///
/// Only the receive loop uses it: it holds plain values, never allocates and never locks.
public final class CushionController {
    /// The cushion asked of the player, in packets.
    public private(set) var cushionPackets: Int
    /// The desk's buffer in force.
    public private(set) var setting: PlaybackBufferSetting = .auto

    private var cleanIntervals = 0
    private var lastUnderruns = 0
    private var framesSinceAdjust = 0

    public init(startPackets: Int = CushionPolicy.startPackets) {
        cushionPackets = startPackets
    }

    /// The receive loop's step for one packet of `packetFrames` frames: follows the desk's
    /// `setting` (read once per packet) at the player's `playbackRate`, or the packet's
    /// `packetRate` while no player set one, in a player of `capacityFrames`, and counts the
    /// frames played.
    ///
    /// Android takes the packet's samples as well and divides them by a fixed two channels to count
    /// time. The frame count from the datagram header already carries the real channel count, so
    /// one number does both jobs here and counting samples as frames cannot be written at all.
    public func onPacket(
        setting: PlaybackBufferSetting, packetFrames: Int, playbackRate: Int?, packetRate: Int,
        capacityFrames: Int
    ) -> CushionStep {
        let rate = playbackRate ?? packetRate
        var step: CushionStep = []
        if follow(setting, packetFrames: packetFrames, sampleRate: rate, capacityFrames: capacityFrames) {
            step.insert(.apply)
        }
        if onPlayed(frames: packetFrames, sampleRate: rate) {
            step.insert(.adjust)
        }
        return step
    }

    /// Follows the desk's `setting` with packets of `packetFrames` at `sampleRate` in a player of
    /// `capacityFrames`. Answers whether the cushion changed; the same setting again changes nothing.
    public func follow(
        _ setting: PlaybackBufferSetting, packetFrames: Int, sampleRate: Int, capacityFrames: Int
    ) -> Bool {
        guard setting != self.setting else { return false }
        let before = cushionPackets
        self.setting = setting
        cleanIntervals = 0
        switch setting {
        case .auto:
            cushionPackets = min(
                max(before, CushionPolicy.minimumPackets), CushionPolicy.maximumPackets)
        case .fixed(let milliseconds):
            cushionPackets = CushionPolicy.fixedCushionPackets(
                milliseconds: Int(milliseconds), packetFrames: packetFrames,
                sampleRate: sampleRate, capacityFrames: capacityFrames)
        }
        return cushionPackets != before
    }

    /// Counts `frames` played at `sampleRate`; answers true once a whole adjustment interval has
    /// been played since the last one. What runs past the interval counts towards the next, so the
    /// adjustments keep to the second.
    public func onPlayed(frames: Int, sampleRate: Int) -> Bool {
        framesSinceAdjust += max(frames, 0)
        let interval = CushionPolicy.adjustIntervalFrames(sampleRate: sampleRate)
        guard framesSinceAdjust >= interval else { return false }
        framesSinceAdjust -= interval
        return true
    }

    /// One adjustment, with the player's *total* underrun count. On Auto the cushion moves as the
    /// policy decides; while fixed it never does, but the count is still taken so a fixed buffer's
    /// underruns never land on Auto later. Answers whether the cushion changed.
    public func adjust(underrunsTotal: Int) -> Bool {
        let underruns = underrunsTotal - lastUnderruns
        lastUnderruns = underrunsTotal
        guard setting == .auto else { return false }
        let decision = CushionPolicy.next(
            cushionPackets: cushionPackets, cleanIntervals: cleanIntervals,
            underrunsInInterval: underruns)
        cleanIntervals = decision.cleanIntervals
        let changed = decision.cushionPackets != cushionPackets
        cushionPackets = decision.cushionPackets
        return changed
    }
}
