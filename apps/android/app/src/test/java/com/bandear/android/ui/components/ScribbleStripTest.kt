package com.bandear.android.ui.components

import com.bandear.android.ui.theme.BandEarDarkColors
import com.bandear.android.ui.theme.ColorToken
import com.bandear.android.ui.theme.GroupTone
import org.junit.Assert.assertEquals
import org.junit.Test

class ScribbleStripTest {
    private val colors = BandEarDarkColors

    @Test
    fun aChannelInAGroupTakesThatGroupsTone() {
        assertEquals(ScribbleTone.Group(GroupTone.TEAL), scribbleToneFor(mine = false, groupIndex = 0))
        assertEquals(ScribbleTone.Group(GroupTone.VIOLET), scribbleToneFor(mine = false, groupIndex = 5))
    }

    @Test
    fun aChannelInNoGroupIsNeutral() {
        assertEquals(ScribbleTone.NoGroup, scribbleToneFor(mine = false, groupIndex = null))
    }

    @Test
    fun theMusiciansOwnChannelIsMarkedAsTheirsWhateverItsGroup() {
        assertEquals(ScribbleTone.Mine, scribbleToneFor(mine = true, groupIndex = 2))
        assertEquals(ScribbleTone.Mine, scribbleToneFor(mine = true, groupIndex = null))
    }

    @Test
    fun aGroupToneColoursTheStripWithTheGroupsBackgroundAndText() {
        assertEquals(
            ScribbleColors(colors.groupBackground(GroupTone.ORANGE), colors.groupText(GroupTone.ORANGE)),
            scribbleColors(ScribbleTone.Group(GroupTone.ORANGE), colors),
        )
    }

    @Test
    fun theMusiciansOwnChannelIsAmberOnTheAmberTint() {
        assertEquals(ScribbleColors(colors.tint(ColorToken.AMBER), colors.accent), scribbleColors(ScribbleTone.Mine, colors))
    }

    @Test
    fun noGroupIsPlainTextOnARaisedSurface() {
        assertEquals(ScribbleColors(colors.surfaceRaised, colors.text), scribbleColors(ScribbleTone.NoGroup, colors))
    }
}
