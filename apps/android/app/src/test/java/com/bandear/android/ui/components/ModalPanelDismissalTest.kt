package com.bandear.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ModalPanelDismissalTest {
    private var dismissed = 0
    private val dismissal = ModalPanelDismissal(onDismiss = { dismissed++ })

    @Test
    fun aRequestToCloseStartsTheSlideAwayButTellsTheCallerNothingYet() {
        assertTrue(dismissal.requestClose())
        assertEquals(0, dismissed)
    }

    @Test
    fun theCallerHearsOnceThePanelHasSlidOffTheScreen() {
        dismissal.requestClose()
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun aSecondRequestWhileThePanelSlidesAwayChangesNothing() {
        // A double tap on Done, or Done and then the back key.
        assertTrue(dismissal.requestClose())
        assertFalse(dismissal.requestClose())
        assertEquals(0, dismissed)
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun theCallerHearsOnlyOnceHoweverManySlidesEnd() {
        // Done hides the sheet while the sheet's own dismissal hides it too: both end hidden.
        dismissal.requestClose()
        dismissal.slideEnded(hidden = true)
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun aSlideCutShortWithThePanelShowingLeavesItOpenAndReadyToCloseAgain() {
        // The sheet dragged back up while it hides.
        dismissal.requestClose()
        dismissal.slideEnded(hidden = false)
        assertEquals(0, dismissed)
        assertTrue(dismissal.requestClose())
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun theSheetsOwnDismissalTellsTheCallerOnceItIsHidden() {
        // A tap on the scrim, the back key or a swipe down: the sheet slides itself away first.
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun theSheetsOwnDismissalCuttingDoneShortStillTellsTheCallerOnce() {
        dismissal.requestClose()
        // The back key's hide takes over from Done's, which ends with the sheet still showing.
        dismissal.slideEnded(hidden = false)
        dismissal.slideEnded(hidden = true)
        assertEquals(1, dismissed)
    }

    @Test
    fun onceTheCallerHeardNoRequestStartsAnotherSlide() {
        dismissal.requestClose()
        dismissal.slideEnded(hidden = true)
        assertFalse(dismissal.requestClose())
        assertEquals(1, dismissed)
    }
}
