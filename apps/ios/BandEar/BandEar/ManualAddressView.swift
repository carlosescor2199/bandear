import BandEarScreens
import SwiftUI

/// The manual address form, pushed from Connect while no session runs: the Settings screen, with a
/// back button that returns to the servers, as the Android client opens `SettingsScreen` with
/// `onBack`.
struct ManualAddressView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        SettingsView(session: session)
            .toolbar(.visible, for: .navigationBar)
            .navigationBarBackButtonHidden(true)
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button {
                        session.manualAddressOpen = false
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "chevron.left")
                                .fontWeight(.semibold)
                                .accessibilityHidden(true)
                            Text(SettingsWords.backToServers)
                        }
                    }
                }
            }
    }
}
