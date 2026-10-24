package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/*
 * Previews of the console atoms put together as the Mix boards draw them, in both layouts: the
 * strips (keys or held tag, pan, fader, value, scribble strip), the More of me switch, and the
 * bottom bar in portrait or the rail in landscape. The Mix screen itself assembles them.
 */

/** One preview strip's channel. */
private class PreviewChannel(
    val label: String,
    val subtitle: String,
    val tone: ScribbleTone,
    val level: Int,
    val pan: Int,
    val mine: Boolean = false,
    val muted: Boolean = false,
    val locked: Boolean = false,
    val stereo: Boolean = false,
)

private val PreviewChannels = listOf(
    PreviewChannel("Kick", "Drums", scribbleToneFor(mine = false, groupIndex = 0), level = 75, pan = PAN_CENTRE, locked = true),
    PreviewChannel("Bass", "No group", ScribbleTone.NoGroup, level = 64, pan = 40),
    PreviewChannel("Keys", "Band", scribbleToneFor(mine = false, groupIndex = 1), level = 58, pan = PAN_CENTRE, stereo = true),
    PreviewChannel("Lead vox", "My channel", ScribbleTone.Mine, level = 100, pan = PAN_CENTRE, mine = true),
    PreviewChannel("Click", "No group", ScribbleTone.NoGroup, level = 45, pan = 70, muted = true),
)

@Composable
private fun PreviewStrip(channel: PreviewChannel, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    val fill = when {
        channel.muted -> colors.faderMuted
        channel.mine -> colors.accent
        else -> colors.faderIdle
    }
    Column(
        modifier = modifier
            .fillMaxHeight()
            .background(if (channel.mine) colors.surfaceSelected else colors.surface, BandEarShapes.Panel)
            .border(1.dp, if (channel.mine) colors.accent else Color.Transparent, BandEarShapes.Panel)
            .padding(start = 6.dp, end = 6.dp, bottom = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (channel.locked) {
            HeldTag(LineIcons.Lock, "Locked")
        } else {
            StripKeys(channel.muted, soloed = false, onMute = {}, onSolo = {}, channelLabel = channel.label)
        }
        Column(
            modifier = Modifier
                .weight(1f)
                .alpha(if (channel.locked) 0.4f else 1f),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            StripPan(
                pan = channel.pan,
                onPanChange = {},
                contentDescription = "${channel.label} pan",
                enabled = !channel.locked,
                dotColor = if (channel.mine) colors.accent else colors.textSecondary,
            )
            VerticalFader(
                value = channel.level,
                onValueChange = {},
                accent = fill,
                contentDescription = "${channel.label} level",
                modifier = Modifier
                    .weight(1f)
                    .padding(vertical = 6.dp),
                enabled = !channel.locked,
            )
            Text(
                text = if (channel.muted) "Muted" else "${channel.level}%",
                style = BandEarType.Number.copy(fontSize = 13.sp),
                color = when {
                    channel.muted -> colors.red
                    channel.mine -> colors.accent
                    else -> colors.text
                },
            )
            ScribbleStrip(
                title = channel.label,
                subtitle = channel.subtitle,
                tone = channel.tone,
                contentDescription = channel.label,
                onClick = {},
                stereo = channel.stereo,
            )
        }
    }
}

@Composable
private fun PreviewMaster(modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxHeight()
            .background(colors.surface, BandEarShapes.Panel)
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("MASTER", style = BandEarType.Caps.copy(fontSize = 11.sp), color = colors.textSecondary)
        VerticalFader(
            value = 80,
            onValueChange = {},
            accent = colors.textSecondary,
            contentDescription = "Master level",
            modifier = Modifier
                .weight(1f)
                .padding(vertical = 6.dp),
            limitMarker = 85,
        )
        Text("80%", style = BandEarType.Number.copy(fontSize = 13.sp), color = colors.text)
    }
}

@Composable
private fun RowScope.PreviewConsole() {
    PreviewChannels.forEach { PreviewStrip(it, Modifier.weight(1f)) }
    PreviewMaster(Modifier.width(96.dp))
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 390, heightDp = 844)
@Composable
private fun ConsolePortraitPreview() {
    BandEarTheme {
        Column(Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 12.dp, end = 12.dp, top = 48.dp, bottom = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                MoreOfMeSwitch(on = true, channelLabel = "Lead vox", onToggle = {}, modifier = Modifier.fillMaxWidth())
                Row(
                    modifier = Modifier.weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    PreviewStrip(PreviewChannels[1], Modifier.width(80.dp))
                    PreviewStrip(PreviewChannels[3], Modifier.width(80.dp))
                    PreviewStrip(PreviewChannels[4], Modifier.weight(1f))
                    PreviewMaster(Modifier.width(96.dp))
                }
            }
            BottomTabBar(selected = MainTab.MIX, onSelect = {})
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 844, heightDp = 390)
@Composable
private fun ConsoleLandscapePreview() {
    BandEarTheme {
        Row(Modifier.fillMaxSize()) {
            NavigationRail(selected = MainTab.MIX, onSelect = {})
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    MoreOfMeSwitch(on = true, channelLabel = "Lead vox", onToggle = {})
                }
                Row(
                    modifier = Modifier.weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    PreviewConsole()
                }
            }
        }
    }
}
