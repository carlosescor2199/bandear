import BandEarProtocol

extension LinkStats {
    /// The `stats` message for one of the phone's link reports: Android's `buildStatsMessage`
    /// (`ControlChannel.kt:571-591`).
    ///
    /// - `recovered` is always carried, 0 included, as Android always sends it: the phone announces
    ///   `redundancy.recovered`, so a missing count would read as a phone that cannot count.
    /// - The extras go through ``LinkStats``'s own checks: one not measured, or outside what the desk
    ///   reads, is left out, and the rest are rounded to a hundredth.
    /// - The counts are never below zero in a report the ``LinkStatsAccumulator`` makes: each one
    ///   only grows within an interval, and the underruns and recovered packets are differences it
    ///   holds at zero or more. A report built by hand with a negative count sends 0 for it rather
    ///   than stopping the app over a message.
    ///
    /// It never fails. ``LinkStats`` refuses only an interval outside 1 to
    /// ``LinkStats/maximumIntervalMilliseconds``, and the accumulator already clamps its interval to
    /// exactly that range (``LinkStatsAccumulator/poll(nowMilliseconds:cumulativeUnderruns:cushionMilliseconds:backlogMilliseconds:cumulativeRecovered:)``).
    /// The interval is clamped again here, which changes nothing for the accumulator's reports, so
    /// that a report built by hand cannot make it fail either.
    public init(_ report: LinkStatsReport) {
        let interval = UInt32(
            min(max(report.intervalMilliseconds, 1), Int64(LinkStats.maximumIntervalMilliseconds)))
        // Cannot throw: the interval was just held to the only range `LinkStats` checks, and nothing
        // else it is given can make it refuse.
        try! self.init(
            intervalMilliseconds: interval,
            packetsReceived: UInt64(clamping: report.packetsReceived),
            packetsLost: UInt64(clamping: report.packetsLost),
            underruns: UInt64(clamping: report.underruns),
            receiveTimeouts: UInt64(clamping: report.receiveTimeouts),
            jitterMilliseconds: report.jitterMilliseconds,
            cushionMilliseconds: report.cushionMilliseconds,
            backlogMilliseconds: report.backlogMilliseconds,
            recovered: UInt64(clamping: report.recovered))
    }
}
