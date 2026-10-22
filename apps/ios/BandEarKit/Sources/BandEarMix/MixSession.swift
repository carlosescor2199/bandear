import BandEarProtocol

/// Something that changes the mix: the musician's edits, and what the desk announces.
public enum MixEvent: Equatable, Sendable {
    /// The desk's `config`: its channels, groups and channel details.
    case configReceived(DeskConfiguration)
    case channelLevelChanged(channel: Int, level: Int)
    case channelPanChanged(channel: Int, pan: Int)
    case channelMuteChanged(channel: Int, muted: Bool)
    case channelSoloChanged(channel: Int, soloed: Bool)
    case groupLevelChanged(group: Int, level: Int)
    case masterVolumeChanged(level: Int)
    case masterMuteChanged(muted: Bool)
    case levelCapChanged(level: Int)
}

/// The phone's side of the mix for one session, and the pure reducer over it.
///
/// Ported from the mix branches of the Android client's `MonitorReducer.kt`, which reduces the whole
/// app's state; only the mix lives here. Every change is a new value from the old one and an event,
/// so each rule is tested by the events that reach it, exactly as Android's suite does.
public struct MixSession: Equatable, Sendable {
    public var mix = MixState()

    public init() {}

    /// The session after `event`.
    public func reduced(by event: MixEvent) -> MixSession {
        var next = self
        switch event {
        case .configReceived(let config):
            next.applyConfig(config)
        case .channelLevelChanged(let channel, let level):
            next.editStrip(channel) { mix, sides in
                mix.levels.replace(at: sides, with: Self.percent(level))
            }
        case .channelPanChanged(let channel, let pan):
            next.editStrip(channel) { mix, sides in
                mix.pans.pan(sides, Self.percent(pan))
            }
        case .channelMuteChanged(let channel, let muted):
            next.editStrip(channel) { mix, sides in mix.mutes.replace(at: sides, with: muted) }
        case .channelSoloChanged(let channel, let soloed):
            next.editStrip(channel) { mix, sides in mix.solos.replace(at: sides, with: soloed) }
        case .groupLevelChanged(let group, let level):
            next.setGroupLevel(group, Self.percent(level))
        // The master, the mute and the cap stay the musician's whatever the engineer set: a lock
        // or a mix being followed never reaches them.
        case .masterVolumeChanged(let level):
            next.mix.masterVolume = Self.percent(level)
        case .masterMuteChanged(let muted):
            next.mix.masterMuted = muted
        case .levelCapChanged(let level):
            next.mix.levelCap = Self.percent(level)
        }
        return next
    }

    private static func percent(_ value: Int) -> Int {
        min(max(value, 0), 100)
    }

    /// A `config`: resizes the per-channel lists when the channel count changed, takes the announced
    /// groups and details (which rebuild the strips), and keeps group levels by position.
    ///
    /// A channel that still exists keeps what the musician set, a new one starts neutral with the
    /// default pan layout, and a repeated count changes nothing. Nothing is sent: a pair the engineer
    /// just linked was panned apart on the desk, and the `mix_state` that follows says so.
    ///
    /// A `config` for no channels, or for more than the mix holds, is ignored, as Android's parser
    /// ignores it before its reducer ever sees it (`ControlChannel.kt:160`).
    private mutating func applyConfig(_ config: DeskConfiguration) {
        let count = Int(config.sourceChannels)
        guard (1...DeskConfiguration.maximumMixChannels).contains(count) else { return }

        if !(mix.sourceChannels == count && mix.levels.count == count) {
            mix.sourceChannels = count
            mix.levels = mix.levels.resized(to: count) { _ in MixState.neutralLevel }
            mix.pans = mix.pans.resized(to: count, filling: ChannelLayout.defaultPan(channel:))
            mix.mutes = mix.mutes.resized(to: count) { _ in false }
            mix.solos = mix.solos.resized(to: count) { _ in false }
        }
        mix.groupLevels = mix.groupLevels.resized(to: config.groups.count) { _ in MixState.neutralLevel }
        mix.groups = config.groups
        mix.channelDetails = config.channels
    }

    /// Applies `change` with the channels an edit of `channel` writes: both sides of a pair's strip,
    /// otherwise the channel alone. A channel with no entry changes nothing, and neither does a strip
    /// the engineer locked (a pair when either side is) or any strip while the mix follows another's:
    /// the desk would keep those values anyway, so the phone's mix never carries one it keeps.
    private mutating func editStrip(_ channel: Int, _ change: (inout MixState, [Int]) -> Void) {
        guard mix.levels.indices.contains(channel), mix.following == nil,
            !mix.isStripLocked(channel)
        else { return }
        change(&mix, mix.stripSides(channel))
    }

    /// Sets one group's level, rebuilding the list to one entry per announced group with the others
    /// as they read on screen. While the mix follows another's, the desk keeps every group level from
    /// the leader's, so nothing changes.
    private mutating func setGroupLevel(_ group: Int, _ level: Int) {
        guard mix.groups.indices.contains(group), mix.following == nil else { return }
        mix.groupLevels = mix.groupLevels.resized(to: mix.groups.count) { _ in MixState.neutralLevel }
        mix.groupLevels[group] = level
    }
}

extension Array {
    /// The first `count` elements, then `filling(index)` for each index past the end.
    fileprivate func resized(to count: Int, filling: (Int) -> Element) -> [Element] {
        (0..<count).map { indices.contains($0) ? self[$0] : filling($0) }
    }

    /// Writes `value` at each of `indices` this array has.
    ///
    /// Android writes by index unguarded and would throw on a list shorter than the levels. The
    /// lists are always resized together, so only a hand-built state gets here, but a reducer
    /// behind a screen must not be able to crash the app over it.
    fileprivate mutating func replace(at indices: [Int], with value: Element) {
        for index in indices where self.indices.contains(index) {
            self[index] = value
        }
    }
}

extension Array where Element == Int {
    /// The pans with `pan` written for `sides`: a single channel takes it as it is, a pair takes it
    /// as its balance, written as both pans.
    fileprivate mutating func pan(_ sides: [Int], _ pan: Int) {
        guard sides.count == 2 else {
            replace(at: sides, with: pan)
            return
        }
        let pans = ChannelLayout.pans(balance: pan)
        replace(at: [sides[0]], with: pans.first)
        replace(at: [sides[1]], with: pans.second)
    }
}
