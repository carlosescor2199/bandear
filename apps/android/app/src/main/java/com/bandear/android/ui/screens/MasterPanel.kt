package com.bandear.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.HorizontalFader
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.ModalPanel
import com.bandear.android.ui.components.SecondaryButton
import com.bandear.android.ui.state.MixUiState
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/**
 * The master's panel, opened from the level-cap button at the foot of the master strip: the level
 * cap that protects the musician's ears, and the engineer's limit when there is one. It opens in
 * the channel panel's container ([ModalPanel]) and closes the same ways, Done included, each of
 * which ends in [onDismiss]; the caller keeps whether it is open, so it stays open across a
 * rotation.
 */
@Composable
internal fun MasterPanel(mix: MixUiState, onLevelCap: (Int) -> Unit, onDismiss: () -> Unit) {
    ModalPanel(onDismiss = onDismiss) { dismiss ->
        MasterPanelContent(mix = mix, onLevelCap = onLevelCap, onDone = dismiss)
    }
}

/** What the master's panel holds, apart from the panel itself, so previews can show it. */
@Composable
internal fun MasterPanelContent(
    mix: MixUiState,
    onLevelCap: (Int) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        Text(
            text = "MASTER",
            modifier = Modifier.semantics { heading() },
            style = BandEarType.Caps,
            color = colors.textSecondary,
        )
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Level cap ", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
                Text("${mix.levelCap}%", style = BandEarType.NumberSmall.copy(fontSize = 13.sp), color = colors.text)
                Text(" · protects your ears", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
            }
            HorizontalFader(
                value = mix.levelCap,
                onValueChange = onLevelCap,
                accent = colors.faderIdle,
                contentDescription = levelCapDescription(mix.levelCap, mix.engineer.limitPercent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        // The engineer's limit sits apart from the phone's cap, which still works: the musician hears
        // the lower of the two, and the line says how much that is.
        val limit = engineerLimitText(mix.levelCap, mix.engineer.limitPercent)
        if (limit != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(LineIcons.Lock, contentDescription = null, tint = colors.textSecondary, modifier = Modifier.size(14.dp))
                Text(limit, style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium), color = colors.text)
            }
        }
        SecondaryButton(text = "Done", onClick = onDone, modifier = Modifier.fillMaxWidth())
    }
}

/** The bottom sheet's top corners, for the preview's stand-in for the sheet. */
private val PreviewSheetShape = RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp)

/** The panel as it rests over the Mix screen; a preview cannot show the real sheet's window. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MasterPanelPreview(mix: MixUiState) {
    BandEarTheme {
        val colors = BandEarTheme.colors
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(colors.ground),
            contentAlignment = Alignment.BottomCenter,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(PreviewSheetShape)
                    .background(colors.surface),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                BottomSheetDefaults.DragHandle(color = colors.textSecondary)
                MasterPanelContent(mix = mix, onLevelCap = {}, onDone = {})
            }
        }
    }
}

private const val MASTER_PREVIEW_HEIGHT_DP = 360

@Preview(
    name = "Master panel · no limit",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = MASTER_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MasterPanelNoLimitPreview() = MasterPanelPreview(PreviewSamples.connected.mix)

@Preview(
    name = "Master panel · limited by the engineer",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = MASTER_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MasterPanelLimitedPreview() = MasterPanelPreview(PreviewSamples.console.mix)
