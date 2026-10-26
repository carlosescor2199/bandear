import Foundation

/// What the phone remembers about its musician from one launch to the next: who it registers as
/// with the desk, the name the musician last connected with, and the channel they call theirs.
///
/// The session reads and writes it through this port, so its rules are proved with an in-memory
/// fake from the test target and the device's storage stays in one adapter,
/// ``UserDefaultsMusicianStore``. Android draws the same seam (`MonitorStore` and `clientId`,
/// `MonitorCollaborators.kt:47-69`).
public protocol MusicianStore: AnyObject {
    /// This installation's client id, or nil before one was made. ``ClientIdentity`` reads it.
    var clientID: String? { get set }
    /// The name the musician last started a session with, or empty before the first.
    var musicianName: String { get set }
    /// The channel the musician calls theirs, by the channel its strip is known by, or nil when
    /// none is chosen.
    var musicianChannel: Int? { get set }
}

/// The store the app uses: `UserDefaults`, where Android keeps the same values in
/// `SharedPreferences` (`ClientIdentity.kt:49-62`, `MixBankStore.kt:46-69`).
public final class UserDefaultsMusicianStore: MusicianStore {
    /// Where each value is kept. Changing one forgets what every earlier launch remembered.
    private enum Key {
        static let clientID = "bandear.client_id"
        static let musicianName = "bandear.musician_name"
        static let musicianChannel = "bandear.musician_channel"
    }

    private let defaults: UserDefaults

    public init(_ defaults: UserDefaults) {
        self.defaults = defaults
    }

    public var clientID: String? {
        get { defaults.string(forKey: Key.clientID) }
        set { defaults.set(newValue, forKey: Key.clientID) }
    }

    public var musicianName: String {
        get { defaults.string(forKey: Key.musicianName) ?? "" }
        set { defaults.set(newValue, forKey: Key.musicianName) }
    }

    /// A stored value that is no channel (not a whole number, or a negative one) reads as none, as
    /// Android reads its own "none", `-1` (`MixBankStore.kt:47-48`).
    public var musicianChannel: Int? {
        get {
            guard let channel = defaults.object(forKey: Key.musicianChannel) as? Int, channel >= 0
            else { return nil }
            return channel
        }
        set {
            if let newValue {
                defaults.set(newValue, forKey: Key.musicianChannel)
            } else {
                defaults.removeObject(forKey: Key.musicianChannel)
            }
        }
    }
}
