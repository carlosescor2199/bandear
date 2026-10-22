import BandEarProtocol
import BandEarSession
import Foundation
import Testing

/// The Android client's `ClientIdentityTest.kt`
/// (`apps/android/app/src/test/java/com/bandear/android/`): what counts as a client id (the desk's
/// rule), how a new one is made, and when a stored one is kept or replaced. Android's
/// `MAX_CLIENT_ID_LENGTH` is ``Registration/maximumClientIDLength``.
@Suite("Client identity: Android parity")
struct ClientIdentityParityTests {
    @Test func aNewClientIdIsARandomUuidTheServerAccepts() {
        let first = ClientIdentity.newClientID()
        let second = ClientIdentity.newClientID()

        // Android compares with `UUID.fromString(first).toString()`, which writes it in lowercase.
        #expect(UUID(uuidString: first)?.uuidString.lowercased() == first)
        #expect(ClientIdentity.validClientID(first) == first)
        #expect(first != second)
    }

    @Test func anIdOfUpTo64LettersDigitsAndHyphensIsValid() {
        let longest = String(repeating: "A-9", count: 21) + "z"
        #expect(longest.count == Registration.maximumClientIDLength)

        for id in ["a", "phone-1", "ABC-def-123", longest] {
            #expect(ClientIdentity.validClientID(id) == id)
        }
    }

    @Test func anythingElseIsNotAClientId() {
        let invalid = [
            "", "has space", "under_score", "slash/id", "acentuación",
            String(repeating: "a", count: Registration.maximumClientIDLength + 1),
        ]

        for id in invalid {
            #expect(ClientIdentity.validClientID(id) == nil, "'\(id)' must not be a client id")
        }
        #expect(ClientIdentity.validClientID(nil) == nil)
    }

    @Test func aStoredValidIdIsKeptAndNothingIsSaved() {
        var saved: [String] = []

        let id = ClientIdentity.resolveClientID(
            stored: "phone-1", generate: { "unused" }, save: { saved.append($0) })

        #expect(id == "phone-1")
        #expect(saved.isEmpty, "nothing new is stored")
    }

    @Test func withNoStoredIdANewOneIsGeneratedAndSaved() {
        var saved: [String] = []

        let id = ClientIdentity.resolveClientID(
            stored: nil, generate: { "fresh-id" }, save: { saved.append($0) })

        #expect(id == "fresh-id")
        #expect(saved == ["fresh-id"])
    }

    @Test func aCorruptedStoredIdIsReplacedAndSaved() {
        var saved: [String] = []

        let id = ClientIdentity.resolveClientID(
            stored: "not valid!", generate: { "fresh-id" }, save: { saved.append($0) })

        #expect(id == "fresh-id")
        #expect(saved == ["fresh-id"])
    }
}
