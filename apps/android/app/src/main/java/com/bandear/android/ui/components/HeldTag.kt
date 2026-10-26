package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/**
 * Why a strip takes no input, as an [icon] and [text] in words ("Locked" with [LineIcons.Lock],
 * "Following" with [LineIcons.Follow]), so it is never told by the strip's fade alone.
 *
 * It takes the place of the strip's keys: drawn 28 dp tall in the middle of a
 * [StripKeySlotHeight] slot and as wide as the slot, so swapping the keys for the tag moves
 * nothing else on the strip. A screen reader reads [text]; the icon adds nothing to it.
 */
@Composable
fun HeldTag(icon: ImageVector, text: String, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(StripKeySlotHeight),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(StripKeyHeight)
                .background(colors.surfaceRaised, BandEarShapes.Tag)
                .padding(horizontal = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp, alignment = Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, tint = colors.text, modifier = Modifier.size(12.dp))
            Text(
                text = text,
                style = MaterialTheme.typography.labelSmall,
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun HeldTagPreview() {
    BandEarTheme {
        Column(modifier = Modifier.width(80.dp).padding(vertical = 8.dp)) {
            HeldTag(LineIcons.Lock, "Locked")
            HeldTag(LineIcons.Follow, "Following")
        }
    }
}
