import BandEarScreens
import SwiftUI
import UIKit

/// The axis a fader moves along.
enum FaderAxis: Equatable, Sendable {
    /// Down the screen: positions are measured from the top, and a drag down is positive.
    case vertical
    /// Across the screen: positions are measured from the left, and a drag right is positive.
    case horizontal
}

/// A transparent UIKit view laid over a fader that takes its touches: a tap where the finger lifts
/// without moving, and a drag along `axis` only.
///
/// A drag that starts across the axis is not the fader's: its pan recogniser declines to begin
/// unless the finger first moves further along the axis than across it, and leaves the touch to
/// whatever is behind. A sideways swipe on a vertical fader therefore still scrolls the row of strips, as
/// Android's `draggable(orientation = Vertical)` leaves it to the parent. A drag along the axis is
/// the fader's even inside a scroll view that scrolls the same way: an enclosing scroll view's pan
/// waits for the fader's to fail, so the strip pan moves under a sideways drag instead of the row.
/// UIKit's recognisers are used because a SwiftUI `DragGesture` cannot choose by direction whether
/// an enclosing `ScrollView` takes the touch.
///
/// It reports raw positions and deltas in points along `axis`; the fader turns them into values
/// with its ``FaderGesture`` or ``HorizontalFaderGesture``. A drag reports nothing for the slop the
/// finger travels before the drag is recognised, so a fader starts moving from where it was, as a
/// Compose `draggable` does. It is hidden from VoiceOver: the fader is one element that says its
/// label and value and takes the adjustable action.
struct FaderTouchSurface: UIViewRepresentable {
    let axis: FaderAxis
    var isEnabled = true
    /// The surface's length along `axis`, once laid out and again whenever it changes. It may be
    /// called while SwiftUI updates the view, so it must not change view state.
    let onLength: @MainActor (Double) -> Void
    /// A tap this far along `axis`.
    let onTap: @MainActor (Double) -> Void
    /// A drag along `axis` begins.
    let onDragStart: @MainActor () -> Void
    /// The finger moved this far along `axis` since the last report.
    let onDrag: @MainActor (Double) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(surface: self)
    }

    func makeUIView(context: Context) -> TouchView {
        let view = TouchView()
        view.backgroundColor = .clear
        view.isAccessibilityElement = false
        view.accessibilityElementsHidden = true
        let coordinator = context.coordinator
        view.onLayout = { [weak coordinator] size in coordinator?.laidOut(size) }

        let pan = UIPanGestureRecognizer(target: coordinator, action: #selector(Coordinator.panned(_:)))
        pan.delegate = coordinator
        pan.maximumNumberOfTouches = 1
        view.addGestureRecognizer(pan)
        let tap = UITapGestureRecognizer(target: coordinator, action: #selector(Coordinator.tapped(_:)))
        view.addGestureRecognizer(tap)
        return view
    }

    func updateUIView(_ view: TouchView, context: Context) {
        context.coordinator.surface = self
        view.isUserInteractionEnabled = isEnabled
        context.coordinator.laidOut(view.bounds.size)
    }

    /// A plain view that says when its size changes.
    final class TouchView: UIView {
        var onLayout: ((CGSize) -> Void)?

        override func layoutSubviews() {
            super.layoutSubviews()
            onLayout?(bounds.size)
        }
    }

    /// Turns the recognisers' callbacks into the surface's reports, always through the latest
    /// surface, so the closures see the fader's current value.
    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var surface: FaderTouchSurface
        private var reportedLength: Double?

        init(surface: FaderTouchSurface) {
            self.surface = surface
        }

        func laidOut(_ size: CGSize) {
            let length = Double(surface.axis == .vertical ? size.height : size.width)
            guard length != reportedLength else { return }
            reportedLength = length
            surface.onLength(length)
        }

        @objc func tapped(_ recogniser: UITapGestureRecognizer) {
            guard recogniser.state == .ended, let view = recogniser.view else { return }
            surface.onTap(along(recogniser.location(in: view)))
        }

        @objc func panned(_ recogniser: UIPanGestureRecognizer) {
            guard let view = recogniser.view else { return }
            switch recogniser.state {
            case .began:
                // The slop travelled before recognition is not reported: the drag starts here.
                recogniser.setTranslation(.zero, in: view)
                surface.onDragStart()
            case .changed:
                let delta = along(recogniser.translation(in: view))
                recogniser.setTranslation(.zero, in: view)
                if delta != 0 { surface.onDrag(delta) }
            default:
                break
            }
        }

        /// A drag starts only when the finger has moved further along the axis than across it
        /// (``LayoutRules/dragBegins(movedAlong:movedAcross:velocityAlong:velocityAcross:)``).
        func gestureRecognizerShouldBegin(_ recogniser: UIGestureRecognizer) -> Bool {
            guard let pan = recogniser as? UIPanGestureRecognizer, let view = pan.view else { return true }
            let moved = pan.translation(in: view)
            let velocity = pan.velocity(in: view)
            return LayoutRules.dragBegins(
                movedAlong: along(moved), movedAcross: across(moved),
                velocityAlong: along(velocity), velocityAcross: across(velocity))
        }

        /// An enclosing scroll view's pan waits for the fader's drag to fail, so a drag along the
        /// fader's axis is the fader's, whichever way the scroll view scrolls.
        func gestureRecognizer(
            _ recogniser: UIGestureRecognizer,
            shouldBeRequiredToFailBy other: UIGestureRecognizer
        ) -> Bool {
            recogniser is UIPanGestureRecognizer && other is UIPanGestureRecognizer
                && other.view is UIScrollView
        }

        private func along(_ point: CGPoint) -> Double {
            Double(surface.axis == .vertical ? point.y : point.x)
        }

        private func across(_ point: CGPoint) -> Double {
            Double(surface.axis == .vertical ? point.x : point.y)
        }
    }
}
