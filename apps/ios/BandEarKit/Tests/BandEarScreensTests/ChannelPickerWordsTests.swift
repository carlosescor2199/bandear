import BandEarMix
import BandEarProtocol
import BandEarScreens
import Testing

/// A session over five announced channels, "Vox", "Gtr", "Keys" paired with an unnamed fourth, and
/// an unnamed fifth, with `channel` as the musician's.
private func session(channel: Int?) -> MixSession {
    let names = ["Vox", "Gtr", "Keys", "", ""]
    var session = MixSession()
    session.mix.sourceChannels = names.count
    session.mix.levels = Array(repeating: 100, count: names.count)
    session.mix.channelDetails = names.enumerated().map { channel, name in
        SourceChannel(name: name, kind: .other, isHidden: false, isLinkedToNext: channel == 2)
    }
    session.moreOfMe = MoreOfMeState(isEnabled: false, musicianChannel: channel)
    return session
}

/// The "My channel" picker's words (`MixScreen.kt:290-374`) and the channel it names.
@Suite("Channel picker words")
struct ChannelPickerWordsTests {

    @Test func thePickersWordsAreAndroids() {
        #expect(ChannelPickerWords.title == "MY CHANNEL")
        #expect(ChannelPickerWords.none == "None")
        #expect(ChannelPickerWords.waiting == "Waiting for the server's channels")
        #expect(ChannelPickerWords.chooseHint == "Choose your channel")
        #expect(ChannelPickerWords.chosen == "Chosen")
    }

    /// Android passes `moreOfMe.musicianChannel?.let(mix::labelOf)` (`MixScreen.kt:147`): the
    /// channel as its strip names it, or nothing when none is chosen.
    @Test func theChosenChannelIsNamedAsItsStripNamesIt() {
        #expect(ChannelPickerWords.chosenLabel(in: session(channel: 2)) == "Keys")
        #expect(ChannelPickerWords.chosenLabel(in: session(channel: 4)) == "CH 5")
        #expect(ChannelPickerWords.chosenLabel(in: session(channel: nil)) == nil)
        #expect(
            ChannelPickerWords.description(ChannelPickerWords.chosenLabel(in: session(channel: 2)))
                == "My channel: Keys. Tap to change")
    }
}
