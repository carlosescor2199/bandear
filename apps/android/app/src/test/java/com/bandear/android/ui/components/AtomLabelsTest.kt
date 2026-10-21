package com.bandear.android.ui.components

import com.bandear.android.ui.theme.GroupTone
import org.junit.Assert.assertEquals
import org.junit.Test

class AtomLabelsTest {
    @Test
    fun moreOfMeSaysWhichChannelItLifts() {
        assertEquals("On · CH 3 lifted", moreOfMeCaption(on = true, channelLabel = "CH 3"))
    }

    @Test
    fun moreOfMeOffInvitesATap() {
        assertEquals("Off · tap to lift your channel", moreOfMeCaption(on = false, channelLabel = "CH 3"))
    }

    @Test
    fun moreOfMeWithoutAChannelLabelStillReadsNaturally() {
        assertEquals("On · your channel lifted", moreOfMeCaption(on = true, channelLabel = "  "))
    }

    @Test
    fun groupsTakeTheTonesInTurn() {
        assertEquals(GroupTone.TEAL, groupToneFor(0))
        assertEquals(GroupTone.VIOLET, groupToneFor(1))
        assertEquals(GroupTone.ORANGE, groupToneFor(2))
        assertEquals(GroupTone.BLUE, groupToneFor(3))
        assertEquals(GroupTone.TEAL, groupToneFor(4))
    }

    @Test
    fun aNegativeIndexStillGetsATone() {
        assertEquals(GroupTone.BLUE, groupToneFor(-1))
    }
}
