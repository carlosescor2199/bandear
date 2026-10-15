package com.mixlink.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import com.mixlink.android.ui.theme.MixLinkTheme

private val LinkDotSize = 10.dp

/**
 * The link-health light, the same as the desktop's. A screen reader hears the level and what it
 * means. Alone it is a colour, so wherever it is the only sign of the level, pass [showWords] to
 * write the level beside it.
 */
@Composable
fun LinkDot(health: LinkHealth, modifier: Modifier = Modifier, showWords: Boolean = false) {
    val colors = MixLinkTheme.colors
    val description = linkHealthDescription(health)
    Row(
        modifier = modifier.clearAndSetSemantics { contentDescription = description },
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(LinkDotSize)
                .background(colors.solid(linkHealthToken(health)), CircleShape),
        )
        if (showWords) {
            Text(
                text = linkHealthWords(health),
                style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
            )
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun LinkDotPreview() {
    MixLinkTheme {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            LinkDot(LinkHealth.GOOD)
            LinkHealth.entries.forEach { LinkDot(it, showWords = true) }
        }
    }
}
