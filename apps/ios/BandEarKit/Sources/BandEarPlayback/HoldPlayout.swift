/// The receive loop's use of the ``ReorderHold`` and the ``LinkStatsAccumulator``, wake-up by
/// wake-up, kept apart from the socket and the audio engine so the wiring is tested without either.
///
/// A port of the Android client's `HoldPlayout.kt`. Per wake-up the loop calls, in order:
/// ``onTimeout()`` when no packet came, ``wake(holdPackets:)`` with the hold the desk asks for now,
/// ``offer(sequence:packet:)`` for a packet that came (skipping it when refused), then
/// ``nextToPlay(nowMilliseconds:)`` until it answers nil, playing each packet it answers.
/// ``poll(nowMilliseconds:cumulativeUnderruns:cushionMilliseconds:backlogMilliseconds:)`` ends a
/// report interval with the hold's recovered count, so a packet a late copy brought counts in
/// `packets_received` (it is played) and in `recovered`, never in `packets_lost` (see
/// ``LinkStatsReport/recovered``).
///
/// On Android the player holds the same accumulator, and asks it directly whether a report is due
/// and what each packet's arrival deviation was. Here the accumulator is a value the playout owns,
/// so nothing else can hold it: ``linkStats`` reads it, and ``onArrivalDeviation(milliseconds:)``
/// feeds it.
///
/// Single-threaded, on the receive thread; nothing is allocated per packet.
public final class HoldPlayout<Packet> {
    private let hold: ReorderHold<Packet>

    /// The link report's counts so far, for asking whether a report is due
    /// (``LinkStatsAccumulator/isDue(nowMilliseconds:)``) before reading the player's underruns.
    public private(set) var linkStats: LinkStatsAccumulator

    public init(hold: ReorderHold<Packet>, linkStats: LinkStatsAccumulator) {
        self.hold = hold
        self.linkStats = linkStats
    }

    /// The hold now in force, in packets, read from the desk's setting once per wake-up.
    public var holdPackets: Int { hold.holdPackets }

    /// Every packet the stream skipped so far, each counted once (the receiver's own metrics).
    public var lost: Int64 { hold.lost }

    /// No packet came within the receive timeout: counted, and nothing held is waited for longer.
    public func onTimeout() {
        linkStats.onReceiveTimeout()
        hold.release()
    }

    /// A wake-up, with the hold the desk asks for now (a shorter one plays what is now due).
    public func wake(holdPackets: Int) {
        hold.holdPackets = holdPackets
    }

    /// Offers a packet that arrived; false when it is dropped (a duplicate, or too late to play).
    public func offer(sequence: UInt64, packet: Packet) -> Bool {
        hold.offer(sequence: sequence, packet: packet)
    }

    /// The next packet to play, in sequence order, counted as played at `nowMilliseconds` with the
    /// packets the stream skipped right before it; nil when none is due.
    public func nextToPlay(nowMilliseconds: Int64) -> Packet? {
        guard let packet = hold.next() else { return nil }
        linkStats.onPacket(lostBefore: hold.lostBefore, nowMilliseconds: nowMilliseconds)
        return packet
    }

    /// How far a packet's spacing from the one before it was from the smoothed spacing, as it
    /// arrived, whatever the hold then does with it (see
    /// ``LinkStatsAccumulator/onArrivalDeviation(milliseconds:)``).
    public func onArrivalDeviation(milliseconds deviation: Double) {
        linkStats.onArrivalDeviation(milliseconds: deviation)
    }

    /// The link report due at `nowMilliseconds`, with the hold's recovered count, or nil when none
    /// is due.
    public func poll(
        nowMilliseconds: Int64, cumulativeUnderruns: Int, cushionMilliseconds: Double? = nil,
        backlogMilliseconds: Double? = nil
    ) -> LinkStatsReport? {
        linkStats.poll(
            nowMilliseconds: nowMilliseconds,
            cumulativeUnderruns: cumulativeUnderruns,
            cushionMilliseconds: cushionMilliseconds,
            backlogMilliseconds: backlogMilliseconds,
            cumulativeRecovered: hold.recovered)
    }
}
