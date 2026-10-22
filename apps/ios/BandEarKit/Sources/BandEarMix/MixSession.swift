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
    /// A session with a desk started. What the engineer set belongs to the last one, so it goes, and
    /// so do levels a link left to put back; the mix's numbers stay (see ``MixSync``).
    case sessionStarted
    /// The session ended: a report that arrives after it changes nothing.
    case sessionEnded
    /// The desk pushed the musician's whole mix (`mix_state`), because it changed elsewhere.
    case mixPushed(MixReport)
    /// The desk answered the phone's mix number `sequence`: with the whole mix it now plays, or with
    /// nil when it refused that mix or its answer could not be read.
    case mixAnswered(sequence: UInt64, report: MixReport?)
    /// The musician turned More of me on or off.
    case moreOfMeToggled(on: Bool)
    /// The musician called a channel theirs, by the channel its strip is known by, or none.
    case musicianChannelChosen(Int?)
}

/// The phone's side of the mix for one session, and the pure reducer over it.
///
/// Ported from the mix branches of the Android client's `MonitorReducer.kt`, which reduces the whole
/// app's state; only the mix lives here. Every change is a new value from the old one and an event,
/// so each rule is tested by the events that reach it, exactly as Android's suite does.
public struct MixSession: Equatable, Sendable {
    public var mix = MixState()
    /// How the desk's answers are matched to the mixes this phone sent.
    public var sync = MixSync()
    /// True from ``MixEvent/sessionStarted`` until ``MixEvent/sessionEnded``. A report belongs to
    /// a session, so one that arrives outside a session changes neither the mix nor what the
    /// engineer set.
    public var isSessionRunning = false
    /// More of me and the musician's channel. The lift is never stored: ``mix`` always holds the
    /// musician's own levels, and the lifted ones are derived when the mix is played or sent.
    public var moreOfMe = MoreOfMeState()
    /// The musician's own levels, kept when a link turned More of me off, to put back over the lift
    /// the desk still plays once its next report arrives; nil when nothing waits. Not shown.
    public var ownLevelsToRestore: [Int]?

    public init() {}

    /// The channels More of me lifts: the musician's channel and, when its strip is a stereo pair,
    /// the other side too; empty when no channel is chosen (`MonitorUiState.kt:60-65`).
    public var moreOfMeChannels: [Int] {
        moreOfMe.musicianChannel.map(mix.stripSides) ?? []
    }

    /// ``reduced(by:)`` for a change the musician made. When `sends` says the change goes to the
    /// desk, the same step numbers it as the phone's newest mix (``MixSync/changed()``): the change
    /// and its number land in one value, so a send still queued already counts, and no answer from
    /// the desk can be matched to a mix older than the one on screen.
    ///
    /// A change that is sent also cancels levels still waiting to be put back after a link
    /// (``ownLevelsToRestore``): the mix holds the musician's own unlifted levels, this change
    /// included, so the send already puts them on the desk, and restoring the older ones when its
    /// answer arrives would undo the change.
    ///
    /// Ported from Android's `reduceChange` (`MonitorReducer.kt:190-208`). Which changes are sent is
    /// the outgoing mix's rule (T5e); until then the caller says.
    public func reducedSending(
        _ event: MixEvent, sends: (_ before: MixSession, _ after: MixSession) -> Bool
    ) -> MixSession {
        var next = reduced(by: event)
        if sends(self, next) {
            next.sync = next.sync.changed()
            next.ownLevelsToRestore = nil
        }
        return next
    }

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
        case .sessionStarted:
            // This desk reports its own controls (an older one never does, and leaves all open). A
            // new session sends the phone's own mix when it opens, so no lift waits to be dropped.
            next.isSessionRunning = true
            next.mix.engineer = EngineerControls()
            next.ownLevelsToRestore = nil
        case .sessionEnded:
            next.isSessionRunning = false
        case .mixPushed(let report):
            // A push while a change of the phone's is on its way is overtaken by that change's
            // answer; the engineer's controls it carries hold at once (`MonitorReducer.kt:123-125`).
            next.takeEngineerControls(report.engineerControls)
            if next.sync.isSettled {
                next.applyDeskMix(report)
            }
        case .mixAnswered(let sequence, let report):
            next.answer(sequence, with: report)
        case .moreOfMeToggled(let on):
            next.moreOfMe = moreOfMe.toggled(on: on, in: mix)
        case .musicianChannelChosen(let channel):
            next.moreOfMe = moreOfMe.choosing(channel, in: mix)
        }
        return next
    }

    fileprivate static func percent(_ value: Int) -> Int {
        min(max(value, 0), 100)
    }

    /// The desk answered the phone's change number `sequence`, with the whole mix it now plays or
    /// with nothing (it refused the mix, or its answer could not be read). The desk answers in
    /// order, so every older change is answered too, even outside a session: the numbers only grow.
    ///
    /// Only the answer to the newest change is shown: an older one's mix is overtaken by a change
    /// still on its way, whose own answer follows. The engineer's controls of any readable answer
    /// hold at once; a refusal carries none and leaves them as they were (`MonitorReducer.kt:428-437`).
    private mutating func answer(_ sequence: UInt64, with report: MixReport?) {
        let answersNewest = sequence == sync.latest
        sync = sync.answeredUpTo(sequence)
        guard let report else { return }
        takeEngineerControls(report.engineerControls)
        if answersNewest {
            applyDeskMix(report)
        }
    }

    /// The engineer's `controls` as the desk reported them with a mix. They hold at once, even when
    /// the mix they came with is overtaken by a change of the phone's still on its way: the phone
    /// never sends them, so nothing of its own can overtake them, and the desk reports them in
    /// order. Outside a session they change nothing (`MonitorReducer.kt:275-288`).
    ///
    /// More of me goes off when the musician's channel is now locked or the mix now follows
    /// another's. When following stops, its notice goes; More of me stays off until the musician
    /// turns it on.
    private mutating func takeEngineerControls(_ controls: EngineerControls) {
        guard isSessionRunning, mix.engineer != controls else { return }
        let wasFollowing = mix.following != nil
        mix.engineer = controls
        if wasFollowing, mix.following == nil, moreOfMe.notice == .following {
            moreOfMe.notice = nil
        }
        moreOfMe = moreOfMe.offIfHeld(by: mix)
    }

    /// The desk's whole mix, pushed because it changed elsewhere or answering the phone's newest
    /// change: it becomes the phone's mix, exactly as the desk plays it (`MonitorReducer.kt:439-462`).
    ///
    /// Outside a session it changes nothing. The first report after a link turned More of me off
    /// puts the musician's own levels back (``restoreOwnLevels(_:over:)``). A report of the mix the
    /// phone already plays (an answer to its own mix, or the whole mix the desk re-sends after it
    /// missed events) changes nothing: that is the echo check. Any other report turns More of me off
    /// and says why: its levels are what the desk plays, so lifting them again on the next send would
    /// lift them twice.
    private mutating func applyDeskMix(_ report: MixReport) {
        guard isSessionRunning else { return }
        let reported = mix.takingDeskMix(report)
        if let own = ownLevelsToRestore {
            restoreOwnLevels(own, over: reported)
            return
        }
        guard reported != mixAsPlayed else { return }
        mix = reported
        if moreOfMe.isEnabled {
            moreOfMe.isEnabled = false
            moreOfMe.notice = .mixChangedElsewhere
        }
    }

    /// The first report after a link turned More of me off (``MoreOfMeNotice/channelPaired``)
    /// (`MonitorReducer.kt:464-484`).
    ///
    /// The `config` turned it off, kept the musician's `own` levels and sent nothing; the link's
    /// `mix_state` (or the answer to a change made meanwhile) arrives with the pans the link moved
    /// and the levels the desk plays, still lifted by the mix More of me sent. The phone takes that
    /// report with its own levels put back on every channel but a locked one, which the desk keeps
    /// (a pair whole when either side is), and with a pair's second side at its first side's level,
    /// as the link sets it. When that differs from what the desk plays, it is numbered as the
    /// phone's newest mix, so it is sent once through the usual numbering. While the mix follows
    /// another's the desk keeps every channel, so the report is taken as it is. Either way nothing
    /// waits any more.
    private mutating func restoreOwnLevels(_ own: [Int], over reported: MixState) {
        ownLevelsToRestore = nil
        guard reported.following == nil else {
            mix = reported
            return
        }
        let locked = reported.lockedSides
        var levels = reported.levels.indices.map { channel in
            locked.contains(channel) || !own.indices.contains(channel)
                ? reported.levels[channel] : own[channel]
        }
        for strip in reported.layout {
            if let second = strip.second, !locked.contains(second) {
                levels[second] = levels[strip.first]
            }
        }
        mix = reported
        mix.levels = levels
        if levels != reported.levels {
            sync = sync.changed()
        }
    }

    /// The mix as the phone plays and sends it, which is what the desk reports back when nothing
    /// changed elsewhere: with More of me on, the musician's strip lifted and every other channel
    /// halved, except the channels the engineer locked (a pair whole when either side is), which
    /// stay as stored (`MonitorReducer.kt:511-521`).
    private var mixAsPlayed: MixState {
        guard moreOfMe.isEnabled else { return mix }
        var played = mix
        played.levels = MoreOfMe.liftedLevels(
            mix.levels, musicianChannels: moreOfMeChannels, lockedChannels: mix.lockedSides)
        return played
    }

    /// A `config`: resizes the per-channel lists when the channel count changed, takes the announced
    /// groups and details (which rebuild the strips), keeps group levels by position, and fits the
    /// musician's channel to the new strips (`MonitorReducer.kt:217-255`).
    ///
    /// A channel that still exists keeps what the musician set, a new one starts neutral with the
    /// default pan layout, and a repeated count changes nothing. Nothing is sent: a pair the engineer
    /// just linked was panned apart on the desk, and the `mix_state` that follows says so. When that
    /// link turns More of me off, the stored levels are the musician's own (the lift is never
    /// stored), so they are kept to be put back once that `mix_state` arrives (``applyDeskMix(_:)``).
    ///
    /// A `config` for no channels, or for more than the mix holds, is ignored, as Android's parser
    /// ignores it before its reducer ever sees it (`ControlChannel.kt:160`).
    private mutating func applyConfig(_ config: DeskConfiguration) {
        let count = Int(config.sourceChannels)
        guard (1...DeskConfiguration.maximumMixChannels).contains(count) else { return }
        let before = mix

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

        let fitted = moreOfMe.fitted(to: mix).offIfNewlyPaired(
            before: before, previousChannel: moreOfMe.musicianChannel, after: mix)
        if moreOfMe.isEnabled, !fitted.isEnabled, fitted.notice == .channelPaired {
            ownLevelsToRestore = mix.levels
        }
        moreOfMe = fitted.offIfHeld(by: mix)
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

extension MixState {
    /// This mix with the desk's `report` applied to the channels and groups the desk announced
    /// (`MonitorReducer.kt:492-506`). The report holds the desk's full tables (32 channels, 16
    /// groups) and the rest is not shown. Before the first `config` the strips the phone shows keep
    /// their count. A value the report lacks keeps what the phone has, and levels and pans stay
    /// between 0 and 100. What the engineer set is not taken here: it holds on its own, before the
    /// mix is weighed at all.
    fileprivate func takingDeskMix(_ report: MixReport) -> MixState {
        let announced = sourceChannels > 0
        let channelCount = announced ? sourceChannels : levels.count
        let groupCount = announced ? groups.count : groupLevels.count
        let percent = MixSession.percent

        var next = self
        next.levels = report.channelGains.map(percent).resized(to: channelCount) {
            levels.element(at: $0) ?? Self.neutralLevel
        }
        next.pans = report.pans.map(percent).resized(to: channelCount) {
            pans.element(at: $0) ?? ChannelLayout.defaultPan(channel: $0)
        }
        next.mutes = report.channelMutes.resized(to: channelCount) { mutes.element(at: $0) ?? false }
        next.solos = report.channelSolos.resized(to: channelCount) { solos.element(at: $0) ?? false }
        next.groupLevels = report.groupLevels.map(percent).resized(to: groupCount) {
            groupLevels.element(at: $0) ?? Self.neutralLevel
        }
        next.masterVolume = percent(report.volumePercent)
        next.levelCap = percent(report.maxLevelPercent)
        next.masterMuted = report.muted
        return next
    }
}

extension Array {
    /// The first `count` elements, then `filling(index)` for each index past the end.
    fileprivate func resized(to count: Int, filling: (Int) -> Element) -> [Element] {
        (0..<count).map { indices.contains($0) ? self[$0] : filling($0) }
    }

    /// The element at `index`, or nil when the array has none there.
    fileprivate func element(at index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
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
