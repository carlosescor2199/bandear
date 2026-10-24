package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarTheme

/** The three places of the connected app, in the order of the bottom bar. */
enum class MainTab(val label: String) {
    MIX("Mix"),
    MIXES("Mixes"),
    SETTINGS("Settings"),
}

/**
 * The bottom bar that switches between [MainTab]s in the portrait layout ([NavigationRail] takes
 * its place in landscape). It sits on the bar colour under a hairline; the selected tab is amber,
 * bolder and marked with a bar above its icon, so it is not told by colour alone; a screen reader
 * hears each as a tab and which one is selected.
 *
 * Its colour reaches under the navigation bar, and it keeps its tabs clear of it and of the
 * sides' cutouts: it pads for the bottom and side safe-drawing insets itself, unless something
 * around it already did.
 */
@Composable
fun BottomTabBar(selected: MainTab, onSelect: (MainTab) -> Unit, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Column(modifier = modifier.fillMaxWidth().background(colors.bar)) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(1.dp)
                .background(colors.border),
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Bottom + WindowInsetsSides.Horizontal))
                .padding(start = 12.dp, end = 12.dp, top = 6.dp, bottom = 8.dp)
                .selectableGroup(),
        ) {
            MainTab.entries.forEach { tab ->
                TabItem(
                    tab = tab,
                    selected = tab == selected,
                    onClick = { onSelect(tab) },
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun TabItem(tab: MainTab, selected: Boolean, onClick: () -> Unit, modifier: Modifier) {
    val colors = BandEarTheme.colors
    val tint = if (selected) colors.accent else colors.textSecondary
    Column(
        modifier = modifier
            .heightIn(min = 56.dp)
            .selectable(selected = selected, role = Role.Tab, onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, alignment = Alignment.CenterVertically),
    ) {
        Box(
            Modifier
                .size(width = 20.dp, height = 3.dp)
                .background(if (selected) colors.accent else Color.Transparent, RoundedCornerShape(2.dp)),
        )
        Icon(imageVector = tabIcon(tab), contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
        Text(
            text = tab.label,
            style = MaterialTheme.typography.labelMedium.copy(
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
            ),
            color = tint,
        )
    }
}

/** The line icon of [tab], the same in the bottom bar and the rail. */
internal fun tabIcon(tab: MainTab): ImageVector = when (tab) {
    MainTab.MIX -> MixIcon
    MainTab.MIXES -> MixesIcon
    MainTab.SETTINGS -> SettingsIcon
}

/** Three faders at different heights. */
private val MixIcon: ImageVector by lazy {
    lineIcon("Mix", "M6 4v16M12 4v16M18 4v16", "M4 14h4M10 8h4M16 12h4")
}

/** A storage card: a saved mix. */
private val MixesIcon: ImageVector by lazy {
    lineIcon("Mixes", "M5 4h11l3 3v13H5z", "M8 4v5h7V4M8 20v-6h8v6")
}

/** A hub with spokes: settings. */
private val SettingsIcon: ImageVector by lazy {
    lineIcon(
        "Settings",
        "M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0",
        "M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1",
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 390)
@Composable
private fun BottomTabBarPreview() {
    BandEarTheme {
        var selected by remember { mutableStateOf(MainTab.MIX) }
        BottomTabBar(selected = selected, onSelect = { selected = it })
    }
}
