import Foundation

/// A mix the musician saved on this phone, under a name: Android's `MixBank` (`MixBankStore.kt:8-9`).
public struct SavedMix: Equatable, Sendable {
    public let name: String
    public let snapshot: MixSnapshot

    public init(name: String, snapshot: MixSnapshot) {
        self.name = name
        self.snapshot = snapshot
    }
}

/// What a change to the saved mixes did, or why it did nothing. Each one is one of Android's
/// notices (`MonitorReducer.kt:132-162`, `MonitorViewModel.kt:433-478`); the words are the
/// screen's, so only the reasons live here.
public enum MixBankOutcome: Equatable, Sendable {
    /// Saved under `name`, the name as typed without the spaces around it: "Saved \"name\"".
    case saved(name: String)
    case renamed(from: String, to: String)
    /// The mix called `name` is gone, as asked: "Deleted \"name\"", whether or not one was saved.
    case deleted(name: String)
    /// A rename to the name the mix already has: nothing to do, and nothing to say.
    case unchanged
    /// The name was blank, so nothing was saved or renamed: "Enter a name for the mix".
    case nameNeeded
    /// No mix is saved as `name`: "\"name\" is no longer saved".
    case missing(name: String)
    /// Another mix already has `name`, so a rename to it was refused rather than overwrite that
    /// mix: "A mix named \"name\" already exists".
    case nameTaken(name: String)
}

/// The mixes the musician saved on this phone, in the order they were saved, and the active one.
///
/// Android splits these rules three ways: its store saves under a trimmed name, replacing a mix of
/// that name, and deletes (`MixBankStore.kt:28-44`); its view model refuses a blank name and checks
/// a rename (`MonitorViewModel.kt:433-478`); its reducer keeps the active one
/// (`MonitorReducer.kt:131-162,523-525`). Here they are one value, so the list and the active mix
/// never disagree. Keeping the list across launches is the store's (`BandEarSession`), applying a
/// recalled mix is ``MixSession``'s, and saying what happened is the screen's.
public struct MixBank: Equatable, Sendable {
    /// The saved mixes, in the order they were saved: a mix saved again under its name, or renamed,
    /// moves to the end.
    public private(set) var mixes: [SavedMix]
    /// The mix last saved or recalled, while it is still saved; nil otherwise. Like Android's, it is
    /// not remembered across launches: the store keeps only the list.
    public private(set) var activeName: String?

    /// The bank holding `mixes`, as the store hands them over, with none active.
    public init(_ mixes: [SavedMix] = []) {
        self.mixes = mixes
    }

    public var names: [String] { mixes.map(\.name) }

    /// True when nothing is saved: the screen shows its empty state.
    public var isEmpty: Bool { mixes.isEmpty }

    /// The mix saved as exactly `name`, or nil (`MonitorViewModel.kt:449`).
    public func mix(named name: String) -> SavedMix? {
        mixes.first { $0.name == name }
    }

    /// Saves `snapshot` under `name` without the spaces around it, replacing the mix already saved
    /// under that name, and makes it the active one. A blank name is refused and changes nothing
    /// (`MonitorViewModel.kt:429-442`, `MixBankStore.kt:28-34`).
    public mutating func save(_ snapshot: MixSnapshot, as name: String) -> MixBankOutcome {
        let trimmed = Self.trimmed(name)
        guard !trimmed.isEmpty else { return .nameNeeded }
        mixes.removeAll { $0.name == trimmed }
        mixes.append(SavedMix(name: trimmed, snapshot: snapshot))
        activeName = trimmed
        return .saved(name: trimmed)
    }

    /// Renames the mix saved as `name` to `newName` without the spaces around it
    /// (`MonitorViewModel.kt:457-473`). In Android's order: a blank name is refused, then a mix that
    /// is gone, then a name that is already this mix's changes nothing, then a name another mix has
    /// is refused, so no mix is ever overwritten. A renamed mix moves to the end, as Android saves it
    /// again under its new name, and stays active if it was.
    public mutating func rename(_ name: String, to newName: String) -> MixBankOutcome {
        let target = Self.trimmed(newName)
        guard !target.isEmpty else { return .nameNeeded }
        guard let renamed = mix(named: name) else { return .missing(name: name) }
        guard target != name else { return .unchanged }
        guard mix(named: target) == nil else { return .nameTaken(name: target) }
        mixes.removeAll { $0.name == name }
        mixes.append(SavedMix(name: target, snapshot: renamed.snapshot))
        if activeName == name {
            activeName = target
        }
        return .renamed(from: name, to: target)
    }

    /// Deletes the mix saved as `name` without the spaces around it (`MixBankStore.kt:36-44`). The
    /// active mix stays active only if it was another one.
    @discardableResult
    public mutating func delete(_ name: String) -> MixBankOutcome {
        let trimmed = Self.trimmed(name)
        mixes.removeAll { $0.name == trimmed }
        keepActiveOnlyWhileSaved()
        return .deleted(name: name)
    }

    /// Makes the mix saved as `name` the active one, as a recall does once the mix took it; a name
    /// no mix has leaves none active (`MonitorReducer.kt:138-142`).
    public mutating func activate(_ name: String) {
        activeName = name
        keepActiveOnlyWhileSaved()
    }

    /// The list as the store now holds it: Android's `MixesLoaded` (`MonitorReducer.kt:131`). The
    /// active mix stays active only while it is still saved.
    public mutating func reload(_ mixes: [SavedMix]) {
        self.mixes = mixes
        keepActiveOnlyWhileSaved()
    }

    /// Android's `listing` (`MonitorReducer.kt:523-525`).
    private mutating func keepActiveOnlyWhileSaved() {
        if let activeName, mix(named: activeName) == nil {
            self.activeName = nil
        }
    }

    /// `name` without the spaces and line breaks around it, as Kotlin's `trim()` reads it.
    private static func trimmed(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
