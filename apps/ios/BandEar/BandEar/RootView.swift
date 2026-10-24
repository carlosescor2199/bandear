import BandEarDiscovery
import BandEarScreens
import SwiftUI

/// The whole app: Connect while no session runs, with the manual address form pushed from it, and
/// the Mix, Mixes and Settings tabs while one does, chosen by ``AppScreen`` as the Android client's
/// `MonitorApp` chooses.
///
/// It owns the session and discovery for the app's life. Discovery listens while the app is in
/// front: it starts at launch, stops in the background, and starts again on each return, which
/// also retries after the browser failed. Both triggers can fire at launch, appear and then the
/// scene turning active; ``DeskDiscoveryModel/start()`` does nothing while a run is under way, so
/// discovery starts once all the same.
struct RootView: View {
    @StateObject private var session = SessionModel()
    @StateObject private var discovery = DeskDiscoveryModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        content
            .onAppear { discovery.start() }
            .onChange(of: scenePhase) { phase in
                switch phase {
                case .active:
                    discovery.start()
                case .background:
                    discovery.stop()
                default:
                    break
                }
            }
    }

    private var screen: AppScreen {
        AppScreen(
            sessionRunning: session.isRunning, tab: session.tab,
            manualAddressOpen: session.manualAddressOpen)
    }

    @ViewBuilder private var content: some View {
        if screen.showsTabs {
            TabView(selection: $session.tab) {
                // The Mix screen owns its own navigation, if it needs any.
                MixView(session: session)
                    .tabItem { Label(AppTab.mix.label, systemImage: "slider.vertical.3") }
                    .tag(AppTab.mix)
                NavigationStack { MixesView(mixes: session.mixes) }
                    .tabItem { Label(AppTab.mixes.label, systemImage: "square.stack") }
                    .tag(AppTab.mixes)
                NavigationStack { SettingsView(session: session) }
                    .tabItem { Label(AppTab.settings.label, systemImage: "gearshape") }
                    .tag(AppTab.settings)
            }
        } else {
            NavigationStack {
                ConnectView(session: session, discovery: discovery)
                    .navigationDestination(isPresented: manualAddressShown) {
                        ManualAddressView(session: session)
                    }
            }
        }
    }

    /// Pushed while the screen is the manual address form; going back closes it.
    private var manualAddressShown: Binding<Bool> {
        Binding(
            get: { screen == .manualAddress },
            set: { session.manualAddressOpen = $0 })
    }
}
