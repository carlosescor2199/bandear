package com.bandear.android.ui.components

import androidx.compose.ui.graphics.vector.ImageVector
import com.bandear.android.ChannelKind

/**
 * The line icons of what a channel carries, drawn like [LineIcons] on a 24-unit grid and named after
 * their kind. A strip shows one next to the channel's name; the name says what it is, so the icon
 * adds a shape to find it by and is never the only way to tell.
 */
private object KindIcons {
    /** A microphone on its stand. */
    val Voice: ImageVector by lazy {
        lineIcon(
            ChannelKind.VOICE.label,
            "M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0z",
            "M6 11a6 6 0 0 0 12 0",
            "M12 17v4M9 21h6",
        )
    }

    /** A guitar: a round body with its sound hole, the neck and the head. */
    val Guitar: ImageVector by lazy {
        lineIcon(
            ChannelKind.GUITAR.label,
            "M3.5 15.5a5 5 0 1 0 10 0a5 5 0 1 0-10 0",
            "M7 15.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0",
            "M12 12l8-8M18.5 2.5l3 3",
        )
    }

    /** A bass clef. */
    val Bass: ImageVector by lazy {
        lineIcon(
            ChannelKind.BASS.label,
            "M5 9a4.5 4.5 0 0 1 9 0c0 5.5-4 9-9 11",
            "M18 6.5a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1",
            "M18 11.5a0.5 0.5 0 1 0 0 1a0.5 0.5 0 1 0 0-1",
        )
    }

    /** A keyboard with its black keys. */
    val Keys: ImageVector by lazy {
        lineIcon(ChannelKind.KEYS.label, "M3 5h18v14H3z", "M8 5v7M12 5v7M16 5v7")
    }

    /** A drum seen from the side, with two sticks over it. */
    val Drums: ImageVector by lazy {
        lineIcon(
            ChannelKind.DRUMS.label,
            "M4 10a8 3 0 1 0 16 0a8 3 0 1 0-16 0",
            "M4 10v7a8 3 0 0 0 16 0v-7",
            "M8 2l3 5M16 2l-3 5",
        )
    }

    /** A pair of maracas. */
    val Percussion: ImageVector by lazy {
        lineIcon(
            ChannelKind.PERCUSSION.label,
            "M3 6.5a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0-7 0",
            "M14 6.5a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0-7 0",
            "M8.5 9.5L12.5 21M15.5 9.5L11.5 21",
        )
    }

    /** A trumpet: mouthpiece, valves and bell. */
    val Horns: ImageVector by lazy {
        lineIcon(ChannelKind.HORNS.label, "M3 10v4M3 12h12", "M7 12V8M10 12V8", "M15 9l6-3v12l-6-3z")
    }

    /** A violin's body, neck and bridge. */
    val Strings: ImageVector by lazy {
        lineIcon(
            ChannelKind.STRINGS.label,
            "M9 9a3 3 0 0 1 6 0c0 1-1 1.5-1 3s2 1.5 2 4a4 4 0 0 1-8 0c0-2.5 2-2.5 2-4s-1-2-1-3z",
            "M12 2v4M10.5 16h3",
        )
    }

    /** A play triangle: tracks played back. */
    val Playback: ImageVector by lazy { lineIcon(ChannelKind.PLAYBACK.label, "M8 5v14l11-7z") }

    /** A metronome with its pendulum. */
    val Click: ImageVector by lazy { lineIcon(ChannelKind.CLICK.label, "M7 21h10L14.5 3h-5z", "M12 17l4-9") }

    /** A waveform: any other source. */
    val Other: ImageVector by lazy {
        lineIcon(ChannelKind.OTHER.label, "M3 12h2l2-6l3 12l3-9l2 6l2-3h4")
    }
}

/** The line icon of what a channel carries. */
fun kindIcon(kind: ChannelKind): ImageVector = when (kind) {
    ChannelKind.VOICE -> KindIcons.Voice
    ChannelKind.GUITAR -> KindIcons.Guitar
    ChannelKind.BASS -> KindIcons.Bass
    ChannelKind.KEYS -> KindIcons.Keys
    ChannelKind.DRUMS -> KindIcons.Drums
    ChannelKind.PERCUSSION -> KindIcons.Percussion
    ChannelKind.HORNS -> KindIcons.Horns
    ChannelKind.STRINGS -> KindIcons.Strings
    ChannelKind.PLAYBACK -> KindIcons.Playback
    ChannelKind.CLICK -> KindIcons.Click
    ChannelKind.OTHER -> KindIcons.Other
}
