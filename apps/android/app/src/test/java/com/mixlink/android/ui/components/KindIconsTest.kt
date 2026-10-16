package com.mixlink.android.ui.components

import com.mixlink.android.ChannelKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KindIconsTest {
    @Test
    fun everyKindHasAnIconOfItsOwn() {
        val icons = ChannelKind.entries.map(::kindIcon)

        assertEquals(ChannelKind.entries.size, icons.toSet().size)
        assertEquals(ChannelKind.entries.map { it.label }, icons.map { it.name })
        // Drawing an icon parses its paths; every one draws something.
        for (icon in icons) {
            assertTrue(icon.name, icon.root.size > 0)
        }
    }

    @Test
    fun theStereoMarkIsNotAKindsIcon() {
        assertTrue(LineIcons.Stereo !in ChannelKind.entries.map(::kindIcon))
        assertTrue(LineIcons.Stereo.root.size > 0)
    }

    @Test
    fun theFollowMarkDrawsAnArrowToABar() {
        assertEquals("Following", LineIcons.Follow.name)
        assertEquals(3, LineIcons.Follow.root.size)
    }

    @Test
    fun theLockDrawsAPadlock() {
        assertEquals("Locked", LineIcons.Lock.name)
        assertEquals(2, LineIcons.Lock.root.size)
    }
}
