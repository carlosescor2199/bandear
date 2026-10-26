import BandEarMix
import BandEarProtocol
import BandEarScreens
import Testing

/// A session over five announced channels, "Vox", "Gtr", "Keys" paired with an unnamed fourth, and
/// an unnamed fifth, with More of me `on` and `channel` as the musician's.
private func session(on: Bool, channel: Int?) -> MixSession {
    let names = ["Vox", "Gtr", "Keys", "", ""]
    var session = MixSession()
    session.mix.sourceChannels = names.count
    session.mix.levels = Array(repeating: 100, count: names.count)
    session.mix.channelDetails = names.enumerated().map { index, name in
        SourceChannel(name: name, kind: .other, isHidden: false, isLinkedToNext: index == 2)
    }
    session.moreOfMe = MoreOfMeState(isEnabled: on, musicianChannel: channel)
    return session
}

/// The More of me words for the iOS notices and session, which no Android test pins.
@Suite("More of me words")
struct MoreOfMeWordsTests {

    /// `MoreOfMeToggle.kt:63`
    @Test func theToggleIsCalledMoreOfMe() {
        #expect(MoreOfMeWords.title == "More of me")
    }

    /// `MoreOfMeNotice.message` (`MonitorUiState.kt:335-351`), case for case.
    @Test func eachNoticeSaysWhyMoreOfMeIsNotOn() {
        #expect(
            MoreOfMeWords.message(for: .needsChannel)
                == "Choose your channel first, then turn More of me on")
        #expect(
            MoreOfMeWords.message(for: .noChannelSelected) == "More of me is off: no channel is chosen")
        #expect(
            MoreOfMeWords.message(for: .channelOutOfRange)
                == "More of me is off: your channel is no longer in the mix")
        #expect(
            MoreOfMeWords.message(for: .mixChangedElsewhere)
                == "More of me is off: your mix was changed from the desktop")
        #expect(
            MoreOfMeWords.message(for: .channelLocked)
                == "More of me is off: the engineer locked your channel")
        #expect(
            MoreOfMeWords.message(for: .following)
                == "More of me is off while your mix follows another musician's")
        #expect(
            MoreOfMeWords.message(for: .channelPaired)
                == "More of me is off: your channel is now part of a stereo pair")
    }

    /// Android's `MoreOfMeRow` (`MixScreen.kt:147,250-252`): the caption names the musician's channel
    /// as its strip does, and reads naturally with none.
    @Test func theCaptionNamesTheMusiciansChannelAsItsStripDoes() {
        #expect(MoreOfMeWords.caption(for: session(on: true, channel: 2)) == "On · Keys lifted")
        #expect(MoreOfMeWords.caption(for: session(on: true, channel: 4)) == "On · CH 5 lifted")
        #expect(
            MoreOfMeWords.caption(for: session(on: false, channel: 2)) == "Off · tap to lift your channel")
        #expect(MoreOfMeWords.caption(for: session(on: true, channel: nil)) == "On · your channel lifted")
    }
}
