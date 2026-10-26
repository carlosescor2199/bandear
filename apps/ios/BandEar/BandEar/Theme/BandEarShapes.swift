import SwiftUI

/// The corner radii of the BandEar look, from the smallest tag to the round status pill, as the
/// Android client's `BandEarShapes` (`ui/theme/Shape.kt:8-26`) sets them.
enum BandEarShapes {
    /// Group chips and the "Locked" and "Following" tags.
    static let tag: CGFloat = 6
    /// The mute and solo keys on a channel strip, and the scribble strip.
    static let key: CGFloat = 8
    /// Buttons and segmented controls.
    static let control: CGFloat = 10
    /// Cards in a list, such as a saved mix, and the rail's items.
    static let card: CGFloat = 12
    /// Channel strips, the master strip and the More of me toggle.
    static let panel: CGFloat = 14
    /// The status pill's round ends, and the open edge of a modal panel.
    static let pill: CGFloat = 22

    /// A rounded rectangle with one of the radii above, drawn with the continuous corners iOS uses.
    static func rounded(_ radius: CGFloat) -> RoundedRectangle {
        RoundedRectangle(cornerRadius: radius, style: .continuous)
    }
}
