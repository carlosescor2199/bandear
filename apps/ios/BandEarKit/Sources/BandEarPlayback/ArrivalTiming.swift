/// How evenly the desk's packets arrive: the smoothed spacing between arrivals, the smoothed jitter
/// around it, and each arrival's deviation from that spacing, which the link report averages.
///
/// Ported from the Android client's receive loop (`PmonPlayer.kt:222-233`), which keeps the same
/// three values in local variables. For every packet the reorder hold accepts, measured as it
/// arrives rather than as it plays, so the hold cannot hide the Wi-Fi's own timing:
/// 1. The interval since the previous arrival moves the spacing a tenth of the way towards it. The
///    first interval seeds the spacing instead.
/// 2. The interval's distance from the spacing *just moved* moves the jitter a tenth of the way.
/// 3. That same signed deviation, interval minus the moved spacing, is handed out for the link
///    report.
///
/// The weight is Android's 0.1, not RFC 3550's 1/16, so both clients report the same Wi-Fi alike.
///
/// Time is a monotonic nanosecond reading, as Android's `System.nanoTime()`, so a 5.33 ms packet
/// spacing is measured as 5.33 ms; a millisecond clock would read it as 5, 5, 6 and report its own
/// rounding as jitter. The interval is the signed difference over 1 000 000, Android's `Long`
/// arithmetic, so a reading that went backwards is a negative interval rather than a trap on the
/// receive thread. Nothing here reads a clock. A value on the receive path: plain fields, no
/// allocation.
///
/// Unlike Android, which re-seeds while the spacing is exactly 0.0, the spacing is `nil` until the
/// first interval seeds it, once.
public struct ArrivalTiming: Equatable, Sendable {
    /// The weight each new interval gets in the spacing, and each new deviation in the jitter.
    public static let smoothing = 0.1

    /// The smoothed spacing between arrivals, in milliseconds; `nil` until a second packet arrived.
    public private(set) var interArrivalMilliseconds: Double?
    /// The smoothed distance of each interval from the spacing, in milliseconds; `nil` until a
    /// second packet arrived.
    public private(set) var jitterMilliseconds: Double?

    private var previousArrivalNanoseconds: UInt64?

    public init() {}

    /// Notes a packet accepted at `nowNanoseconds` on a monotonic clock and answers its deviation
    /// from the smoothed spacing, in milliseconds (negative for a packet early on it), or `nil` for
    /// the first packet, which has no interval yet.
    public mutating func onArrival(nowNanoseconds: UInt64) -> Double? {
        defer { previousArrivalNanoseconds = nowNanoseconds }
        guard let previous = previousArrivalNanoseconds else { return nil }
        let interval = Double(Int64(bitPattern: nowNanoseconds &- previous)) / 1_000_000.0
        let spacing = interArrivalMilliseconds.map { $0 + Self.smoothing * (interval - $0) } ?? interval
        let deviation = interval - spacing
        let jitter = jitterMilliseconds ?? 0
        interArrivalMilliseconds = spacing
        jitterMilliseconds = jitter + Self.smoothing * (abs(deviation) - jitter)
        return deviation
    }
}
