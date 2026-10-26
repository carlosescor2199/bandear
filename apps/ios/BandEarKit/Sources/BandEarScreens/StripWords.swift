import BandEarMix

/// The colour role of a strip's level fill; the app turns it into a colour.
///
/// Ported from the Android client's `StripFill` (`MixScreenText.kt:15-16`).
public enum StripFill: Equatable, Sendable {
    /// The musician's own strip while More of me lifts it: amber.
    case accent
    /// Any other strip.
    case idle
    /// A muted strip, greyed, whoever's it is.
    case muted
}

/// What a channel strip shows where its group goes.
///
/// A held strip says why in words where its group would be, so it is not told by its fade alone;
/// the channel sheet still names the group. Ported from Android's `ChannelStrip`
/// (`MixStrips.kt:153-165`).
public enum StripTag: Equatable, Sendable {
    /// The engineer locked the strip: "Locked".
    case locked
    /// The mix follows another musician's: "Following".
    case following
    /// The first announced group that lists the channel, shown as its chip.
    case group(GroupTag)
    /// No group lists the channel: "No group".
    case noGroup

    /// The words the strip shows: the group's name on its chip, otherwise the tag's words.
    public var text: String {
        switch self {
        case .locked: StripWords.lockedTag
        case .following: StripWords.followingTag
        case .group(let group): group.name
        case .noGroup: StripWords.noGroup
        }
    }
}

/// The tone of a group's chip; the app turns it into a colour. The groups take the tones in turn,
/// in the order the desk announced them.
///
/// Ported from the Android client's `GroupTone` (`Color.kt:15`) and `groupToneFor`
/// (`GroupChip.kt:24-27`).
public enum GroupTone: CaseIterable, Equatable, Sendable {
    case teal
    case violet
    case orange
    case blue

    /// The tone of the group at `index`. A negative index still gets one, counted back from the
    /// last tone, as Kotlin's `Math.floorMod` counts it.
    public static func forGroup(_ index: Int) -> GroupTone {
        let tones = allCases
        let count = tones.count
        return tones[((index % count) + count) % count]
    }
}

/// The words and roles of the Mix screen's strips: a channel strip's level, fill, tag and controls,
/// and a group strip's.
///
/// Ported from the Android client's `MixScreenText.kt`, with the words of the strips' own
/// components (`MixStrips.kt`, `MuteSoloButtons.kt`). The strip forms read an iOS ``ChannelStrip``
/// and leave the mix's rules to it: a pair is locked when either side is, and is known by its first
/// channel.
public enum StripWords {
    /// The level under a strip's fader: "72%", or "muted" in words, not only by its grey fill
    /// (`stripValueText`, `MixScreenText.kt:21-22`).
    public static func valueText(level: Int, muted: Bool) -> String {
        muted ? "muted" : "\(level)%"
    }

    /// A muted strip is greyed; the musician's own strip is amber while More of me lifts it; any
    /// other strip is the idle grey (`stripFillFor`, `MixScreenText.kt:24-32`).
    public static func fill(muted: Bool, mine: Bool, moreOfMeOn: Bool) -> StripFill {
        if muted { return .muted }
        if mine && moreOfMeOn { return .accent }
        return .idle
    }

    /// True when `strip` is the musician's own: the one their channel names, as the strip is known
    /// by its first channel (`MixStrips.kt:104`).
    public static func isMine(_ strip: ChannelStrip, moreOfMe: MoreOfMeState) -> Bool {
        strip.index == moreOfMe.musicianChannel
    }

    /// ``fill(muted:mine:moreOfMeOn:)`` for `strip`, More of me on or off as `moreOfMe` says.
    public static func fill(for strip: ChannelStrip, moreOfMe: MoreOfMeState) -> StripFill {
        fill(
            muted: strip.isMuted, mine: isMine(strip, moreOfMe: moreOfMe),
            moreOfMeOn: moreOfMe.isEnabled)
    }

    /// What a screen reader says for a strip's fader: the channel, whether it is the musician's,
    /// whether it is muted, whether the engineer locked it, and whose mix it follows
    /// (`stripFaderDescription`, `MixScreenText.kt:34-49`).
    public static func faderDescription(
        label: String, mine: Bool, muted: Bool, locked: Bool = false, following: String? = nil
    ) -> String {
        "\(label) level" + (mine ? ", your channel" : "") + (muted ? ", muted" : "")
            + heldWords(locked: locked, following: following)
    }

    /// ``faderDescription(label:mine:muted:locked:following:)`` for `strip` (`MixStrips.kt:172`).
    public static func faderDescription(for strip: ChannelStrip, moreOfMe: MoreOfMeState) -> String {
        faderDescription(
            label: strip.label, mine: isMine(strip, moreOfMe: moreOfMe), muted: strip.isMuted,
            locked: strip.isLocked, following: strip.following)
    }

    /// The words a locked strip shows in place of its group (`LOCKED_TAG`, `MixScreenText.kt:51-52`).
    public static let lockedTag = "Locked"

    /// The words a follower's strips show in place of their group (`FOLLOWING_TAG`,
    /// `MixScreenText.kt:66-67`).
    public static let followingTag = "Following"

    /// What a strip, and the channel sheet, show when no group lists the channel
    /// (`MixStrips.kt:160`, `ChannelSheet.kt:214`).
    public static let noGroup = "No group"

    /// A control's channel `label` for a screen reader, saying the engineer locked it when `locked`
    /// and whose mix it follows while `following` is set (`lockedLabel`, `MixScreenText.kt:54-58`).
    public static func lockedLabel(_ label: String, locked: Bool, following: String? = nil) -> String {
        label + heldWords(locked: locked, following: following)
    }

    /// What `strip` shows where its group goes: a lock outlasts a mirror, so a locked follower says
    /// "Locked"; then "Following"; then its group, or "No group" (`MixStrips.kt:153-165`).
    public static func tag(for strip: ChannelStrip) -> StripTag {
        if strip.isLocked { return .locked }
        if strip.following != nil { return .following }
        return strip.group.map(StripTag.group) ?? .noGroup
    }

    /// What a screen reader says for a group's fader, and whose mix it follows while `following` is
    /// set (`groupFaderDescription`, `MixScreenText.kt:69-70`).
    public static func groupFaderDescription(name: String, following: String?) -> String {
        "\(name) group level" + heldWords(locked: false, following: following)
    }

    /// ``groupFaderDescription(name:following:)`` for `group`.
    public static func groupFaderDescription(for group: GroupFader) -> String {
        groupFaderDescription(name: group.name, following: group.following)
    }

    /// The channels of a group as their strips name them (``GroupFader/members``), or "No channels"
    /// (`groupMembersText`, `MixScreenText.kt:91-93`).
    public static func groupMembersText(_ members: [String]) -> String {
        members.isEmpty ? "No channels" : members.joined(separator: " · ")
    }

    /// What a screen reader is told a strip's label does: it opens the channel's sheet
    /// (`MixStrips.kt:255`).
    public static let openSettingsHint = "Open the channel's settings"

    /// The letter on a strip's Mute key (`MuteSoloButtons.kt:56`).
    public static let muteLetter = "M"

    /// The letter on a strip's Solo key (`MuteSoloButtons.kt:64`).
    public static let soloLetter = "S"

    /// What a screen reader calls `strip`'s Mute key, on the strip and in its sheet: "Mute CH 3",
    /// held as the strip is held (`MuteSoloButtons.kt:57`, `MixStrips.kt:184`, `ChannelSheet.kt:166`).
    public static func muteDescription(for strip: ChannelStrip) -> String {
        "Mute " + lockedLabel(strip.label, locked: strip.isLocked, following: strip.following)
    }

    /// What a screen reader calls `strip`'s Solo key, on the strip and in its sheet: "Solo CH 3"
    /// (`MuteSoloButtons.kt:65`, `MixStrips.kt:184`, `ChannelSheet.kt:175`).
    public static func soloDescription(for strip: ChannelStrip) -> String {
        "Solo " + lockedLabel(strip.label, locked: strip.isLocked, following: strip.following)
    }

    /// ", locked by the engineer" and ", following Ana's mix", as they apply, for a screen reader
    /// (`heldWords`, `MixScreenText.kt:72-76`).
    static func heldWords(locked: Bool, following: String?) -> String {
        var words = locked ? ", locked by the engineer" : ""
        if let following {
            let title = MixWords.followingTitle(following)
            words += ", " + title.prefix(1).lowercased() + title.dropFirst()
        }
        return words
    }
}
