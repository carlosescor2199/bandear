import BandEarProtocol

// MARK: - The mix the phone sends

extension MixSession {
    /// The `mix` the phone sends: the whole mix as it plays it, under the number of its newest change.
    ///
    /// Ported from the Android client's `outgoingMixMessage` and `buildMixMessage`
    /// (`ControlChannel.kt:415-425,470-504`):
    /// - The number is ``MixSync/latest``. Android reads it before the mix, so the mix is never older
    ///   than the number it goes out under; here both come from one value and cannot disagree. Every
    ///   mix is numbered, the first one too, so every answer from the desk can be matched to one.
    /// - The master volume, the cap and the mute are the musician's own: the cap is
    ///   ``MixState/levelCap``, never the engineer's limit, since the desk applies the lower of the two
    ///   itself.
    /// - The per-channel lists and the group levels are the mix as played: with More of me on, the
    ///   musician's strip is lifted and every other channel halved here, and only here, the locked
    ///   sides left as stored. The stored mix never carries the lift.
    /// - Every list goes out whole on every `mix`, as Android sends them. Before the first `config`
    ///   the lists are empty and go out empty: a list sets the desk's channels from the first and an
    ///   empty one sets none, so the desk keeps its own, and its answer gives the phone the mix it
    ///   holds (`ControlChannel.kt:370-380`, `apps/server/src/mix.rs:1003-1027`).
    /// - Nothing the engineer set goes out: ``MixCommand`` has no field for it, and the desk alone
    ///   sets it.
    public func outgoingMix() -> MixCommand {
        let played = mixAsPlayed
        return MixCommand(
            sequence: sync.latest, volumePercent: played.masterVolume,
            maxLevelPercent: played.levelCap, muted: played.masterMuted,
            channelGains: played.levels, pans: played.pans, channelMutes: played.mutes,
            channelSolos: played.solos, groupLevels: played.groupLevels)
    }
}

// MARK: - When the phone sends one

extension MixSession {
    /// True when `event`, which took the session from `before` to `after`, goes to the desk as a
    /// `mix`. Ported from the rules the Android client hands its `reduceChange`
    /// (`MonitorViewModel.kt:318-321,346-348,414-416,419-425`):
    /// - An edit is sent when it changed the mix. One that changes nothing (a value already set, a
    ///   channel that does not exist, a locked strip, any strip while the mix follows another's)
    ///   sends nothing, as a slider that did not move never did.
    /// - A `config` is sent only when it turned More of me off, so the desk stops lifting; not when
    ///   it did so because the musician's channel became part of a pair. The link's own `mix_state`
    ///   follows with the pans it moved, a send now would put the phone's old pans back over them,
    ///   and the musician's own levels go back once it arrives (``ownLevelsToRestore``).
    /// - Toggling More of me is sent when it actually turned on or off.
    /// - Choosing a channel is sent when the choice was taken while More of me was on: the mix then
    ///   lifts another channel, or none.
    /// - A recalled mix is sent unless the mix was following another musician's
    ///   (``MixSession/recallRefusal``), even when it changed nothing (`MonitorViewModel.kt:454`).
    ///
    /// The desk's reports and the start and end of a session are never sent: the desk already plays
    /// what it reports. The one report the phone answers with a mix, the first after a link turned
    /// More of me off, is numbered by the reducer itself (``reduced(by:)``).
    public static func sends(_ event: MixEvent, before: MixSession, after: MixSession) -> Bool {
        switch event {
        case .channelLevelChanged, .channelPanChanged, .channelMuteChanged, .channelSoloChanged,
            .groupLevelChanged, .masterVolumeChanged, .masterMuteChanged, .levelCapChanged:
            return before.mix != after.mix
        case .configReceived:
            return before.moreOfMe.isEnabled && !after.moreOfMe.isEnabled
                && after.moreOfMe.notice != .channelPaired
        case .moreOfMeToggled:
            return before.moreOfMe.isEnabled != after.moreOfMe.isEnabled
        case .musicianChannelChosen(let channel):
            return before.moreOfMe.isEnabled && after.moreOfMe.musicianChannel == channel
        case .mixRecalled:
            return before.recallRefusal == nil
        case .sessionStarted, .sessionEnded, .mixPushed, .mixAnswered:
            return false
        }
    }

    /// The session after `event`, numbered as the phone's newest mix when ``sends(_:before:after:)``
    /// says it goes to the desk: ``reducedSending(_:sends:)`` with the rule above.
    ///
    /// Every event can go through it. The phone sends ``outgoingMix()`` whenever the result's
    /// ``MixSync/latest`` grew, which covers both a change the rule sends and the report the reducer
    /// numbers itself (`MonitorViewModel.kt:346-348,390-394`).
    public func reducedNumberingSends(by event: MixEvent) -> MixSession {
        reducedSending(event) { before, after in Self.sends(event, before: before, after: after) }
    }
}
