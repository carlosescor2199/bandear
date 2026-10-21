package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/**
 * The line under "More of me": whether it is on and which channel it lifts. It says "On" or "Off"
 * in words, so the state is never told by the fill colour alone.
 */
fun moreOfMeCaption(on: Boolean, channelLabel: String): String {
    if (!on) return "Off · tap to lift your channel"
    val channel = channelLabel.trim().ifEmpty { "your channel" }
    return "On · $channel lifted"
}

/**
 * The large More of me switch on the Mix screen: amber and filled while it lifts the musician's
 * channel ([channelLabel]), a plain panel while it is off. At least 64 dp tall, so it can be hit
 * without looking.
 */
@Composable
fun MoreOfMeToggle(
    on: Boolean,
    channelLabel: String,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    val content = if (on) colors.onAccent else colors.text
    Column(
        modifier = modifier
            .heightIn(min = 64.dp)
            .clip(BandEarShapes.Panel)
            .background(if (on) colors.accent else colors.surface)
            .border(1.dp, if (on) colors.accent else colors.border, BandEarShapes.Panel)
            .toggleable(value = on, enabled = enabled, role = Role.Switch, onValueChange = onToggle)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp, alignment = Alignment.CenterVertically),
    ) {
        Text(text = "More of me", style = MaterialTheme.typography.titleSmall, color = content)
        Text(
            text = moreOfMeCaption(on, channelLabel),
            style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
            color = content,
        )
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun MoreOfMeTogglePreview() {
    BandEarTheme {
        var on by remember { mutableStateOf(true) }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            MoreOfMeToggle(on, "CH 3", { on = it }, Modifier.fillMaxWidth())
            MoreOfMeToggle(on = false, channelLabel = "CH 3", onToggle = {}, modifier = Modifier.fillMaxWidth())
        }
    }
}
