package com.bandear.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Test

class LayoutOrientationTest {
    @Test
    fun aPhoneOnItsSideIsLandscape() {
        assertEquals(LayoutOrientation.LANDSCAPE, layoutOrientationFor(widthDp = 844, heightDp = 390))
    }

    @Test
    fun anUprightPhoneIsPortrait() {
        assertEquals(LayoutOrientation.PORTRAIT, layoutOrientationFor(widthDp = 390, heightDp = 844))
    }

    @Test
    fun aSquareWindowIsPortrait() {
        assertEquals(LayoutOrientation.PORTRAIT, layoutOrientationFor(widthDp = 600, heightDp = 600))
    }

    @Test
    fun aWindowFollowsItsOwnShapeNotTheDevices() {
        // Half of a landscape tablet's split screen is taller than wide; a portrait tablet's top
        // half is wider than tall.
        assertEquals(LayoutOrientation.PORTRAIT, layoutOrientationFor(widthDp = 640, heightDp = 800))
        assertEquals(LayoutOrientation.LANDSCAPE, layoutOrientationFor(widthDp = 800, heightDp = 620))
    }

    @Test
    fun aWindowNotMeasuredYetIsPortrait() {
        assertEquals(LayoutOrientation.PORTRAIT, layoutOrientationFor(widthDp = 0, heightDp = 0))
    }
}
