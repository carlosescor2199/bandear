import BandEarScreens
import Testing

/// The group tone cases of the Android client's `AtomLabelsTest` (`apps/android/app/src/test/java/
/// com/bandear/android/ui/components/AtomLabelsTest.kt`), case for case. Its More of me cases are in
/// `MoreOfMeWordsParityTests`.
@Suite("Strip words: Android parity")
struct StripWordsParityTests {

    /// `AtomLabelsTest.kt:23-30`
    @Test func groupsTakeTheTonesInTurn() {
        #expect(GroupTone.forGroup(0) == .teal)
        #expect(GroupTone.forGroup(1) == .violet)
        #expect(GroupTone.forGroup(2) == .orange)
        #expect(GroupTone.forGroup(3) == .blue)
        #expect(GroupTone.forGroup(4) == .teal)
    }

    /// `AtomLabelsTest.kt:32-35`
    @Test func aNegativeIndexStillGetsATone() {
        #expect(GroupTone.forGroup(-1) == .blue)
    }
}
