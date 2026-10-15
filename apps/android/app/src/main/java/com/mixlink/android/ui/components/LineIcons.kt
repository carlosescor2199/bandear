package com.mixlink.android.ui.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** The line icons of the MixLink look, drawn on a 24-unit grid; [androidx.compose.material3.Icon] tints them. */
object LineIcons {
    /** A screen on a stand: a MixLink server. */
    val Server: ImageVector by lazy {
        lineIcon(
            "Server",
            "M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
            "M8 20h8M12 16v4",
        )
    }

    /** Three lines getting shorter: typing an address. */
    val Form: ImageVector by lazy { lineIcon("Form", "M4 7h16M4 12h10M4 17h7") }

    /** Three dots in a column: more actions. Each dot is a tiny ring drawn thick enough to fill. */
    val MoreActions: ImageVector by lazy {
        lineIcon(
            "More actions",
            "M12 4a1 1 0 1 0 0 2a1 1 0 1 0 0-2",
            "M12 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2",
            "M12 18a1 1 0 1 0 0 2a1 1 0 1 0 0-2",
        )
    }

    /** A chevron pointing down: this opens a list. */
    val ChevronDown: ImageVector by lazy { lineIcon("Open", "M6 9l6 6l6-6") }

    /** A chevron pointing left: back. */
    val Back: ImageVector by lazy { lineIcon("Back", "M15 18l-6-6l6-6") }

    /** A tick: the chosen item of a list. */
    val Check: ImageVector by lazy { lineIcon("Check", "M5 12l5 5L20 7") }
}

/** A 24-unit line icon drawn with round two-unit strokes, one stroke per path. */
internal fun lineIcon(name: String, vararg paths: String): ImageVector =
    ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        paths.forEach { path ->
            addPath(
                pathData = addPathNodes(path),
                stroke = SolidColor(Color.Black),
                strokeLineWidth = 2f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()
