package com.mixlink.android.ui.theme

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.ui.unit.dp

/** The corner radii of the MixLink look, from the smallest tag to the round status pill. */
object MixLinkShapes {
    /** Group chips and the fader thumb. */
    val Tag = RoundedCornerShape(6.dp)

    /** The mute and solo keys on a channel strip. */
    val Key = RoundedCornerShape(8.dp)

    /** Buttons and segmented controls. */
    val Control = RoundedCornerShape(10.dp)

    /** Cards in a list, such as a saved mix or a legend tile. */
    val Card = RoundedCornerShape(12.dp)

    /** Channel strips, the master card and the More of me toggle. */
    val Panel = RoundedCornerShape(14.dp)

    /** The status pill: half its 44 dp height, so its ends are round. */
    val Pill = RoundedCornerShape(22.dp)
}

/** Material's shape roles mapped onto the MixLink radii. */
val MixLinkMaterialShapes = Shapes(
    extraSmall = MixLinkShapes.Tag,
    small = MixLinkShapes.Control,
    medium = MixLinkShapes.Card,
    large = MixLinkShapes.Panel,
    extraLarge = MixLinkShapes.Pill,
)
