/// Where a point on a vertical fader's track reads, and where a level sits on it.
///
/// Ported from the Android client's `FaderMath.kt`. Positions are points on iOS where Android
/// counts pixels; the arithmetic is the same. The top of the track is ``max`` and the bottom is 0,
/// like a mixing desk.
public enum FaderMath {
    /// The highest fader value: the top of the track (`FADER_MAX`, `FaderMath.kt:5-6`).
    public static let max = 100

    /// How far one VoiceOver swipe moves a level or a pan. Android's faders are Compose sliders over
    /// `0..100` with no steps, which TalkBack moves a twentieth of the range at a time
    /// (`AccessibilitySliderStepsCount`); iOS moves them the same 5.
    public static let screenReaderStep = 5

    /// The fader value under a point `y` below the top of a track `trackHeight` tall
    /// (`faderValueAt`, `FaderMath.kt:8-19`).
    ///
    /// A point above or below the track reads as the nearest end. A track with no height reads as
    /// the bottom, and so does a point that is not a number; touch input never asks then, because
    /// ``FaderGesture`` ignores a fader with no travel and a touch with no place.
    ///
    /// The value is clamped before it becomes an `Int`, as Kotlin's `roundToInt` saturates, so no
    /// point traps. Kotlin rounds halves up and Swift away from zero; they differ only on negative
    /// halves, which the clamp turns into 0 either way.
    public static func value(at y: Double, trackHeight: Double) -> Int {
        guard trackHeight > 0, !y.isNaN else { return 0 }
        let fromBottom = 1 - y / trackHeight
        return wholeLevel(fromBottom * Double(max))
    }

    /// Where `value` sits on a track `trackHeight` tall, below its top: the inverse of
    /// ``value(at:trackHeight:)``, used to place the thumb. A value outside `0...max` is drawn at
    /// the nearest end (`faderOffsetOf`, `FaderMath.kt:21-29`).
    public static func offset(of value: Int, trackHeight: Double) -> Double {
        let fromBottom = Double(Swift.min(Swift.max(value, 0), max)) / Double(max)
        return trackHeight * (1 - fromBottom)
    }

    /// `level` rounded to a whole level and kept in `0...max`; `level` is a number.
    static func wholeLevel(_ level: Double) -> Int {
        Int(Swift.min(Swift.max(level.rounded(), 0), Double(max)))
    }

    /// The levels a vertical fader marks with ticks beside its track: the top, every quarter, the
    /// bottom (`FADER_TICK_LEVELS`, `FaderMath.kt:32-33`).
    public static let tickLevels = [100, 75, 50, 25, 0]

    /// Where the centre of the cap sits for `value` on a fader `length` long whose cap is `cap`
    /// tall, below its top (`faderCentreOf`, `FaderMath.kt:35-43`).
    ///
    /// The cap's edge travels the length minus the cap, as ``FaderGesture`` measures it, so the
    /// centre runs from half a cap below the top to half a cap above the bottom; the track, its fill,
    /// the ticks and the limit marker are all drawn from it, and a tap on the drawn cap reads back as
    /// its level. A fader no taller than its cap keeps the cap at the top.
    public static func centre(of value: Int, length: Double, cap: Double) -> Double {
        offset(of: value, trackHeight: Swift.max(length - cap, 0)) + cap / 2
    }

    /// Where the ticks of ``tickLevels`` go on a fader `length` long, top to bottom
    /// (`faderTickCentres`, `FaderMath.kt:45-47`).
    public static func tickCentres(length: Double, cap: Double) -> [Double] {
        tickLevels.map { centre(of: $0, length: length, cap: cap) }
    }

    /// Where the marker of an engineer's `limit` goes on a fader `length` long: across the track at
    /// the height the cap's centre would have at that level, so the cap reaches the marker exactly at
    /// the limit. `nil` when there is no limit to mark; a limit outside `0...max` is drawn at the
    /// nearest end (`faderLimitCentre`, `FaderMath.kt:49-55`).
    public static func limitCentre(_ limit: Int?, length: Double, cap: Double) -> Double? {
        limit.map { centre(of: $0, length: length, cap: cap) }
    }
}

extension PanScale {
    /// Where the centre of a thumb `thumb` wide sits for `pan` on a control `length` wide, from its
    /// left end: ``position(of:travel:detentShare:)`` over the travel the gesture uses (the width
    /// minus the thumb), plus half a thumb (`panCentreOf`, `PanScale.kt:57-64`). The centre is drawn
    /// in the middle of the control; a control no wider than its thumb keeps the thumb at the start.
    ///
    /// It sits with the fader maths because it places what ``HorizontalFaderGesture`` reads.
    public static func thumbCentre(of pan: Int, length: Double, thumb: Double) -> Double {
        position(of: pan, travel: Swift.max(length - thumb, 0)) + thumb / 2
    }
}

/// How a fader turns where its thumb is into a value and back. Offsets are measured from the
/// fader's loud end (where ``FaderMath/max`` sits: the top of a vertical fader, the right end of a
/// horizontal one) along a travel `travel` long, as ``FaderGesture`` measures them.
///
/// Ported from the Android client's `FaderScale`, `LevelScale` and `PanScale` objects
/// (`PanScale.kt:88-113`); Android's interface becomes the two cases here.
public enum FaderScale: Equatable, Sendable {
    /// The levels' even scale: 0 at one end, ``FaderMath/max`` at the other.
    case level
    /// The pan's scale, hard left to hard right with ``PanScale``'s centre detent.
    case pan

    /// The value with the thumb's edge `offset` from the loud end.
    public func value(at offset: Double, travel: Double) -> Int {
        switch self {
        case .level: FaderMath.value(at: offset, trackHeight: travel)
        case .pan: PanScale.value(at: travel - offset, travel: travel)
        }
    }

    /// Where `value` puts the thumb's edge, from the loud end.
    public func offset(of value: Int, travel: Double) -> Double {
        switch self {
        case .level: FaderMath.offset(of: value, trackHeight: travel)
        case .pan: travel - PanScale.position(of: value, travel: travel)
        }
    }
}

/// The arithmetic behind a fader's touch and screen-reader input, kept apart from the views so
/// `swift test` proves it.
///
/// Ported from the Android client's `FaderGesture.kt`. Positions are points along the fader's axis,
/// measured from its loud end (the top of a vertical fader), and a positive delta moves toward the
/// quiet end. The thumb's centre follows the finger; its edge travels the fader's length minus the
/// thumb.
///
/// Every input returns the level to report, or `nil` when there is nothing to report: the level
/// would not change, or the fader has no travel. A fader laid out no taller than its thumb (or not
/// laid out yet) has nowhere to move the thumb, so it ignores input instead of reading it as the
/// bottom: an input can never silently drop a level to silence. A touch that is not a finite number
/// is ignored too, where Kotlin would throw.
///
/// `scale` turns the thumb's place into a value: even for a level, with a centre detent for the pan.
public struct FaderGesture: Sendable {
    private let thumbLength: Double
    private let scale: FaderScale

    /// The fader's laid-out length along its axis; 0 until it is laid out.
    public var length: Double = 0

    /// Where the thumb's centre is during a drag, from the loud end.
    private var dragCentre: Double = 0

    public init(thumbLength: Double, scale: FaderScale = .level) {
        self.thumbLength = thumbLength
        self.scale = scale
    }

    private var travel: Double {
        Swift.max(length - thumbLength, 0)
    }

    /// False while the thumb has no room to move; every input is ignored then.
    public var acceptsInput: Bool {
        travel > 0
    }

    /// A tap at `position`: the thumb's centre jumps under the finger (`FaderGesture.kt:34-38`).
    public func tap(at position: Double, current: Int) -> Int? {
        guard acceptsInput, position.isFinite else { return nil }
        return changed(levelWithCentre(at: position), current)
    }

    /// A drag begins. It is seeded from the `current` level rather than from where the finger
    /// landed, so touching the fader never makes the level jump (`FaderGesture.kt:40-46`).
    public mutating func startDrag(current: Int) {
        dragCentre = scale.offset(of: current, travel: travel) + thumbLength / 2
    }

    /// The finger moved `delta` during a drag. The thumb follows the finger even when the caller
    /// did not apply the last level, so the fader never drifts away from the finger
    /// (`FaderGesture.kt:48-56`).
    public mutating func drag(by delta: Double, current: Int) -> Int? {
        guard acceptsInput, delta.isFinite else { return nil }
        dragCentre += delta
        return changed(levelWithCentre(at: dragCentre), current)
    }

    /// A screen reader sets the level to `target`, rounded to a whole level and kept in range
    /// (`FaderGesture.kt:58-62`).
    public func step(to target: Double, current: Int) -> Int? {
        guard acceptsInput, !target.isNaN else { return nil }
        return changed(FaderMath.wholeLevel(target), current)
    }

    /// A VoiceOver swipe up: ``FaderMath/screenReaderStep`` louder, or further right for a pan.
    public func increment(current: Int) -> Int? {
        step(to: Double(current + FaderMath.screenReaderStep), current: current)
    }

    /// A VoiceOver swipe down: ``FaderMath/screenReaderStep`` quieter, or further left for a pan.
    public func decrement(current: Int) -> Int? {
        step(to: Double(current - FaderMath.screenReaderStep), current: current)
    }

    private func levelWithCentre(at centre: Double) -> Int {
        scale.value(at: centre - thumbLength / 2, travel: travel)
    }

    /// Reports are de-duplicated: a level equal to the one shown is not reported again
    /// (`FaderGesture.kt:67-68`).
    private func changed(_ next: Int, _ current: Int) -> Int? {
        next == current ? nil : next
    }
}

/// ``FaderGesture`` for a fader laid out left to right: quiet at the left end, loud at the right.
///
/// Ported from the Android client's `HorizontalFaderGesture.kt`. A touch arrives as its distance
/// from the left edge and a rightward drag as a positive delta. ``FaderGesture`` measures from the
/// loud end, with a positive delta toward the quiet end, so both are flipped here, in one place: a
/// tap is measured from the right edge and a drag delta is negated. A rightward drag and a tap near
/// the right end therefore raise the level.
///
/// With ``FaderScale/pan`` it drives the strip's pan: hard left at the left end, hard right at the
/// right.
public struct HorizontalFaderGesture: Sendable {
    private var gesture: FaderGesture

    public init(thumbWidth: Double, scale: FaderScale = .level) {
        gesture = FaderGesture(thumbLength: thumbWidth, scale: scale)
    }

    /// The fader's laid-out width; 0 until it is laid out.
    public var width: Double {
        get { gesture.length }
        set { gesture.length = newValue }
    }

    /// False while the thumb has no room to move; every input is ignored then.
    public var acceptsInput: Bool {
        gesture.acceptsInput
    }

    /// A tap `x` from the left edge: the thumb's centre jumps under the finger.
    public func tap(atX x: Double, current: Int) -> Int? {
        gesture.tap(at: width - x, current: current)
    }

    /// A drag begins, seeded from the `current` level so touching the fader never makes it jump.
    public mutating func startDrag(current: Int) {
        gesture.startDrag(current: current)
    }

    /// The finger moved `dx` to the right (negative: to the left) during a drag.
    public mutating func drag(byX dx: Double, current: Int) -> Int? {
        gesture.drag(by: -dx, current: current)
    }

    /// A screen reader sets the level to `target`; the axis does not matter here.
    public func step(to target: Double, current: Int) -> Int? {
        gesture.step(to: target, current: current)
    }

    /// A VoiceOver swipe up: ``FaderMath/screenReaderStep`` further right.
    public func increment(current: Int) -> Int? {
        gesture.increment(current: current)
    }

    /// A VoiceOver swipe down: ``FaderMath/screenReaderStep`` further left.
    public func decrement(current: Int) -> Int? {
        gesture.decrement(current: current)
    }
}
