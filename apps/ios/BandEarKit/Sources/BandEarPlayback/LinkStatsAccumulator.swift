import BandEarProtocol

/// What the phone experienced over one report interval: the counts the desk classifies the link
/// from. ``packetsReceived`` counts the packets played, so a dropped duplicate or late packet is
/// not received twice, and received plus lost is the stretch of the stream the interval covered.
///
/// A port of the Android client's `LinkStatsReport` (`LinkStats.kt`). It is the phone's own
/// measurement, not the wire message: ``LinkStats`` is what is sent, and it checks and rounds these
/// values on the way out.
///
/// The extras the phone announces with `stats.v2`, in milliseconds, are nil when not measured
/// (Android's `NaN`):
/// - ``jitterMilliseconds``: how much the packets' arrival varied over the interval, the mean
///   distance of each packet's spacing from the smoothed spacing. Not measured when no two packets
///   arrived in the interval.
/// - ``cushionMilliseconds``: the playback cushion the phone aims for at the interval's end, not
///   measured before the first packet opened the player.
/// - ``backlogMilliseconds``: the audio queued for playback at the interval's end, as last read
///   from the player.
///
/// ``recovered`` (announced with `redundancy.recovered`) counts the packets over the interval that
/// were played after filling a gap: each arrived for the first time after a newer one, while the
/// receiver held that newer one back for the desk's delayed copies (``ReorderHold/recovered``),
/// which only a delayed redundant copy or Wi-Fi reordering does. They are in ``packetsReceived``,
/// not in ``packetsLost``. A late packet that could not be played (no hold, or past it) is not
/// counted, nor is an immediate copy, which cannot be told from its original.
///
/// Plain values only, so building one allocates nothing on the receive thread.
public struct LinkStatsReport: Equatable, Sendable {
    public let intervalMilliseconds: Int64
    public let packetsReceived: Int64
    public let packetsLost: Int64
    public let underruns: Int64
    public let receiveTimeouts: Int64
    public let jitterMilliseconds: Double?
    public let cushionMilliseconds: Double?
    public let backlogMilliseconds: Double?
    public let recovered: Int64

    public init(
        intervalMilliseconds: Int64, packetsReceived: Int64, packetsLost: Int64, underruns: Int64,
        receiveTimeouts: Int64, jitterMilliseconds: Double? = nil,
        cushionMilliseconds: Double? = nil, backlogMilliseconds: Double? = nil, recovered: Int64 = 0
    ) {
        self.intervalMilliseconds = intervalMilliseconds
        self.packetsReceived = packetsReceived
        self.packetsLost = packetsLost
        self.underruns = underruns
        self.receiveTimeouts = receiveTimeouts
        self.jitterMilliseconds = jitterMilliseconds
        self.cushionMilliseconds = cushionMilliseconds
        self.backlogMilliseconds = backlogMilliseconds
        self.recovered = recovered
    }
}

/// Collects the receive loop's counts into one ``LinkStatsReport`` about once a second.
///
/// A line-for-line port of the Android client's `LinkStatsAccumulator` (`LinkStats.kt`), so both
/// phones report the same Wi-Fi the same way to the engineer's screen.
///
/// Time is passed in by the caller, in milliseconds of a monotonic clock, so the accumulator is
/// pure and its timing is tested without waiting. The player's underrun count is cumulative, so
/// the accumulator keeps the last value it saw and reports the difference; the playback cushion
/// keeps its own last value and is not affected.
///
/// Reports are held back until the stream starts (the first packet is played) or
/// ``startupGraceMilliseconds`` has passed since the accumulator was created, whichever comes
/// first; from then on one is due every ``intervalMilliseconds``:
/// - The stream starts within the grace: the first interval starts at the first packet, so the
///   first report comes one interval later and describes the stream alone. The silence before it
///   was startup, not the link, and is not reported.
/// - The grace passes with no packet: the first report covers everything since the start (the
///   grace, or longer when the poll is late, clamped like any interval). It carries no packets, no
///   timeouts and no underruns, which is the finding: the phone heard nothing for that long.
/// - A stream that starts after the grace joins the interval already running.
///
/// Single-threaded: it lives on the receive thread, and between reports it only bumps counters.
public struct LinkStatsAccumulator: Sendable {
    /// How often the phone reports its link to the desk: once a second while the receiver runs.
    public static let intervalMilliseconds: Int64 = 1_000

    /// The longest interval one report may cover; the desk refuses a report that claims more.
    public static let maximumIntervalMilliseconds = Int64(LinkStats.maximumIntervalMilliseconds)

    /// How long the phone waits for its first packet before it reports a link that hears nothing.
    ///
    /// The receiver starts before the desk knows where to send audio, and the desk judges a report
    /// with no packets as bad, so a report sent before the audio arrives would show a healthy phone
    /// as red. Three seconds covers connecting, registering and the first packets, and matches how
    /// long the desk keeps a report fresh.
    public static let startupGraceMilliseconds: Int64 = 3_000

    private let startedAtMilliseconds: Int64
    private var intervalStartedAtMilliseconds: Int64
    private var packetsReceived: Int64 = 0
    private var packetsLost: Int64 = 0
    private var receiveTimeouts: Int64 = 0
    private var lastUnderruns = 0
    private var lastRecovered: Int64 = 0
    private var streamStarted = false
    private var arrivalDeviationSumMilliseconds = 0.0
    private var arrivalDeviations = 0

    /// An accumulator for a receiver started at `startedAtMilliseconds`, the clock every later call
    /// reads from.
    public init(startedAtMilliseconds: Int64) {
        self.startedAtMilliseconds = startedAtMilliseconds
        intervalStartedAtMilliseconds = startedAtMilliseconds
    }

    /// One played packet, arriving at `nowMilliseconds`, with the `lostBefore` packets the stream
    /// skipped right before it.
    public mutating func onPacket(lostBefore: Int64, nowMilliseconds: Int64) {
        if !streamStarted && !gracePassed(nowMilliseconds: nowMilliseconds) {
            // Reports start with the stream: the first interval runs from this packet.
            intervalStartedAtMilliseconds = nowMilliseconds
        }
        streamStarted = true
        packetsReceived += 1
        packetsLost += lostBefore
    }

    /// One receive timeout: a wait for the next packet that ended with nothing.
    ///
    /// Counted only once the stream has started (a packet was played). The receiver starts before
    /// the desk knows where to send audio, so a wait before the first packet says nothing about the
    /// link, and counting it would show a phone that connects cleanly as red for its first second.
    /// Once started, the stream stays started: a wait while a restarted desk comes back is a real
    /// gap in the audio and is counted.
    public mutating func onReceiveTimeout() {
        if streamStarted { receiveTimeouts += 1 }
    }

    /// How far one packet's spacing from the one before it was from the smoothed spacing, in
    /// milliseconds, early or late alike: the interval's jitter is the mean of these.
    public mutating func onArrivalDeviation(milliseconds deviation: Double) {
        arrivalDeviationSumMilliseconds += abs(deviation)
        arrivalDeviations += 1
    }

    /// True once a report is due at `nowMilliseconds`, so the caller reads the underrun count only
    /// then.
    public func isDue(nowMilliseconds: Int64) -> Bool {
        (streamStarted || gracePassed(nowMilliseconds: nowMilliseconds))
            && nowMilliseconds - intervalStartedAtMilliseconds >= Self.intervalMilliseconds
    }

    private func gracePassed(nowMilliseconds: Int64) -> Bool {
        nowMilliseconds - startedAtMilliseconds >= Self.startupGraceMilliseconds
    }

    /// The report for the interval ending at `nowMilliseconds`, or nil when one is not due yet.
    ///
    /// The interval is the real time since the last report, clamped to what the desk accepts, so a
    /// late poll (the loop waits up to one receive timeout) is reported as the longer interval it
    /// was and the desk's rates stay honest. The interval counts start again from zero.
    ///
    /// `cushionMilliseconds` and `backlogMilliseconds` are the playback values at
    /// `nowMilliseconds`, nil when not measured, and are carried as given; the jitter is the
    /// interval's own (see ``LinkStatsReport``). `cumulativeRecovered` is the hold's count
    /// (``ReorderHold/recovered``); the report carries its difference since the last one.
    ///
    /// Android's jitter is `NaN` when a deviation was not a number, and infinite when one was
    /// infinite; its message leaves either out (`ControlChannel.kt:586-591`). Here such an
    /// interval's jitter is nil, so the report never holds a `NaN`.
    public mutating func poll(
        nowMilliseconds: Int64, cumulativeUnderruns: Int, cushionMilliseconds: Double? = nil,
        backlogMilliseconds: Double? = nil, cumulativeRecovered: Int64 = 0
    ) -> LinkStatsReport? {
        guard isDue(nowMilliseconds: nowMilliseconds) else { return nil }
        // A count that went down belongs to a new track, which starts from zero.
        let underruns = max(cumulativeUnderruns - lastUnderruns, 0)
        lastUnderruns = cumulativeUnderruns
        // Likewise a recovered count that went down belongs to a new hold.
        let recovered =
            cumulativeRecovered >= lastRecovered
            ? cumulativeRecovered - lastRecovered : cumulativeRecovered
        lastRecovered = cumulativeRecovered
        let elapsed = nowMilliseconds - intervalStartedAtMilliseconds
        let report = LinkStatsReport(
            intervalMilliseconds: min(max(elapsed, 1), Self.maximumIntervalMilliseconds),
            packetsReceived: packetsReceived,
            packetsLost: packetsLost,
            underruns: Int64(underruns),
            receiveTimeouts: receiveTimeouts,
            jitterMilliseconds: meanArrivalDeviation(),
            cushionMilliseconds: cushionMilliseconds,
            backlogMilliseconds: backlogMilliseconds,
            recovered: recovered)
        intervalStartedAtMilliseconds = nowMilliseconds
        packetsReceived = 0
        packetsLost = 0
        receiveTimeouts = 0
        arrivalDeviationSumMilliseconds = 0
        arrivalDeviations = 0
        return report
    }

    /// The interval's jitter: nil when no deviation came, or when one was not a finite number.
    private func meanArrivalDeviation() -> Double? {
        guard arrivalDeviations > 0 else { return nil }
        let mean = arrivalDeviationSumMilliseconds / Double(arrivalDeviations)
        return mean.isFinite ? mean : nil
    }
}
