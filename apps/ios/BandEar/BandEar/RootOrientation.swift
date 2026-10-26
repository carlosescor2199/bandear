import SwiftUI

extension EnvironmentValues {
    /// The ``LayoutOrientation`` of the whole screen, as ``View/screenOrientationHost()`` measures
    /// it: whether the tabs sit in a bar or a rail, and whether Connect, Mixes and Settings lay out
    /// in one column or two. Portrait where no host is installed.
    var screenOrientation: LayoutOrientation {
        get { self[ScreenOrientationKey.self] }
        set { self[ScreenOrientationKey.self] = newValue }
    }
}

extension View {
    /// Hands the views inside ``EnvironmentValues/screenOrientation``: the orientation of the space
    /// this view is given, again on every rotation or window resize. Put it on the root.
    ///
    /// The space is measured as if no keyboard were up. ``OrientationReader`` reads the space left
    /// above the keyboard, and in portrait that space can be wider than it is tall: a screen with a
    /// text field would switch to its landscape layout as the field is focused, and back as it is
    /// left.
    func screenOrientationHost() -> some View {
        modifier(ScreenOrientationHost())
    }
}

private struct ScreenOrientationKey: EnvironmentKey {
    nonisolated static var defaultValue: LayoutOrientation { .portrait }
}

/// The size of the host's space, keyboard or not; nil from the views that do not measure it, which
/// are reduced with it and must not hide it.
private struct ScreenSizeKey: PreferenceKey {
    nonisolated static var defaultValue: CGSize? { nil }

    nonisolated static func reduce(value: inout CGSize?, nextValue: () -> CGSize?) {
        value = nextValue() ?? value
    }
}

private struct ScreenOrientationHost: ViewModifier {
    @State private var orientation = LayoutOrientation.portrait

    func body(content: Content) -> some View {
        content
            .environment(\.screenOrientation, orientation)
            .background {
                // Runs on under the keyboard, so the keyboard never changes the size it reads.
                GeometryReader { proxy in
                    Color.clear.preference(key: ScreenSizeKey.self, value: proxy.size)
                }
                .ignoresSafeArea(.keyboard)
            }
            .onPreferenceChange(ScreenSizeKey.self) { size in
                if let size {
                    orientation = LayoutOrientation(size)
                }
            }
    }
}
