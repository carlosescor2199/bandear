package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Icon
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.ColorToken

/** Each rail item is at least this big, icon over label. */
private val RailItemMinWidth = 60.dp
private val RailItemMinHeight = 64.dp

/**
 * The rail that switches between [MainTab]s in the landscape layout, down the start side of the
 * screen where [BottomTabBar] sits along the bottom in portrait: the same three tabs, centred
 * one above the other on the bar colour, with a hairline on the side facing the screen.
 *
 * The selected tab is amber on the amber tint, so it is told by its filled background as well as
 * by its colour, and its label is bolder; a screen reader hears each as a tab and which one is
 * selected. Each item is at least 60 by 64 dp.
 *
 * In landscape the display cutout or the navigation bar can sit on the start side: the rail's
 * colour reaches under them, and it keeps its tabs clear by padding for the start and vertical
 * safe-drawing insets itself, so the screen beside it pads only for its other sides.
 */
@Composable
fun NavigationRail(selected: MainTab, onSelect: (MainTab) -> Unit, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Row(modifier = modifier.fillMaxHeight().background(colors.bar)) {
        Column(
            modifier = Modifier
                .fillMaxHeight()
                .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Start + WindowInsetsSides.Vertical))
                .padding(horizontal = 4.dp)
                .selectableGroup(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp, alignment = Alignment.CenterVertically),
        ) {
            MainTab.entries.forEach { tab ->
                RailItem(tab = tab, selected = tab == selected, onClick = { onSelect(tab) })
            }
        }
        Box(
            Modifier
                .fillMaxHeight()
                .width(1.dp)
                .background(colors.border),
        )
    }
}

@Composable
private fun RailItem(tab: MainTab, selected: Boolean, onClick: () -> Unit) {
    val colors = BandEarTheme.colors
    val tint = if (selected) colors.accent else colors.textSecondary
    Column(
        modifier = Modifier
            .sizeIn(minWidth = RailItemMinWidth, minHeight = RailItemMinHeight)
            .clip(BandEarShapes.Card)
            .background(if (selected) colors.tint(ColorToken.AMBER) else Color.Transparent)
            .selectable(selected = selected, role = Role.Tab, onClick = onClick)
            .padding(horizontal = 4.dp, vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, alignment = Alignment.CenterVertically),
    ) {
        Icon(imageVector = tabIcon(tab), contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
        Text(
            text = tab.label,
            style = MaterialTheme.typography.labelSmall.copy(
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
            ),
            color = tint,
            maxLines = 1,
        )
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, heightDp = 390)
@Composable
private fun NavigationRailPreview() {
    BandEarTheme {
        var selected by remember { mutableStateOf(MainTab.MIX) }
        NavigationRail(selected = selected, onSelect = { selected = it }, modifier = Modifier.height(390.dp))
    }
}
