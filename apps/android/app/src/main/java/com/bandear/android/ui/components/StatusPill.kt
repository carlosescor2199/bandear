package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

private val PillMinHeight = 44.dp
private val StatusDotSize = 10.dp

/**
 * The connection status at the top of the Mix screen: a coloured dot and the state in words, so
 * the state is never told by colour alone. At least 44 dp tall, so a later screen can make it a
 * touch target without resizing it.
 */
@Composable
fun StatusPill(kind: PillKind, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    val token = pillToken(kind)
    val words = pillWords(kind)
    Row(
        modifier = modifier
            .heightIn(min = PillMinHeight)
            .border(1.dp, colors.tintBorder(token), BandEarShapes.Pill)
            .background(colors.tint(token), BandEarShapes.Pill)
            .padding(horizontal = 14.dp)
            .clearAndSetSemantics { contentDescription = "Connection status: $words" },
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(StatusDotSize)
                .background(colors.solid(token), CircleShape),
        )
        Text(
            text = words,
            style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.Medium),
            color = colors.text,
        )
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun StatusPillPreview() {
    BandEarTheme {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            PillKind.entries.forEach { StatusPill(it) }
        }
    }
}
