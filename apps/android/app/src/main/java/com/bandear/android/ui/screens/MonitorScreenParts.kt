package com.bandear.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/*
 * The pieces the Connect, Saved mixes and Settings screens share, so the three read as one app in
 * both of their layouts.
 */

/**
 * A screen's title, such as "Saved mixes": large in the portrait layout, where it heads a tall
 * column, and the plain title size in landscape, where height is short.
 */
@Composable
internal fun ScreenHeading(text: String, landscape: Boolean, modifier: Modifier = Modifier) {
    val style = MaterialTheme.typography.titleLarge
    Text(
        text = text,
        modifier = modifier,
        style = if (landscape) style else style.copy(fontSize = 28.sp, lineHeight = 34.sp, letterSpacing = (-0.02).em),
        color = BandEarTheme.colors.text,
    )
}

/**
 * A rounded surface holding a column of rows, such as the saved mixes or the manual connection's
 * fields; rows inside it are split by [GroupedCardDivider]. The landscape boards outline it, where
 * it sits beside another column.
 */
@Composable
internal fun GroupedCard(outlined: Boolean, modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(BandEarShapes.Panel)
            .background(colors.surface)
            .then(if (outlined) Modifier.border(1.dp, colors.border, BandEarShapes.Panel) else Modifier),
        content = content,
    )
}

/** The hairline between two rows of a [GroupedCard], inset from the card's edges. */
@Composable
internal fun GroupedCardDivider() {
    Box(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp)
            .height(1.dp)
            .background(BandEarTheme.colors.border),
    )
}
