package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.ColorToken
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/**
 * A tinted panel that says what is going on in words: the reconnecting banner on the Mix screen, a
 * refused connect. [busy] turns the dot into a spinner for something that is still under way. A
 * screen reader announces it when it appears or changes.
 */
@Composable
fun StatusBanner(
    token: ColorToken,
    title: String,
    modifier: Modifier = Modifier,
    detail: String? = null,
    busy: Boolean = false,
) {
    val colors = BandEarTheme.colors
    Row(
        modifier = modifier
            .clip(BandEarShapes.Panel)
            .background(colors.tint(token))
            .border(1.dp, colors.tintBorder(token), BandEarShapes.Panel)
            .padding(horizontal = 16.dp, vertical = 14.dp)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) {
            CircularProgressIndicator(
                modifier = Modifier.size(20.dp),
                color = colors.solid(token),
                strokeWidth = 2.dp,
                trackColor = Color.Transparent,
            )
        } else {
            Box(
                Modifier
                    .size(10.dp)
                    .background(colors.solid(token), CircleShape),
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = title,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
            )
            if (detail != null) {
                Text(text = detail, style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
            }
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun StatusBannerPreview() {
    BandEarTheme {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            StatusBanner(
                token = ColorToken.AMBER,
                title = "Reconnecting to 192.168.1.34:50001",
                detail = "Your mix is held at the server. It retries on its own.",
                busy = true,
                modifier = Modifier.fillMaxWidth(),
            )
            StatusBanner(
                token = ColorToken.RED,
                title = "Could not connect",
                detail = "Enter a valid UDP port between 1 and 65535",
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
