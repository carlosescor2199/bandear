package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.GroupTone
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/**
 * The tone of the group at [index] in the server's group list. Groups take the tones in turn, so
 * neighbouring groups always differ, and an index past the last tone starts over.
 */
fun groupToneFor(index: Int): GroupTone {
    val tones = GroupTone.entries
    return tones[Math.floorMod(index, tones.size)]
}

/** The small tag on a channel strip naming the group the channel belongs to. */
@Composable
fun GroupChip(name: String, tone: GroupTone, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Text(
        text = name,
        modifier = modifier
            .background(colors.groupBackground(tone), BandEarShapes.Tag)
            .padding(horizontal = 8.dp, vertical = 2.dp)
            .clearAndSetSemantics { contentDescription = "Group: $name" },
        style = MaterialTheme.typography.labelSmall,
        color = colors.groupText(tone),
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun GroupChipPreview() {
    BandEarTheme {
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            listOf("Drums", "Keys", "Vocals", "Guitar").forEachIndexed { index, name ->
                GroupChip(name, groupToneFor(index))
            }
        }
    }
}
