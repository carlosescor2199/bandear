/// Puts received packets in sequence order before they are played, holding them back for
/// ``holdPackets`` packets so a late copy can fill the gap of its lost original.
///
/// A line-for-line port of the Android client's `ReorderHold.kt`, so both clients treat the same
/// Wi-Fi the same way and a bug report from either one means the same thing.
///
/// The desk can send each packet twice, the copy a few blocks after the original (the `redundancy`
/// message names the delay). A copy of a lost packet then arrives after newer ones; played on
/// arrival it would be out of order and dropped. Holding every packet that many packets late lets
/// the copy arrive in time and play in its place, at the cost of the hold in latency, and only
/// while the desk asks for one.
///
/// The rules, packet by packet (``offer(sequence:packet:)``, then ``next()`` until it answers nil):
/// - The first packet starts the stream, wherever it joins.
/// - A packet newer than any before is held; one that fills a gap among those held is held too.
/// - A packet already played or given up on, or a second copy of one held, is dropped
///   (``dropped``): it cannot rewind the stream or count a gap twice.
/// - A packet ``streamRestartPackets`` or more behind the newest starts a new stream (the server
///   restarted): what the old one left held is let go, and the jump is not counted as loss.
/// - The oldest packet held is due once the newest is ``holdPackets`` ahead of it. A missing one is
///   waited for until the newest is *more* than ``holdPackets`` ahead (its copy can no longer come
///   in time), then skipped; the skipped packets count once, as ``lostBefore`` the next one played.
/// - A packet played after filling a gap is ``recovered``.
/// - ``release()`` makes everything held due, for when no packet came for a while.
///
/// With a hold of 0 every packet is due as it arrives, which is exactly how the phone always played.
///
/// A class rather than a struct on purpose: it lives on the receive path, and a struct copied by
/// accident would copy its arrays. Holding it by reference makes "nothing is allocated per packet"
/// a property of the type instead of a promise about every call site. Single-threaded: only the
/// receive loop touches it.
public final class ReorderHold<Packet> {
    /// How far behind the newest packet a sequence must be to mean the server restarted: about
    /// 2.5 s at 100 packets/s. Wi-Fi reorders by a few positions, never by seconds.
    public static var streamRestartPackets: Int64 { 256 }

    /// The longest hold there is room for: the longest delay the desk gives a redundant copy.
    public static var maximumHoldPackets: Int { 4 }

    /// Room for the held packets, the one that makes the newest due, and one arriving meanwhile.
    static var holdSlots: Int { maximumHoldPackets + 2 }

    private static var noneHeld: Int64 { .min }

    private var packets: [Packet?]
    private var sequences: [Int64]
    private var arrivedLate: [Bool]

    /// The packets each one is held back, clamped to 0 through ``maximumHoldPackets``. A shorter
    /// hold makes what is now due playable at once; a longer one holds the next packets.
    public var holdPackets: Int {
        get { storedHoldPackets }
        set { storedHoldPackets = min(max(newValue, 0), Self.maximumHoldPackets) }
    }
    private var storedHoldPackets = 0

    /// Packets the stream skipped since the hold was created, each counted once.
    public private(set) var lost: Int64 = 0
    /// Duplicate or late packets dropped since the hold was created.
    public private(set) var dropped: Int64 = 0
    /// Packets played after filling a gap: each arrived after a newer one and was still in time.
    /// What the phone reports as `recovered`; they count as received, never as lost.
    public private(set) var recovered: Int64 = 0
    /// How many packets the stream skipped right before the packet ``next()`` last answered.
    public private(set) var lostBefore: Int64 = 0
    /// Whether the packet ``next()`` last answered filled a gap.
    public private(set) var lastRecovered = false

    private var started = false
    /// The next sequence to play or give up on.
    private var head: Int64 = 0
    /// The newest sequence offered.
    private var newest: Int64 = 0
    /// Whether anything has played in this stream, for the gap before the next one.
    private var played = false
    private var lastPlayed: Int64 = 0
    /// Everything held is due, until nothing is.
    private var releasing = false
    /// A packet that arrived too far ahead to hold with the others. Held once ``head`` is close
    /// enough, or once they have played, so ``head`` never passes it while it waits.
    private var waiting: Packet?
    private var waitingSequence: Int64 = 0

    public init() {
        packets = Array(repeating: nil, count: Self.holdSlots)
        sequences = Array(repeating: 0, count: Self.holdSlots)
        arrivedLate = Array(repeating: false, count: Self.holdSlots)
    }

    /// Offers the received packet `sequence`; false when it is dropped (a duplicate, too late to
    /// play, or a sequence no real stream can reach). Then ``next()`` answers what is due.
    ///
    /// The header carries an unsigned 64-bit sequence. Android compares it as a signed `Long`, and
    /// Kotlin's arithmetic wraps; Swift's traps, so a corrupt packet with the top bit set would
    /// crash the receive loop here. No stream reaches 2^63 blocks (that is billions of years), so
    /// such a sequence is refused before it can touch the arithmetic, and every real one behaves
    /// exactly as it does on Android.
    public func offer(sequence rawSequence: UInt64, packet: Packet) -> Bool {
        guard rawSequence <= UInt64(Int64.max) else {
            dropped += 1
            return false
        }
        let sequence = Int64(rawSequence)

        if started && newest - sequence >= Self.streamRestartPackets {
            restart()
        }
        if !started {
            started = true
            head = sequence
            newest = sequence
            store(sequence, packet, late: false)
            return true
        }
        if sequence < head || isHeld(sequence) || (waiting != nil && sequence == waitingSequence) {
            dropped += 1
            return false
        }
        if sequence > newest {
            newest = sequence
            if sequence - head >= Int64(Self.holdSlots) {
                // Far ahead: what is held now plays first, then this one is held (see `next()`).
                if waiting != nil {
                    // A second jump before the first was settled: the first is given up on.
                    dropped += 1
                }
                waiting = packet
                waitingSequence = sequence
                return true
            }
            store(sequence, packet, late: false)
        } else if sequence - head < Int64(Self.holdSlots) {
            // Within what is held: it fills a gap, arriving after a newer one.
            store(sequence, packet, late: true)
        } else {
            // Behind a packet that arrived far ahead, and too far from what is held to hold.
            dropped += 1
            return false
        }
        return true
    }

    /// The next packet to play now, in sequence order, or nil when none is due; ``lostBefore`` and
    /// ``lastRecovered`` then describe it. Call it until it answers nil after every offer.
    public func next() -> Packet? {
        while started {
            if let ahead = waiting, waitingSequence - head < Int64(Self.holdSlots) {
                // The packet that arrived far ahead is now close enough to hold with the others, so
                // the stream plays it in its place: nothing skips past it or leaves it to rewind to.
                waiting = nil
                store(waitingSequence, ahead, late: false)
            }

            let slot = slotOf(head)
            if let packet = packets[slot], sequences[slot] == head {
                if !releasing && newest - head < Int64(holdPackets) { break }
                packets[slot] = nil
                // Never negative: head only moves forward past the last packet played.
                lostBefore = played ? max(head - lastPlayed - 1, 0) : 0
                lost += lostBefore
                lastRecovered = arrivedLate[slot]
                if lastRecovered { recovered += 1 }
                played = true
                lastPlayed = head
                head += 1
                return packet
            }

            let held = oldestHeld()
            if held == Self.noneHeld {
                // Nothing held: a packet that arrived far ahead is held now, the gap before it
                // given up on (counted when it is played).
                guard let far = waiting else { break }
                waiting = nil
                head = waitingSequence
                store(waitingSequence, far, late: false)
                continue
            }

            // Missing: given up on once its copy can no longer come in time, as is every sequence
            // up to the next one held.
            if !releasing && newest - head <= Int64(holdPackets) { break }
            head = releasing ? held : min(held, newest - Int64(holdPackets))
        }
        releasing = false
        return nil
    }

    /// Makes everything held due: no packet came for a while, so nothing held is worth waiting
    /// for. ``next()`` then plays it in order.
    public func release() {
        if started { releasing = true }
    }

    private func restart() {
        for slot in packets.indices { packets[slot] = nil }
        waiting = nil
        started = false
        played = false
        releasing = false
    }

    /// Sequences here are never negative (see ``offer(sequence:packet:)``), so `%` is the floor
    /// modulus Android's `Math.floorMod` computes.
    private func slotOf(_ sequence: Int64) -> Int {
        Int(sequence % Int64(Self.holdSlots))
    }

    private func isHeld(_ sequence: Int64) -> Bool {
        let slot = slotOf(sequence)
        return packets[slot] != nil && sequences[slot] == sequence
    }

    private func store(_ sequence: Int64, _ packet: Packet, late: Bool) {
        let slot = slotOf(sequence)
        packets[slot] = packet
        sequences[slot] = sequence
        arrivedLate[slot] = late
    }

    /// The oldest sequence held from ``head`` on, or `noneHeld`.
    private func oldestHeld() -> Int64 {
        var oldest = Int64.max
        for slot in packets.indices
        where packets[slot] != nil && sequences[slot] >= head && sequences[slot] < oldest {
            oldest = sequences[slot]
        }
        return oldest == .max ? Self.noneHeld : oldest
    }
}
