import BandEarMix
import Foundation

/// Where the phone keeps the mixes the musician saved, from one launch to the next.
///
/// The session reads and writes it through this port, so its rules are proved with an in-memory
/// fake and the device's storage stays in one adapter, ``UserDefaultsMixBankStore``. Android draws
/// the same seam (`MonitorStore`, `MonitorCollaborators.kt:48-54`), with saving and deleting on
/// the store; here those rules are ``MixBank``'s, so the store only keeps the list.
public protocol MixBankStore: AnyObject {
    /// The saved mixes, in the order they were saved; empty before the first.
    var savedMixes: [SavedMix] { get set }
}

/// The store the app uses: `UserDefaults`, where Android keeps the same list in
/// `SharedPreferences`, written as the same JSON (`MixBankStore.kt:22-26,74-114`).
///
/// Reading never fails: a list that is missing or cannot be read is no saved mix, and an entry that
/// cannot be read is skipped with every other entry kept, because a saved mix can sit on a phone
/// for weeks and may have been written by an older version of the app.
public final class UserDefaultsMixBankStore: MixBankStore {
    /// Where the list is kept: Android's `banks` key. Changing it forgets every mix saved so far.
    private static let key = "bandear.banks"

    private let defaults: UserDefaults

    public init(_ defaults: UserDefaults) {
        self.defaults = defaults
    }

    public var savedMixes: [SavedMix] {
        get { SavedMixesJSON.mixes(fromJSON: defaults.string(forKey: Self.key)) }
        set { defaults.set(SavedMixesJSON.json(of: newValue), forKey: Self.key) }
    }
}

/// The saved mixes as Android writes them: a JSON list of `name` and `snapshot` objects, each
/// snapshot a string holding its own JSON object (`MixBankStore.kt:74-114`, `MixSnapshot.kt:26-87`).
/// Android's keys and defaults are kept, so a mix moves between the two clients' code unchanged.
///
/// Internal so the parity tests reach it, as Android's `serializeBanks` and `parseBanks` are.
enum SavedMixesJSON {
    /// Android's `serializeBanks`.
    static func json(of mixes: [SavedMix]) -> String {
        encoded(mixes.map { StoredMix(name: $0.name, snapshot: json(of: $0.snapshot)) })
    }

    /// Android's `parseBanks`: never fails. A missing or unreadable list is no saved mix, and an
    /// entry that is no object, has no text name, or has no text snapshot that reads as one is
    /// skipped.
    static func mixes(fromJSON json: String?) -> [SavedMix] {
        guard let json, let list = object(json) as? [Any] else { return [] }
        return list.compactMap { entry in
            guard let entry = entry as? [String: Any], let name = entry["name"] as? String,
                let text = entry["snapshot"] as? String, let snapshot = snapshot(fromJSON: text)
            else { return nil }
            return SavedMix(name: name, snapshot: snapshot)
        }
    }

    /// Android's `MixSnapshot.toJson`.
    static func json(of snapshot: MixSnapshot) -> String {
        encoded(
            StoredSnapshot(
                channelGains: snapshot.channelGains, channelPans: snapshot.channelPans,
                channelMutes: snapshot.channelMutes, channelSolos: snapshot.channelSolos,
                groupLevels: snapshot.groupLevels, volumePercent: snapshot.volumePercent,
                maxLevelPercent: snapshot.maxLevelPercent, muted: snapshot.muted))
    }

    /// Android's `MixSnapshot.fromJson`, which throws where this is nil: when `json` is no object.
    ///
    /// Every value is read as Android's `optInt` and `optBoolean` read it. A list that is missing or
    /// not a list is empty; in a list, a number is read as its whole part, a `true` or `false` as
    /// itself, and anything else as 0 or false, so a list keeps its length and every channel its
    /// place. The master and the cap read as 100 and the mute as false unless they are a number and
    /// a `true` or `false`. Android also reads a number or a `true` written as text; this reads text
    /// as the default, as the `config` decoder does, since neither client ever writes one.
    static func snapshot(fromJSON json: String) -> MixSnapshot? {
        guard let fields = object(json) as? [String: Any] else { return nil }
        func ints(_ key: String) -> [Int] {
            (fields[key] as? [Any])?.map { int($0) ?? 0 } ?? []
        }
        func bools(_ key: String) -> [Bool] {
            (fields[key] as? [Any])?.map { bool($0) ?? false } ?? []
        }
        return MixSnapshot(
            channelGains: ints("channelGains"), channelPans: ints("channelPans"),
            channelMutes: bools("channelMutes"), channelSolos: bools("channelSolos"),
            groupLevels: ints("groupLevels"),
            volumePercent: fields["volumePercent"].flatMap(int) ?? 100,
            maxLevelPercent: fields["maxLevelPercent"].flatMap(int) ?? 100,
            muted: fields["muted"].flatMap(bool) ?? false)
    }

    /// One saved mix as Android writes it, its snapshot kept as a string.
    private struct StoredMix: Encodable {
        let name: String
        let snapshot: String
    }

    /// A snapshot as Android writes it, under Android's keys.
    private struct StoredSnapshot: Encodable {
        let channelGains: [Int]
        let channelPans: [Int]
        let channelMutes: [Bool]
        let channelSolos: [Bool]
        let groupLevels: [Int]
        let volumePercent: Int
        let maxLevelPercent: Int
        let muted: Bool
    }

    /// `value` as JSON text. Encoding cannot fail: a non-finite `Double` is the only thing
    /// `JSONEncoder` refuses, and nothing here holds one.
    private static func encoded(_ value: some Encodable) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        guard let data = try? encoder.encode(value) else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    /// The list or object `text` holds, or nil when it holds neither.
    private static func object(_ text: String) -> Any? {
        try? JSONSerialization.jsonObject(with: Data(text.utf8))
    }

    /// A number's whole part, or nil for anything that is no number, a `true` or `false` included.
    /// One too large to be an `Int` is nil too.
    private static func int(_ value: Any) -> Int? {
        guard let number = value as? NSNumber, !isTrueOrFalse(number) else { return nil }
        let whole = number.doubleValue.rounded(.towardZero)
        guard whole.isFinite, whole >= Double(Int.min), whole < Double(Int.max) else { return nil }
        return Int(whole)
    }

    /// A `true` or `false`, or nil for anything else, a number included.
    private static func bool(_ value: Any) -> Bool? {
        guard let number = value as? NSNumber, isTrueOrFalse(number) else { return nil }
        return number.boolValue
    }

    /// `JSONSerialization` reads `true`, `false` and every number as an `NSNumber`; only the first
    /// two are `CFBoolean`s.
    private static func isTrueOrFalse(_ number: NSNumber) -> Bool {
        CFGetTypeID(number) == CFBooleanGetTypeID()
    }
}
