import BandEarScreens
import Testing

/// The More of me cases of the Android client's `AtomLabelsTest` (`apps/android/app/src/test/java/
/// com/bandear/android/ui/components/AtomLabelsTest.kt`), case for case. Its group tone cases are in
/// `StripWordsParityTests`, beside the strips that wear the tones.
@Suite("More of me words: Android parity")
struct MoreOfMeWordsParityTests {

    /// `AtomLabelsTest.kt:8-11`
    @Test func moreOfMeSaysWhichChannelItLifts() {
        #expect(MoreOfMeWords.caption(on: true, channelLabel: "CH 3") == "On · CH 3 lifted")
    }

    /// `AtomLabelsTest.kt:13-16`
    @Test func moreOfMeOffInvitesATap() {
        #expect(
            MoreOfMeWords.caption(on: false, channelLabel: "CH 3") == "Off · tap to lift your channel")
    }

    /// `AtomLabelsTest.kt:18-21`
    @Test func moreOfMeWithoutAChannelLabelStillReadsNaturally() {
        #expect(MoreOfMeWords.caption(on: true, channelLabel: "  ") == "On · your channel lifted")
    }
}
