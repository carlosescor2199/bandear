import BandEarScreens
import SwiftUI

/// Which way the space a screen has is laid out: landscape when it is wider than it is tall,
/// portrait otherwise (a square counts as portrait).
///
/// It reads the space a view is given, not the device, so it also answers on iPad, in Split View
/// and in Slide Over, where the device's orientation says nothing about the window's shape.
///
/// A screen that lays out differently by orientation keeps the state of its controls across a
/// rotation only while they keep their place in the view tree: prefer switching an `AnyLayout`
/// (`HStackLayout` against `VStackLayout`) or a modifier's arguments over an `if` that builds two
/// different trees, and keep the state that must survive a rotation above the switch or in the
/// model.
enum LayoutOrientation: Equatable, Sendable {
    case portrait
    case landscape

    /// The orientation of a space `size` big (``LayoutRules/isLandscape(width:height:)``).
    init(_ size: CGSize) {
        let landscape = LayoutRules.isLandscape(width: Double(size.width), height: Double(size.height))
        self = landscape ? .landscape : .portrait
    }

    var isLandscape: Bool {
        self == .landscape
    }
}

/// Hands its content the ``LayoutOrientation`` of the space it is given, again on every rotation
/// or window resize.
///
/// It takes all the space offered, as `GeometryReader` does, and places its content at the top
/// leading corner of it; the content usually fills it.
struct OrientationReader<Content: View>: View {
    @ViewBuilder let content: (LayoutOrientation) -> Content

    var body: some View {
        GeometryReader { proxy in
            content(LayoutOrientation(proxy.size))
        }
    }
}
