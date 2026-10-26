/// Why More of me is not doing what the musician last asked; the toggle's caption says it.
///
/// Ported case for case from the Android client's `MoreOfMeNotice` (`MonitorUiState.kt:335-352`).
/// The words belong to the screen (T8), so only the reasons live here.
public enum MoreOfMeNotice: Equatable, Sendable {
    /// Asked to turn on before a channel was chosen.
    case needsChannel
    /// The musician chose no channel while More of me was on, so it went off.
    case noChannelSelected
    /// The musician's channel left the mix (past the announced channels, or hidden), so it went off.
    case channelOutOfRange
    /// The desk reported a mix changed elsewhere; the phone shows it as played, so nothing is lifted.
    case mixChangedElsewhere
    /// The engineer locked the musician's channel, so the phone cannot lift it.
    case channelLocked
    /// The mix follows another musician's, whose channels the desk keeps, so nothing is lifted.
    case following
    /// The engineer made the musician's channel part of a stereo pair; the lift the desk plays is one
    /// side's.
    case channelPaired
}

/// More of me and the channel it lifts.
///
/// The musician's channel says who the musician is rather than how the mix sounds, so it lives apart
/// from the mix; remembering it across launches is the app's (T6). Ported from the Android client's
/// `MoreOfMeUiState` (`MonitorUiState.kt:354-361`).
public struct MoreOfMeState: Equatable, Sendable {
    public var isEnabled: Bool
    /// The channel the musician calls theirs, by the channel its strip is known by; nil when none is
    /// chosen.
    public var musicianChannel: Int?
    /// Why More of me is not doing what the musician last asked, or nil.
    public var notice: MoreOfMeNotice?

    public init(isEnabled: Bool = false, musicianChannel: Int? = nil, notice: MoreOfMeNotice? = nil) {
        self.isEnabled = isEnabled
        self.musicianChannel = musicianChannel
        self.notice = notice
    }
}

/// The More of me lift: the musician's channel at full level and every other channel at half.
///
/// It applies to the mix the phone plays and sends, never to the stored mix, so turning More of me
/// off gives back the mix exactly as the musician left it, with nothing to save or replay.
public enum MoreOfMe {
    /// The level the musician's channel is lifted to.
    private static let liftedLevel = 100

    /// `levels` with the musician's channels lifted to 100 and every other channel halved, except
    /// the `lockedChannels`, which stay as they are: the desk keeps a locked channel as the engineer
    /// left it, so it is played and sent as stored.
    ///
    /// `musicianChannels` is the musician's strip: one channel, or both sides of a stereo pair,
    /// lifted alike so that the desk, which keeps a pair's sides equal, plays exactly what was sent.
    /// The first one decides: when the levels have no such channel nothing is lifted and the levels
    /// come back as they are. Android returns null there instead, and every caller of it then plays
    /// and sends the stored mix (`ControlChannel.kt:423`, `MonitorReducer.kt:519`), which is what
    /// this returns. A second side or a locked channel the levels do not have is ignored.
    ///
    /// Ported from `MixSnapshot.moreOfMe` and `moreOfMeFor` (`MixSnapshot.kt:44-54`,
    /// `ClientMixState.kt:77-83`).
    public static func liftedLevels(
        _ levels: [Int], musicianChannels: [Int], lockedChannels: Set<Int>
    ) -> [Int] {
        guard let first = musicianChannels.first, levels.indices.contains(first) else { return levels }
        return levels.indices.map { channel in
            if lockedChannels.contains(channel) { return levels[channel] }
            return musicianChannels.contains(channel) ? liftedLevel : halved(levels[channel])
        }
    }

    /// Half of `level`, a half rounded up.
    ///
    /// Android computes `(gain * 0.5).roundToInt()`, and Kotlin rounds a half toward positive
    /// infinity. Half a whole number is whole or a half, so rounding it up is Kotlin's rule exactly,
    /// for any level. Swift's `rounded()` takes a half away from zero instead: it agrees on every
    /// level a mix holds (0 to 100) but not on a negative one, so it is not used here.
    private static func halved(_ level: Int) -> Int {
        Int((Double(level) / 2).rounded(.up))
    }
}

// MARK: - The rules that turn More of me off, and what the musician asks of it

extension MoreOfMeState {
    /// The musician turned More of me on or off. It turns on only with a channel to lift: asking
    /// without one, while the mix follows another's, or while the engineer locked the channel,
    /// leaves it off and says why (`MonitorReducer.kt:365-378`).
    func toggled(on: Bool, in mix: MixState) -> MoreOfMeState {
        guard on else { return switched(on: false, notice: nil) }
        guard let channel = musicianChannel else { return switched(on: false, notice: .needsChannel) }
        if mix.following != nil { return switched(on: false, notice: .following) }
        if mix.isStripLocked(channel) { return switched(on: false, notice: .channelLocked) }
        return switched(on: true, notice: nil)
    }

    /// `channel` taken as the musician's, or none (`MonitorReducer.kt:380-395`). Only a channel the
    /// picker offers can be chosen: one the desk announced, shown, and known by its strip (a pair
    /// by its first channel); any other changes nothing. Choosing none while More of me is on turns
    /// it off, since nothing is left to lift; so does choosing a channel the engineer locked. The
    /// musician can still call a locked channel theirs.
    func choosing(_ channel: Int?, in mix: MixState) -> MoreOfMeState {
        if let channel, !mix.musicianChoices.contains(where: { $0.index == channel }) { return self }
        if channel == nil, isEnabled {
            return MoreOfMeState(isEnabled: false, musicianChannel: nil, notice: .noChannelSelected)
        }
        return MoreOfMeState(isEnabled: isEnabled, musicianChannel: channel, notice: nil)
            .offIfHeld(by: mix)
    }

    /// The musician's channel fitted to the strips of the channels a `config` announced in `mix`
    /// (`MonitorReducer.kt:305-321`). A channel on a pair's second side becomes the pair's first,
    /// the channel its strip is known by, and More of me keeps lifting the pair. A channel with no
    /// strip (past the announced count, or hidden, and so in no mix) is forgotten: More of me, if it
    /// was on, goes off and says why; a channel that was only remembered is dropped quietly.
    ///
    /// Only a `config` calls this, and only once it announced at least one channel, so the
    /// announced strips are never the empty ones Android guards against.
    func fitted(to mix: MixState) -> MoreOfMeState {
        guard let channel = musicianChannel else { return self }
        let strip = ChannelLayout.strips(channelCount: mix.sourceChannels, details: mix.channelDetails)
            .first { $0.sides.contains(channel) }
        guard let strip else {
            return MoreOfMeState(
                isEnabled: false, musicianChannel: nil, notice: isEnabled ? .channelOutOfRange : notice)
        }
        return MoreOfMeState(isEnabled: isEnabled, musicianChannel: strip.first, notice: notice)
    }

    /// Off, saying why, when it is on and a `config` just made the musician's channel part of a
    /// stereo pair, on either side: on the second, the channel has just moved to the first
    /// (`MonitorReducer.kt:257-273`). `previousChannel` was the musician's in the mix `before`.
    ///
    /// The desk plays the lift it was sent, one side's, and the link gives the second side the first
    /// side's level and pans the pair apart, so the pair's lift would differ from what the desk
    /// plays and the `mix_state` the link sends next would not match. Turned on again, More of me
    /// lifts both sides. A pair the musician's strip already was, or one elsewhere, changes nothing.
    func offIfNewlyPaired(before: MixState, previousChannel: Int?, after: MixState) -> MoreOfMeState {
        guard isEnabled, let channel = musicianChannel, let previousChannel else { return self }
        let sides = after.stripSides(channel)
        let newlyPaired = sides.count == 2 && sides != before.stripSides(previousChannel)
        return newlyPaired ? switched(on: false, notice: .channelPaired) : self
    }

    /// Off, saying why, when it is on and the desk would discard its lift (`MonitorReducer.kt:290-303`):
    /// the mix follows another musician's, whose channels the desk keeps, or the engineer locked the
    /// musician's channel, which the desk keeps as the engineer left it.
    func offIfHeld(by mix: MixState) -> MoreOfMeState {
        guard isEnabled, let channel = musicianChannel else { return self }
        if mix.following != nil { return switched(on: false, notice: .following) }
        if mix.isStripLocked(channel) { return switched(on: false, notice: .channelLocked) }
        return self
    }

    /// This state turned on or off with `notice`, the musician's channel kept.
    private func switched(on isEnabled: Bool, notice: MoreOfMeNotice?) -> MoreOfMeState {
        MoreOfMeState(isEnabled: isEnabled, musicianChannel: musicianChannel, notice: notice)
    }
}
