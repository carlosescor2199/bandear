import BandEarMix

/// What the last save, recall, rename or delete of a saved mix did, or why it did nothing: one of
/// Android's saved-mix notices (`MonitorReducer.kt:132-162`, `MonitorViewModel.kt:433-478`). Only the
/// reasons live here; the words are the screen's (`SavedMixesWords` in `BandEarScreens`).
public enum SavedMixesNotice: Equatable, Sendable {
    /// What the saved mixes did with a save, a rename or a delete, or why they did nothing; also a
    /// recall whose mix is no longer saved (``MixBankOutcome/missing(name:)``). Never
    /// ``MixBankOutcome/unchanged``: a rename to the name a mix already has says nothing, and leaves
    /// the last notice on show, as Android's does.
    case bank(MixBankOutcome)
    /// The mix saved as `name` is the mix now: "Recalled \"name\"".
    case recalled(name: String)
    /// The mix saved as `name` was not recalled, and `because` says why.
    case notRecalled(name: String, because: RecallRefusal)
}

/// The mixes the musician saved on this phone, and what they do to the session's mix: Android's
/// saved-mix actions (`MonitorViewModel.kt:426-478`) with the state its reducer keeps for them
/// (`MonitorReducer.kt:131-162`).
///
/// - The rules are ``MixBank``'s, and only its: which name a mix is saved under, which mix a name
///   already taken replaces, what a rename refuses, which mix is active. This keeps a bank, reads
///   the list back from the ``MixBankStore`` before every action, as Android reads its store
///   (`MonitorViewModel.kt:449,459`), and writes the list back once the bank changed it.
/// - A save holds the session's mix as the musician left it (``MixState/snapshot``), never the More
///   of me lift.
/// - A recall goes through the ``ControlSession`` as ``MixEvent/mixRecalled(_:)``, the musician's
///   change, so the session numbers it and sends it to the desk by its own rule. While the mix
///   follows another musician's (``MixSession/recallRefusal``) nothing is recalled, nothing is
///   sent, and the notice says why.
/// - The active mix is not remembered across launches, as on Android: a new one lists what the
///   store holds, with none active and nothing to say yet.
///
/// Everything happens on the main actor, where the session lives.
@MainActor
public final class SavedMixes {
    /// The saved mixes, in the order they were saved; empty while none is.
    public var mixes: [SavedMix] { bank.mixes }
    /// The mix last saved or recalled, while it is still saved; nil otherwise.
    public var activeName: String? { bank.activeName }
    /// What the last action did, or why it did nothing; nil until the first one.
    public private(set) var notice: SavedMixesNotice?
    /// The number of ``notice`` among the notices given, 0 until the first. Every action that says
    /// something numbers its notice one more than the last, even when it says what the last one
    /// said, so a screen tells a repeated outcome (a second blank save, a re-save) from none; an
    /// action that says nothing leaves it as it was.
    public private(set) var noticeNumber = 0
    /// Called on the main actor after anything above may have changed.
    public var stateDidChange: (() -> Void)?

    private var bank: MixBank
    private let session: ControlSession
    private let store: any MixBankStore

    /// Lists the saved mixes `store` keeps, with none active and nothing said: Android's
    /// `MixesLoaded` at launch (`MonitorViewModel.kt:519-526`). It only reads the store: nothing is
    /// applied to `session`'s mix, and nothing is sent or written.
    public init(session: ControlSession, store: any MixBankStore) {
        self.session = session
        self.store = store
        bank = MixBank(store.savedMixes)
    }

    /// Saves the session's mix as `name`, without the spaces around it, replacing the mix already
    /// saved under that name, and makes it the active one. Says whether it was saved: a blank name
    /// saves nothing, and the screen keeps what was typed (`MonitorViewModel.kt:429-442`).
    @discardableResult
    public func save(as name: String) -> Bool {
        let snapshot = session.mixSession.mix.snapshot
        guard case .saved = changingBank({ $0.save(snapshot, as: name) }) else { return false }
        return true
    }

    /// Recalls the mix saved as `name` into the session's mix, and makes it the active one
    /// (`MonitorViewModel.kt:444-455`). A mix that is no longer saved is said so, with the list read
    /// again; while the mix follows another musician's, nothing is recalled.
    public func recall(_ name: String) {
        bank.reload(store.savedMixes)
        if let saved = bank.mix(named: name) {
            if let refusal = session.mixSession.recallRefusal {
                say(.notRecalled(name: name, because: refusal))
            } else {
                session.apply(.mixRecalled(saved.snapshot))
                bank.activate(name)
                say(.recalled(name: name))
            }
        } else {
            say(.bank(.missing(name: name)))
        }
        stateDidChange?()
    }

    /// Renames the mix saved as `name` to `newName`, without the spaces around it
    /// (`MonitorViewModel.kt:457-473`). A blank name, a mix that is gone, and a name another mix
    /// already has are refused with a notice; the name the mix already has changes nothing and says
    /// nothing.
    public func rename(_ name: String, to newName: String) {
        changingBank { $0.rename(name, to: newName) }
    }

    /// Deletes the mix saved as `name` (`MonitorViewModel.kt:475-478`).
    public func delete(_ name: String) {
        changingBank { $0.delete(name) }
    }

    /// Runs `change` on the bank as the store holds it now, keeps the list in the store when the
    /// change altered it, says what the change did, and tells the observers.
    @discardableResult
    private func changingBank(_ change: (inout MixBank) -> MixBankOutcome) -> MixBankOutcome {
        bank.reload(store.savedMixes)
        let before = bank.mixes
        let outcome = change(&bank)
        if bank.mixes != before {
            store.savedMixes = bank.mixes
        }
        if outcome != .unchanged {
            say(.bank(outcome))
        }
        stateDidChange?()
        return outcome
    }

    /// Shows `notice` as a new one, numbered after the last, even in the last one's words.
    private func say(_ notice: SavedMixesNotice) {
        self.notice = notice
        noticeNumber += 1
    }
}
