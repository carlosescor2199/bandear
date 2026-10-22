import BandEarSession
import Foundation
import Testing

/// A throwaway `UserDefaults` suite for `body`, whose every call of `launch` is one launch of the
/// app: a fresh `UserDefaults` over the same suite, as a new process opens it.
///
/// The suite is named by a path in a new temporary directory, which keeps its file there: a suite
/// named like an app keeps an empty file in `~/Library/Preferences` even after its domain is
/// removed. The domain and the directory are removed afterwards, so nothing outlives the test.
private func withThrowawaySuite(
    _ body: (_ launch: () throws -> UserDefaults) throws -> Void
) throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("bandear-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let suite = directory.appendingPathComponent("musician").path
    defer {
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: directory)
    }
    try body { try #require(UserDefaults(suiteName: suite)) }
}

/// The `UserDefaults` adapter, on a throwaway suite: what one launch writes, the next one reads.
@Suite("Musician store: UserDefaults")
struct UserDefaultsMusicianStoreTests {
    @Test("remembers nothing on the first launch")
    func emptyOnTheFirstLaunch() throws {
        try withThrowawaySuite { launch in
            let store = UserDefaultsMusicianStore(try launch())

            #expect(store.clientID == nil)
            #expect(store.musicianName == "")
            #expect(store.musicianChannel == nil)
        }
    }

    @Test("what one launch writes, the next one reads")
    func remembersAcrossLaunches() throws {
        try withThrowawaySuite { launch in
            let first = UserDefaultsMusicianStore(try launch())
            first.clientID = "phone-1"
            first.musicianName = "Ana"
            first.musicianChannel = 3

            let next = UserDefaultsMusicianStore(try launch())

            #expect(next.clientID == "phone-1")
            #expect(next.musicianName == "Ana")
            #expect(next.musicianChannel == 3)
        }
    }

    @Test("channel 0 is a channel, and forgetting the channel forgets it")
    func channelZeroAndForgetting() throws {
        try withThrowawaySuite { launch in
            let store = UserDefaultsMusicianStore(try launch())

            store.musicianChannel = 0
            #expect(UserDefaultsMusicianStore(try launch()).musicianChannel == 0)

            store.musicianChannel = nil
            #expect(UserDefaultsMusicianStore(try launch()).musicianChannel == nil)
        }
    }

    /// Android reads its own "none", `-1`, the same way (`MixBankStore.kt:47-48`).
    @Test("reads a damaged channel as none")
    func aDamagedChannelIsNone() throws {
        try withThrowawaySuite { launch in
            let defaults = try launch()
            let store = UserDefaultsMusicianStore(defaults)
            let key = "bandear.musician_channel"
            store.musicianChannel = 2
            // The adapter writes under `key`, so the damage below lands where it reads.
            try #require(defaults.object(forKey: key) as? Int == 2)

            let damage: [Any] = [-1, "two", 1.5]
            for damaged in damage {
                defaults.set(damaged, forKey: key)
                #expect(store.musicianChannel == nil, "\(damaged) is not a channel")
            }
        }
    }

    @Test("the id the first launch makes is the one every later launch sends")
    func theClientIDOutlivesTheLaunch() throws {
        try withThrowawaySuite { launch in
            let first = ClientIdentity.clientID(in: UserDefaultsMusicianStore(try launch()))
            let next = ClientIdentity.clientID(in: UserDefaultsMusicianStore(try launch()))

            #expect(ClientIdentity.validClientID(first) == first)
            #expect(next == first)
        }
    }
}

/// ``ClientIdentity/clientID(in:)``, the form the session uses: Android's `resolveClientId` reading
/// and writing the store.
@Suite("Client identity")
struct ClientIdentityTests {
    @Test("makes and saves an id the desk keeps when the store has none")
    func makesOneOnTheFirstLaunch() {
        let store = FakeMusicianStore()

        let id = ClientIdentity.clientID(in: store)

        #expect(ClientIdentity.validClientID(id) == id)
        #expect(store.clientID == id)
    }

    @Test("keeps a stored id the desk keeps, and replaces one it would discard")
    func keepsOrReplacesTheStoredOne() {
        let kept = FakeMusicianStore(clientID: "phone-1")
        let damaged = FakeMusicianStore(clientID: "phone 1")

        #expect(ClientIdentity.clientID(in: kept) == "phone-1")
        #expect(kept.clientID == "phone-1")

        let replaced = ClientIdentity.clientID(in: damaged)
        #expect(replaced != "phone 1")
        #expect(ClientIdentity.validClientID(replaced) == replaced)
        #expect(damaged.clientID == replaced)
    }
}
