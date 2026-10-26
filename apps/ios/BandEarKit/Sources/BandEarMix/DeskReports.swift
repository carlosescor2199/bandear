import BandEarProtocol

extension MixReport {
    /// What the engineer set for this musician, as the report carries it: the limit, every channel
    /// whose `locked` entry is true, and whose mix this one follows.
    ///
    /// The decoder already read them as leniently as Android's `parseEngineerControls`
    /// (`ControlChannel.kt:623-628`); this only turns the per-channel list into the channels it
    /// locks. A report always carries them, an older desk's as the defaults (no limit, nothing
    /// locked, following nobody), so a report always says what the engineer set.
    public var engineerControls: EngineerControls {
        EngineerControls(
            limitPercent: engineerLimitPercent,
            lockedChannels: Set(locked.indices.filter { locked[$0] }),
            following: following)
    }
}

extension MixEvent {
    /// What a message from the desk reports about the mix, or nil when it reports nothing about it.
    ///
    /// Ported from what Android's control channel hands its listener (`ControlChannel.kt:150-178`):
    /// - a `mix_state` is the desk's whole mix, pushed because it changed elsewhere;
    /// - a numbered `mix_ack` answers the phone's mix of that number with the mix the desk now
    ///   plays, and one whose mix could not be read answers it with nothing to show;
    /// - a numbered `error` refused the phone's mix of that number, so it answers it with nothing;
    /// - an ack or an error without a number cannot be matched to any mix: the ack only
    ///   acknowledges, and the error answered something else (a registration, a stats report).
    ///
    /// Nothing else is a report on the mix. A `config` changes the mix too, as
    /// ``MixEvent/configReceived(_:)``, but it is the desk announcing its channels, not reporting a
    /// mix, so it is not mapped here.
    public init?(reportedBy message: ServerMessage) {
        switch message {
        case .mixState(let report):
            self = .mixPushed(report)
        case .mixAck(let report):
            guard let sequence = report.sequence else { return nil }
            self = .mixAnswered(sequence: sequence, report: report)
        case .unreadableMixAck(let sequence):
            self = .mixAnswered(sequence: sequence, report: nil)
        case .error(let error):
            guard let sequence = error.sequence else { return nil }
            self = .mixAnswered(sequence: sequence, report: nil)
        case .configuration, .playbackBuffer, .redundancyHold, .unrecognized:
            return nil
        }
    }
}
