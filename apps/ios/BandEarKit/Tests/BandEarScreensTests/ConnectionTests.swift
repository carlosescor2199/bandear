import BandEarDiscovery
import BandEarSession
import Testing

@testable import BandEarScreens

/// What only the iOS port has to answer about the connection: how a desk is named, which Android
/// names by address alone, the following banner, which Android builds in its view, and the pill's
/// spoken words, which Android does not test.
@Suite("Connection words")
struct ConnectionTests {
    private let foundDesk = DeskEndpoint.bonjour(
        name: "BandEar on STUDIO", type: "_bandear._tcp", domain: "local.")

    @Test func aTypedDeskIsNamedAsAndroidNamesAServer() {
        #expect(
            ConnectionWords.deskLabel(.manual(host: "192.168.1.34", controlPort: 50_001))
                == "192.168.1.34:50001")
    }

    @Test func aFoundDeskIsNamedByItsBonjourName() {
        #expect(ConnectionWords.deskLabel(foundDesk) == "BandEar on STUDIO")
    }

    @Test func theReconnectingBannerNamesAFoundDeskByItsName() {
        let status = ConnectionStatus(
            connection: .reconnecting, sessionRunning: true, desk: foundDesk)
        #expect(status.banner?.title == "Reconnecting to BandEar on STUDIO")
    }

    // `MixScreenText.kt:130`: `server?.label ?: "the server"`.
    @Test func aSessionWithNoDeskReconnectsToTheServer() {
        let status = ConnectionStatus(connection: .reconnecting, sessionRunning: true)
        #expect(status.banner?.title == "Reconnecting to the server")
    }

    // The words of `MixScreenTextTest.aFollowedMixIsNamedInWords` (`MixScreenTextTest.kt:55-64`),
    // in the banner `MixScreen.kt:133-143` builds from them.
    @Test func theFollowingBannerSaysWhoseMixAndWhatStaysTheMusicians() {
        #expect(
            ConnectionBanner.following("Ana")
                == ConnectionBanner(
                    tone: .blue,
                    title: "Following Ana's mix",
                    detail:
                        "Your channels and groups follow that mix. Your master, level cap and mute are still yours.",
                    busy: false))
        #expect(ConnectionBanner.following(" Ana ")?.title == "Following Ana's mix")
        #expect(ConnectionBanner.following(" ")?.title == "Following another musician's mix")
        #expect(ConnectionBanner.following(nil) == nil)
    }

    // `StatusPill.kt:45`.
    @Test func thePillIsSpokenAsOnAndroid() {
        #expect(ConnectionPill.reconnecting.spokenDescription == "Connection status: Reconnecting")
        #expect(ConnectionPill.notConnected.spokenDescription == "Connection status: Not connected")
    }
}
