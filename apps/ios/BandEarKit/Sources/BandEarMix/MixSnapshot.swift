/// A whole mix as the phone saves it: the four per-channel lists, the group levels, and the master
/// volume, cap and mute.
///
/// Ported from the Android client's `MixSnapshot` (`MixSnapshot.kt:16-25`), with its defaults: no
/// lists, the master and the cap at 100, unmuted. It holds the musician's own levels, never the More
/// of me lift, and nothing the desk announced or the engineer set: a saved mix is a balance, and
/// the channels and groups it is played on are the desk's. How it is kept on the phone is the
/// store's (`BandEarSession`).
public struct MixSnapshot: Equatable, Sendable {
    public var channelGains: [Int]
    /// 0 hard left, 50 centre, 100 hard right, one per channel.
    public var channelPans: [Int]
    public var channelMutes: [Bool]
    public var channelSolos: [Bool]
    /// One per group, in the desk's order.
    public var groupLevels: [Int]
    public var volumePercent: Int
    /// The phone's own cap, never the engineer's limit.
    public var maxLevelPercent: Int
    public var muted: Bool

    public init(
        channelGains: [Int] = [], channelPans: [Int] = [], channelMutes: [Bool] = [],
        channelSolos: [Bool] = [], groupLevels: [Int] = [], volumePercent: Int = 100,
        maxLevelPercent: Int = 100, muted: Bool = false
    ) {
        self.channelGains = channelGains
        self.channelPans = channelPans
        self.channelMutes = channelMutes
        self.channelSolos = channelSolos
        self.groupLevels = groupLevels
        self.volumePercent = volumePercent
        self.maxLevelPercent = maxLevelPercent
        self.muted = muted
    }
}

extension MixState {
    /// This mix as the phone saves it: Android's `MixUiState.toSnapshot()`
    /// (`MonitorUiState.kt:246-255`). The stored mix holds the musician's own levels, so a mix saved
    /// while More of me is on is saved without the lift (`MonitorViewModel.kt:427`).
    public var snapshot: MixSnapshot {
        MixSnapshot(
            channelGains: levels, channelPans: pans, channelMutes: mutes, channelSolos: solos,
            groupLevels: groupLevels, volumePercent: masterVolume, maxLevelPercent: levelCap,
            muted: masterMuted)
    }
}
