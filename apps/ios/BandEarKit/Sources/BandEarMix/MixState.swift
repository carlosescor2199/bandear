import BandEarProtocol

/// What the engineer set for this musician: a limit, locked channels, and whose mix this one
/// follows. The desk alone sets it; the phone never sends or saves it.
public struct EngineerControls: Equatable, Sendable {
    /// Caps what the musician hears, 0 to 100; 100 when the engineer set no limit.
    public var limitPercent: Int
    public var lockedChannels: Set<Int>
    /// The musician whose mix this one mirrors, or nil.
    public var following: String?

    public init(limitPercent: Int = 100, lockedChannels: Set<Int> = [], following: String? = nil) {
        self.limitPercent = limitPercent
        self.lockedChannels = lockedChannels
        self.following = following
    }

    public func isLocked(_ channel: Int) -> Bool {
        lockedChannels.contains(channel)
    }
}

/// A group as a strip's chip names it; `index` picks the chip's tone.
public struct GroupTag: Equatable, Sendable {
    public let index: Int
    public let name: String

    public init(index: Int, name: String) {
        self.index = index
        self.name = name
    }
}

/// One channel strip as the musician sees it: a single channel, or a stereo pair read from its
/// first channel with its pan shown as a balance.
public struct ChannelStrip: Equatable, Sendable {
    /// The channel the strip is known by: a pair's first channel.
    public let index: Int
    /// The engineer's name for it, or "CH n".
    public let label: String
    /// The first announced group that lists the channel, or nil.
    public let group: GroupTag?
    public let level: Int
    /// 0 hard left, 50 centre, 100 hard right; a pair's balance, read the same way.
    public let pan: Int
    public let isMuted: Bool
    public let isSoloed: Bool
    /// What the channel carries; nil when the desk announced no details, so no icon shows.
    public let kind: ChannelKind?
    /// The pair's second channel, or nil for a single channel.
    public let pairedWith: Int?
    /// The engineer locked the strip (a pair when either side is): it cannot be changed here.
    public let isLocked: Bool
    /// The musician this mix follows, or nil: while set, the strip is the leader's.
    public let following: String?

    public var isStereo: Bool { pairedWith != nil }
}

/// One group fader. `channels` are the desk's 0-based channels in the group; `members` names its
/// strips as they are shown, a hidden channel left out and a stereo pair named once.
public struct GroupFader: Equatable, Sendable {
    public let index: Int
    public let name: String
    public let channels: [Int]
    public let level: Int
    public let members: [String]
    /// The musician this mix follows, or nil: while set, the level is the leader's.
    public let following: String?
}

/// A channel the musician can call theirs, by the channel its strip is known by.
public struct ChannelChoice: Equatable, Sendable {
    public let index: Int
    public let label: String

    public init(index: Int, label: String) {
        self.index = index
        self.label = label
    }
}

/// The musician's mix: the per-channel lists and the master values, plus what the desk announced
/// about its channels and what the engineer set.
///
/// Ported from the Android client's `MixUiState` (`MonitorUiState.kt:125-256`). The stored values
/// are plain lists, index-aligned with the desk's channels; every strip the musician sees is
/// derived from them and the desk's details, so a pair or a hidden channel never has a stored shape
/// of its own to fall out of step.
public struct MixState: Equatable, Sendable {
    /// The level a new channel or group starts at, and the one a missing level reads as.
    public static let neutralLevel = 100

    /// Source channels in the desk's last `config`; 0 before the first one.
    public var sourceChannels = 0
    public var levels: [Int] = []
    public var pans: [Int] = []
    public var mutes: [Bool] = []
    public var solos: [Bool] = []
    /// The groups the desk announced, in its order; a group's index is the index of its level.
    public var groups: [ChannelGroup] = []
    public var groupLevels: [Int] = []
    public var masterVolume = MixState.neutralLevel
    public var masterMuted = false
    /// The ceiling the phone never plays above, in percent.
    public var levelCap = MixState.neutralLevel
    /// One per source channel, or nil when the desk's `config` carried none (an older desk).
    public var channelDetails: [SourceChannel]?
    public var engineer = EngineerControls()

    public init() {}

    /// The loudest the musician hears: the lower of the phone's cap and the engineer's limit.
    public var effectiveCap: Int { min(levelCap, engineer.limitPercent) }

    /// Whose mix this one follows, or nil. While set, the channels and groups are the leader's and
    /// only the master, the cap and the mute are the phone's.
    public var following: String? { engineer.following }

    /// Which channels each strip controls, left to right.
    public var layout: [StripChannels] {
        ChannelLayout.strips(channelCount: levels.count, details: channelDetails)
    }

    /// One strip per shown channel or stereo pair, left to right.
    public var strips: [ChannelStrip] { layout.map(strip) }

    /// One fader per announced group; a group with no level yet reads neutral. Without details from
    /// the desk its members are its channels by number, as they always were.
    public var groupFaders: [GroupFader] {
        let layout = layout
        return groups.enumerated().map { index, group in
            let members: [String] =
                channelDetails == nil
                ? group.channels.map(ChannelLayout.defaultName(channel:))
                : layout.filter { $0.sides.contains(where: group.channels.contains) }
                    .map { label(of: $0.first) }
            return GroupFader(
                index: index, name: group.name, channels: group.channels,
                level: groupLevels.indices.contains(index) ? groupLevels[index] : Self.neutralLevel,
                members: members, following: following)
        }
    }

    /// The channels the musician can call theirs: one per strip of the *announced* channels. Before
    /// the first `config` there are none, whatever the lists hold.
    public var musicianChoices: [ChannelChoice] {
        ChannelLayout.strips(channelCount: sourceChannels, details: channelDetails)
            .map { ChannelChoice(index: $0.first, label: label(of: $0.first)) }
    }

    /// Every channel the phone must leave as it is: each one the engineer locked, and both sides of
    /// a pair when either side is, so a pair is never split by an edit, a recall or More of me.
    public var lockedSides: Set<Int> {
        engineer.lockedChannels.union(
            layout.filter { $0.sides.contains(where: engineer.isLocked) }.flatMap(\.sides))
    }

    /// How `channel`'s strip is named: the engineer's name, or "CH n".
    public func label(of channel: Int) -> String {
        ChannelLayout.label(channel: channel, detail: detail(of: channel))
    }

    /// The channels an edit of `channel` writes: both sides when it is the first of a pair's strip,
    /// otherwise `channel` alone. Matched by the first channel only, exactly as Android matches it.
    public func stripSides(_ channel: Int) -> [Int] {
        layout.first { $0.first == channel }?.sides ?? [channel]
    }

    /// True when the engineer locked `channel`'s strip: a pair is locked when either side is.
    public func isStripLocked(_ channel: Int) -> Bool {
        stripSides(channel).contains(where: engineer.isLocked)
    }

    private func strip(_ channels: StripChannels) -> ChannelStrip {
        let index = channels.first
        let pan =
            channels.second.map {
                ChannelLayout.balance(of: StereoPans(first: pan(of: index), second: pan(of: $0)))
            } ?? pan(of: index)
        return ChannelStrip(
            index: index,
            label: label(of: index),
            group: groupTag(of: index),
            level: levels[index],
            pan: pan,
            isMuted: value(mutes, index, default: false),
            isSoloed: value(solos, index, default: false),
            kind: channelDetails.map { _ in detail(of: index)?.kind ?? .other },
            pairedWith: channels.second,
            isLocked: channels.sides.contains(where: engineer.isLocked),
            following: following)
    }

    private func pan(of channel: Int) -> Int {
        value(pans, channel, default: ChannelLayout.defaultPan(channel: channel))
    }

    private func detail(of channel: Int) -> SourceChannel? {
        guard let channelDetails, channelDetails.indices.contains(channel) else { return nil }
        return channelDetails[channel]
    }

    /// The first announced group that lists `channel`: the one its strip's chip names.
    private func groupTag(of channel: Int) -> GroupTag? {
        groups.firstIndex { $0.channels.contains(channel) }
            .map { GroupTag(index: $0, name: groups[$0].name) }
    }

    private func value<Value>(_ list: [Value], _ index: Int, default fallback: Value) -> Value {
        list.indices.contains(index) ? list[index] : fallback
    }
}
