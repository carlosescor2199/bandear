package com.mixlink.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme

/** The one main action of a screen or card, such as Connect or Save: amber, 48 dp tall. */
@Composable
fun PrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = MixLinkTheme.colors
    Box(
        modifier = modifier
            .heightIn(min = 48.dp)
            .clip(MixLinkShapes.Control)
            .background(if (enabled) colors.accent else colors.surfaceRaised)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 18.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelLarge.copy(fontSize = 15.sp),
            color = if (enabled) colors.onAccent else colors.textSecondary,
        )
    }
}

/** Any other action, such as Recall or Disconnect: a raised outline, 44 dp tall. */
@Composable
fun SecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = MixLinkTheme.colors
    Box(
        modifier = modifier
            .heightIn(min = 44.dp)
            .clip(MixLinkShapes.Control)
            .background(colors.surfaceRaised)
            .border(1.dp, colors.border, MixLinkShapes.Control)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelLarge,
            color = if (enabled) colors.text else colors.textSecondary,
        )
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun ButtonsPreview() {
    MixLinkTheme {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.Start,
        ) {
            PrimaryButton("Save", onClick = {})
            PrimaryButton("Connect", onClick = {}, enabled = false)
            SecondaryButton("Recall", onClick = {})
            SecondaryButton("Disconnect", onClick = {}, enabled = false)
        }
    }
}
