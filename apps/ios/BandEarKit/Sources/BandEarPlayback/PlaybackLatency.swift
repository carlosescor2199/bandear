/// What the phone measures about its own playback: the audio queued ahead of the output, the
/// cushion it aims for, and the two a link report ends its interval with.
///
/// Ported from the measures in the Android client's `PlaybackLatency.kt`. One clock only, the
/// output's own render position: the frames written but not presented yet are the backlog, which is
/// the cushion plus the system's output buffer. No desk clock takes part, so there is nothing to
/// synchronise and no clock skew to corrupt the value.
///
/// A measure that was not taken is `nil`, where Android reports `NaN`.
///
/// Not every helper of `PlaybackLatency.kt` is here:
/// - `trackCapacityFrames` is ``CushionPolicy/capacityFrames(sampleRate:packetFrames:)`` and
///   `nextCushion` is ``CushionPolicy/next(cushionPackets:cleanIntervals:underrunsInInterval:)``,
///   ported with the cushion they size and move.
/// - `keepsLowLatency` and `SHRINK_TOLERANCE_MS` are not ported. They decide whether an Android
///   `AudioTrack`, opened with room for the longest buffer, honoured the smaller size set on it,
///   and reopen it when it did not. The iOS player queues its cushion in a buffer of its own, so
///   there is no platform buffer size to second-guess.
public enum PlaybackLatency {
    /// Frames written to the output but not presented yet. Never below zero: a render position
    /// that briefly runs ahead of the write count is a reporting artifact, not negative latency.
    public static func pendingFrames(framesWritten: Int64, framePosition: Int64) -> Int64 {
        max(framesWritten - framePosition, 0)
    }

    /// The ``pendingFrames(framesWritten:framePosition:)`` in milliseconds at `sampleRate`, or 0
    /// when the rate cannot be used.
    public static func backlogMilliseconds(
        framesWritten: Int64, framePosition: Int64, sampleRate: Int
    ) -> Double {
        guard sampleRate > 0 else { return 0 }
        return Double(pendingFrames(framesWritten: framesWritten, framePosition: framePosition))
            * 1_000.0 / Double(sampleRate)
    }

    /// The frames the player keeps queued for a cushion of `cushionPackets` packets of
    /// `packetFrames` frames: at least one packet, and never zero frames.
    ///
    /// What is queued *is* the playback latency, so this is kept as small as the cushion allows.
    /// Measured on Android over the fast path, the backlog was linear in it: three packets gave
    /// 53.9 ms, two 43.9 ms, one 33.9 ms, over a fixed pipeline of about 24 ms.
    public static func targetBufferFrames(packetFrames: Int, cushionPackets: Int) -> Int {
        max(max(packetFrames, 0) * max(cushionPackets, 1), 1)
    }

    /// The cushion the phone aims for, in milliseconds: the
    /// ``targetBufferFrames(packetFrames:cushionPackets:)`` at the player's `sampleRate`, so phones
    /// with different packet sizes read on one scale. Not measured before the first packet sized it
    /// or at an unusable rate.
    public static func cushionMilliseconds(
        packetFrames: Int, cushionPackets: Int, sampleRate: Int
    ) -> Double? {
        guard packetFrames > 0, sampleRate > 0 else { return nil }
        return Double(targetBufferFrames(packetFrames: packetFrames, cushionPackets: cushionPackets))
            * 1_000.0 / Double(sampleRate)
    }

    /// The cushion and the backlog a link report ends its interval with, in milliseconds; `nil`
    /// for one that was not measured.
    public struct ReportExtras: Equatable, Sendable {
        public let cushionMilliseconds: Double?
        public let backlogMilliseconds: Double?

        public init(cushionMilliseconds: Double?, backlogMilliseconds: Double?) {
            self.cushionMilliseconds = cushionMilliseconds
            self.backlogMilliseconds = backlogMilliseconds
        }
    }

    /// What the link report due now ends its interval with, from the receive loop's player inputs.
    ///
    /// - The cushion is the buffer the player applied (`appliedBufferFrames`, when it reports one;
    ///   0 when it does not) at its `playbackRate`, or else the buffer asked of it
    ///   (``cushionMilliseconds(packetFrames:cushionPackets:sampleRate:)``). The `holdPackets` held
    ///   back for delayed redundant copies (``ReorderHold``) count in it too: they are audio held
    ///   before playing, as the player's buffer is. Not measured before the first packet sized it,
    ///   or while no player set the rate (`nil`).
    /// - The backlog is the audio queued now
    ///   (``backlogMilliseconds(framesWritten:framePosition:sampleRate:)``): the `framesWritten`
    ///   past the `presentedFrame` the output reported. Not measured when no frame could be read
    ///   (`nil`). A frame read with no rate reads as 0, as the backlog does at an unusable rate.
    ///
    /// Android takes a `timestampRead` flag beside the frame; here the frame is optional, so a frame
    /// that was not read cannot be passed at all.
    public static func reportExtras(
        packetFrames: Int, cushionPackets: Int, playbackRate: Int?, framesWritten: Int64,
        presentedFrame: Int64?, appliedBufferFrames: Int = 0, holdPackets: Int = 0
    ) -> ReportExtras {
        let rate = playbackRate ?? 0
        let player: Double?
        if appliedBufferFrames > 0 && packetFrames > 0 && rate > 0 {
            player = Double(appliedBufferFrames) * 1_000.0 / Double(rate)
        } else {
            player = cushionMilliseconds(
                packetFrames: packetFrames, cushionPackets: cushionPackets, sampleRate: rate)
        }
        // No packet, no cushion, held or not.
        let held =
            holdPackets > 0
            ? cushionMilliseconds(packetFrames: packetFrames, cushionPackets: holdPackets, sampleRate: rate)
            : 0
        let cushion = player.flatMap { player in held.map { player + $0 } }
        let backlog = presentedFrame.map {
            backlogMilliseconds(framesWritten: framesWritten, framePosition: $0, sampleRate: rate)
        }
        return ReportExtras(cushionMilliseconds: cushion, backlogMilliseconds: backlog)
    }
}
