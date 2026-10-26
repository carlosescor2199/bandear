import BandEarScreens
import SwiftUI

/// The manual address form, pushed from Connect while no session runs: the Settings screen, with a
/// "Servers" link above its title that returns to the servers, as the Android client opens
/// `SettingsScreen` with `onBack`.
struct ManualAddressView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        SettingsView(session: session, backToServers: { session.manualAddressOpen = false })
            .navigationBarBackButtonHidden(true)
    }
}
