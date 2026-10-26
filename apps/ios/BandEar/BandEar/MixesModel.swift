import BandEarMix
import BandEarSession
import Combine
import Foundation

/// The saved mixes, published for the Mixes tab alone: what is saved, which one is active, and what
/// the last action did. ``SessionModel`` owns it, so a save or a rename rebuilds only the tab, and
/// a recall reaches the Mix screen through the session, as any edit does.
///
/// Wiring only: the rules are ``SavedMixes``' (`BandEarSession`), and the words are
/// ``SavedMixesWords``' (`BandEarScreens`). Each value means what it means on ``SavedMixes``.
@MainActor
final class MixesModel: ObservableObject {
    /// The saved mixes, in the order they were saved.
    @Published private(set) var mixes: [SavedMix] = []
    /// The mix last saved or recalled, while it is still saved.
    @Published private(set) var activeName: String?
    /// What the last save, recall, rename or delete did, or why it did nothing.
    @Published private(set) var notice: SavedMixesNotice?
    /// The notice's number, which changes with every notice an action gives, even in the words of
    /// the last one.
    @Published private(set) var noticeNumber = 0

    private let savedMixes: SavedMixes

    init(_ savedMixes: SavedMixes) {
        self.savedMixes = savedMixes
        savedMixes.stateDidChange = { [weak self] in self?.mirror() }
        mirror()
    }

    // MARK: Actions

    /// Saves the current mix as `name`; says whether it was saved, so the field keeps a refused name.
    @discardableResult
    func save(as name: String) -> Bool {
        savedMixes.save(as: name)
    }

    /// Brings back the mix saved as `name`, unless the mix follows another musician's.
    func recall(_ name: String) {
        savedMixes.recall(name)
    }

    func rename(_ name: String, to newName: String) {
        savedMixes.rename(name, to: newName)
    }

    func delete(_ name: String) {
        savedMixes.delete(name)
    }

    // MARK: Plumbing

    /// Copies the saved mixes after a change. A `@Published` property publishes on every
    /// assignment, even of the value it holds, so each one is assigned only when it changed.
    private func mirror() {
        if mixes != savedMixes.mixes { mixes = savedMixes.mixes }
        if activeName != savedMixes.activeName { activeName = savedMixes.activeName }
        if notice != savedMixes.notice { notice = savedMixes.notice }
        if noticeNumber != savedMixes.noticeNumber { noticeNumber = savedMixes.noticeNumber }
    }
}
