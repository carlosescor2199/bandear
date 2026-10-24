/// The rules the app's views lay themselves out and take touches by, on plain numbers: which way a
/// space is laid out, whose a drag is, how tall a modal panel's content is drawn. The views read
/// their sizes and gestures from SwiftUI and UIKit and ask here, so `swift test` proves the rules
/// without a simulator.
///
/// Sizes and distances are points, as the views measure them.
public enum LayoutRules {
    /// Whether a space `width` by `height` is laid out in landscape: only when it is wider than it
    /// is tall, so a square, and a space not measured yet (0 by 0), is portrait. The Android client
    /// decides the same (`layoutOrientationFor`, `ui/components/LayoutOrientation.kt:18-19`).
    public static func isLandscape(width: Double, height: Double) -> Bool {
        width > height
    }

    /// Whether a drag belongs to a control that moves along one axis, such as a fader: only when the
    /// finger has moved further along the axis than across it. Otherwise it is left to whatever is
    /// behind the control, so a sideways swipe on a vertical fader still scrolls the row of strips.
    ///
    /// `movedAlong` and `movedAcross` are how far the finger has moved along and across the axis.
    /// Before it has moved at all (both 0), its velocity (`velocityAlong`, `velocityAcross`)
    /// decides instead. A drag that is no further along than across is not the control's: the
    /// exact diagonal declines, and so does a finger that has neither moved nor any velocity.
    public static func dragBegins(
        movedAlong: Double, movedAcross: Double, velocityAlong: Double, velocityAcross: Double
    ) -> Bool {
        let notMovedYet = movedAlong == 0 && movedAcross == 0
        let along = notMovedYet ? velocityAlong : movedAlong
        let across = notMovedYet ? velocityAcross : movedAcross
        return abs(along) > abs(across)
    }

    /// The most a modal panel's content may take of a space `spaceHeight` tall before it scrolls:
    /// three quarters of it for the portrait bottom sheet, and no cap of its own for the landscape
    /// side panel, which is as tall as the space.
    public static func panelContentMaxHeight(spaceHeight: Double, isLandscape: Bool) -> Double {
        isLandscape ? .infinity : spaceHeight * 0.75
    }

    /// How tall a modal panel's content is drawn: the height it would like (`ideal`), but no more
    /// than `maxHeight` (``panelContentMaxHeight(spaceHeight:isLandscape:)``) nor the height its
    /// parent offers (`proposed`, `nil` when the parent leaves the height to the content). A short
    /// panel hugs its content and a tall one scrolls.
    public static func panelContentHeight(ideal: Double, maxHeight: Double, proposed: Double?) -> Double {
        min(ideal, maxHeight, proposed ?? .infinity)
    }
}
