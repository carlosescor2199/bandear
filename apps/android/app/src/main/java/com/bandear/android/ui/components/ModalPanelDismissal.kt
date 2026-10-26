package com.bandear.android.ui.components

/**
 * When a [ModalPanel] tells its caller that it closed, kept apart from Compose so it can be tested
 * on the JVM.
 *
 * Several things ask a panel to close: a tap on the dimmed screen, the back key, a swipe down on
 * the sheet, the content's own `dismiss`. Two can arrive while it is still sliding away, such as a
 * double tap on Done, or Done and then the back key; only the first starts the slide. The caller
 * hears through [onDismiss] once, and only after the panel has slid off the screen, so it never
 * stops composing a panel that is still on its way out. A slide that ends with the panel still
 * showing (the sheet dragged back up while it hides) leaves it open, ready to be asked again.
 *
 * One per panel shown, remembered for as long as the panel is composed, and used on the main
 * thread only, where the panel's clicks and its slide's completion arrive.
 */
internal class ModalPanelDismissal(private val onDismiss: () -> Unit) {
    private var closing = false
    private var dismissed = false

    /**
     * Asks the panel to close. True when this request starts the slide away, which the panel then
     * runs and ends with [slideEnded]; false when it is already sliding away or gone, so this
     * request changes nothing.
     */
    fun requestClose(): Boolean {
        if (closing || dismissed) return false
        closing = true
        return true
    }

    /**
     * A slide away ended, with the panel off the screen when [hidden]: the caller hears it, the
     * first time only. A slide that ends with the panel still showing leaves it open. The sheet's
     * own dismissals (a tap on the scrim, the back key, a swipe down) slide it away themselves and
     * end here without a [requestClose].
     */
    fun slideEnded(hidden: Boolean) {
        closing = false
        if (!hidden || dismissed) return
        dismissed = true
        onDismiss()
    }
}
