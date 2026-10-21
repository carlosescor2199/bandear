package com.bandear.android.ui.components

/**
 * [FaderGesture] for a fader laid out left to right: quiet at the left end, loud at the right.
 *
 * Compose reports a touch as its distance from the left edge and a rightward drag as a positive
 * delta. [FaderGesture] measures from the loud end, with a positive delta toward the quiet end, so
 * both are flipped here, in one place: a tap is measured from the right edge and a drag delta is
 * negated. A rightward drag and a tap near the right end therefore raise the level.
 *
 * With [PanScale] it drives the pan control: hard left at the left end, hard right at the right.
 */
internal class HorizontalFaderGesture(thumbWidthPx: Float, scale: FaderScale = LevelScale) {
    private val gesture = FaderGesture(thumbWidthPx, scale)

    /** The fader's laid-out width; 0 until it is laid out. */
    var widthPx: Float
        get() = gesture.lengthPx
        set(value) {
            gesture.lengthPx = value
        }

    /** False while the thumb has no room to move; every input is ignored then. */
    val acceptsInput: Boolean
        get() = gesture.acceptsInput

    /** A tap [xPx] from the left edge: the thumb's centre jumps under the finger. */
    fun tap(xPx: Float, current: Int): Int? = gesture.tap(widthPx - xPx, current)

    /** A drag begins, seeded from the [current] level so touching the fader never makes it jump. */
    fun startDrag(current: Int) = gesture.startDrag(current)

    /** The finger moved [dxPx] to the right (negative: to the left) during a drag. */
    fun drag(dxPx: Float, current: Int): Int? = gesture.drag(-dxPx, current)

    /** A screen reader sets the level to [target]; the axis does not matter here. */
    fun step(target: Float, current: Int): Int? = gesture.step(target, current)
}
