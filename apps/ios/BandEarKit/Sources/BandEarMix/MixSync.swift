/// How the phone matches the desk's answers to the mixes it sent.
///
/// The phone numbers every change it sends, and the desk answers each `mix` in order with that
/// number (`seq`): with a `mix_ack` and the whole mix it now plays, or with an `error`. ``latest`` is
/// the number of the newest change, taken in the same step as the change, so a send that is still
/// queued already counts. ``answered`` is the newest number the desk answered. Only the answer to
/// ``latest`` is the desk's latest word on the mix; while an older one is outstanding, any mix the
/// desk reports is overtaken by a mix of the phone's still on its way.
///
/// The numbers only grow, across reconnects too: every connection sends the mix again under
/// ``latest``, so its answer settles it.
///
/// Ported from the Android client's `MixSyncState` (`MonitorUiState.kt:258-278`).
public struct MixSync: Equatable, Sendable {
    /// The number of the newest change the phone made to send; 0 before the first.
    public var latest: UInt64
    /// The newest number the desk answered.
    public var answered: UInt64

    public init(latest: UInt64 = 0, answered: UInt64 = 0) {
        self.latest = latest
        self.answered = answered
    }

    /// True when the desk answered the newest change, so nothing of the phone's is on its way.
    public var isSettled: Bool { answered >= latest }

    /// A new change, numbered as the newest.
    public func changed() -> MixSync {
        MixSync(latest: latest + 1, answered: answered)
    }

    /// The desk answered change `sequence`; it answers in order, so every older one is answered too.
    public func answeredUpTo(_ sequence: UInt64) -> MixSync {
        MixSync(latest: latest, answered: max(answered, sequence))
    }
}
