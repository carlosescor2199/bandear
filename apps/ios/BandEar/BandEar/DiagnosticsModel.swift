import BandEarPlayback
import BandEarSession
import Combine
import Foundation

/// The receiver's figures, which change with almost every packet, published apart from the rest of
/// the session so that only the views showing them are rebuilt by a report: Settings and its
/// ``DiagnosticsSection``. ``SessionModel`` owns it and mirrors it at each change of the session.
///
/// Each value means what it means on ``MonitorAudio``.
@MainActor
final class DiagnosticsModel: ObservableObject {
    /// The receiver's latest metrics (``MonitorAudio/receiverStats``).
    @Published private(set) var receiverStats: ReceiverStats?
    /// The latest link report (``MonitorAudio/linkReport``).
    @Published private(set) var linkReport: LinkStatsReport?
    /// Underruns added up over this session's link reports.
    @Published private(set) var underrunTotal: Int64 = 0
    /// Why the receiver last refused a packet, failed to open the output, or lost its socket.
    @Published private(set) var receiverError: (any Error)?
    /// Whether the audio output was reaching the musician at the last packet played; `true` until
    /// a packet says otherwise.
    @Published private(set) var outputPlaying = true

    /// Copies what `audio` holds. A `@Published` property publishes on every assignment, even of
    /// the value it holds, so each one is assigned only when it changed.
    func mirror(_ audio: MonitorAudio) {
        if receiverStats != audio.receiverStats { receiverStats = audio.receiverStats }
        if linkReport != audio.linkReport { linkReport = audio.linkReport }
        if underrunTotal != audio.underrunTotal { underrunTotal = audio.underrunTotal }
        if !Self.sameError(receiverError, audio.receiverError) {
            receiverError = audio.receiverError
        }
        let playing = audio.receiverStats?.outputPlaying ?? true
        if outputPlaying != playing { outputPlaying = playing }
    }

    /// Whether two receiver errors are the same. `any Error` is not `Equatable`, so two errors are
    /// the same when they are of one type and read the same in full (`String(reflecting:)`, which
    /// spells an enum's case with its values): the diagnostics show no more of an error than its
    /// description.
    private static func sameError(_ a: (any Error)?, _ b: (any Error)?) -> Bool {
        switch (a, b) {
        case (nil, nil):
            true
        case let (a?, b?):
            ObjectIdentifier(type(of: a)) == ObjectIdentifier(type(of: b))
                && String(reflecting: a) == String(reflecting: b)
        default:
            false
        }
    }
}
