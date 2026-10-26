import BandEarDiscovery
import BandEarScreens
import Combine
import SwiftUI
import UIKit

/// The whole app: Connect while no session runs, with the manual address form pushed from it, and
/// the Mix, Mixes and Settings tabs while one does, chosen by ``AppScreen`` as the Android client's
/// `MonitorApp` chooses.
///
/// It owns the session and discovery for the app's life. Discovery listens while the app is in
/// front: it starts at launch, stops in the background, and starts again on each return, which
/// also retries after the browser failed. Both triggers can fire at launch, appear and then the
/// scene turning active; ``DeskDiscoveryModel/start()`` does nothing while a run is under way, so
/// discovery starts once all the same.
///
/// The app is dark only, on the brand's ground, whatever the phone's appearance. The modal panels
/// the Mix screen opens are shown from here, so they cover the tab bar or the rail too.
struct RootView: View {
    @StateObject private var session = SessionModel()
    @StateObject private var discovery = DeskDiscoveryModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        RootContent(session: session, discovery: discovery)
            .overlay(alignment: .top) { StatusBarBackdrop() }
            .modalPanelHost()
            .screenOrientationHost()
            .background(BandEarColors.ground.ignoresSafeArea())
            .preferredColorScheme(.dark)
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
}

/// The ground behind the status bar. The screens hide the navigation bar and draw their titles
/// themselves, so without it a screen scrolled up would show its content behind the clock.
private struct StatusBarBackdrop: View {
    var body: some View {
        Color.clear
            .frame(height: 0)
            .background(BandEarColors.ground.ignoresSafeArea(edges: .top))
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// The screen ``AppScreen`` picks: Connect with its manual address form, or the session's tabs.
private struct RootContent: View {
    @ObservedObject var session: SessionModel
    @ObservedObject var discovery: DeskDiscoveryModel

    var body: some View {
        if screen.showsTabs {
            TabShell(session: session)
        } else {
            NavigationStack {
                ConnectView(session: session, discovery: discovery)
                    .navigationDestination(isPresented: manualAddressShown) {
                        ManualAddressView(session: session)
                    }
            }
        }
    }

    private var screen: AppScreen {
        AppScreen(
            sessionRunning: session.isRunning, tab: session.tab,
            manualAddressOpen: session.manualAddressOpen)
    }

    /// Pushed while the screen is the manual address form; going back closes it.
    private var manualAddressShown: Binding<Bool> {
        Binding(
            get: { screen == .manualAddress },
            set: { session.manualAddressOpen = $0 })
    }
}

/// The session's three tabs: the selected one's screen, with the ``BottomTabBar`` under it in
/// portrait and the ``NavigationRail`` beside it in landscape (Android's `MonitorApp` scaffold).
///
/// Every tab stays built while another shows, as a `TabView` keeps them: a tab keeps what was typed
/// in it, its scroll position and its open expander when the musician comes back to it. The tabs
/// also keep their place in the view tree whichever way the screen is laid out, so a rotation
/// keeps all of it too. Mixes and Settings each sit in a navigation stack of their own; Mix sits
/// in none, so its panels reach the host on the root.
private struct TabShell: View {
    @ObservedObject var session: SessionModel
    @Environment(\.screenOrientation) private var orientation
    /// Whether the software keyboard is up. The tab bar hides under it, as a tab bar does, rather
    /// than riding up on top of it.
    @State private var keyboardShown = false

    var body: some View {
        let landscape = orientation.isLandscape
        let layout = landscape ? AnyLayout(HStackLayout(spacing: 0)) : AnyLayout(VStackLayout(spacing: 0))
        layout {
            if landscape {
                // Under a keyboard the rail keeps its place, its foot under the keyboard, rather
                // than squeezing into the space above it.
                NavigationRail(selection: $session.tab)
                    .frame(minHeight: 0, maxHeight: .infinity)
                    .ignoresSafeArea(.keyboard)
            }
            ZStack {
                page(.mix) {
                    MixView(session: session)
                }
                page(.mixes) {
                    NavigationStack { MixesView(mixes: session.mixes) }
                }
                page(.settings) {
                    NavigationStack { SettingsView(session: session) }
                }
            }
            // No tab's own height, the hidden ones' included, may push the screen up past the
            // keyboard or the bar off it.
            .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
            if !landscape, !keyboardShown {
                BottomTabBar(selection: $session.tab)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in
            keyboardShown = true
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
            keyboardShown = false
        }
        .onChange(of: session.tab) { _ in
            // A field left focused on the tab just hidden would go on taking the typing.
            UIApplication.shared.sendAction(
                #selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        }
    }

    /// The screen of `tab`, shown while it is the selected tab and kept, hidden, while it is not:
    /// it takes no touches then, and VoiceOver does not find it.
    private func page(_ tab: AppTab, @ViewBuilder content: () -> some View) -> some View {
        let shown = session.tab == tab
        return content()
            .opacity(shown ? 1 : 0)
            .allowsHitTesting(shown)
            .accessibilityHidden(!shown)
            .zIndex(shown ? 1 : 0)
    }
}
