package com.bandear.android.ui.screens

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.R
import com.bandear.android.ui.components.BandEarTextField
import com.bandear.android.ui.components.FieldLabel
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.StatusBanner
import com.bandear.android.ui.components.isLandscapeLayout
import com.bandear.android.ui.components.lineIcon
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.MAX_NAME_LENGTH
import com.bandear.android.ui.state.ServerEndpoint
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType
import com.bandear.android.ui.theme.ColorToken

/**
 * The first screen, shown whenever no session runs: the musician's name and the servers found on
 * the network, each joined with one tap. The manual address is one tap away, in Settings.
 *
 * In portrait it is one column; in [landscape] the brand and the name sit beside the servers, and
 * each side scrolls on its own when it does not fit, the manual address link staying at the foot
 * of the servers' side.
 *
 * [connection] is only read for why the last Connect was refused.
 */
@Composable
fun ConnectScreen(
    musicianName: String,
    servers: List<ServerEndpoint>,
    connection: ConnectionUiState,
    onNameChange: (String) -> Unit,
    onConnect: (ServerEndpoint) -> Unit,
    onManualAddress: () -> Unit,
    modifier: Modifier = Modifier,
    landscape: Boolean = isLandscapeLayout(),
) {
    val colors = BandEarTheme.colors
    if (landscape) {
        Row(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .padding(horizontal = 24.dp),
            horizontalArrangement = Arrangement.spacedBy(40.dp),
        ) {
            Column(
                modifier = Modifier
                    .weight(4f)
                    .fillMaxHeight()
                    .verticalScroll(rememberScrollState())
                    .padding(top = 24.dp, bottom = 20.dp),
                verticalArrangement = Arrangement.spacedBy(28.dp),
            ) {
                BrandHeader()
                NameSection(musicianName, onNameChange)
            }
            Column(
                modifier = Modifier
                    .weight(5f)
                    .fillMaxHeight()
                    .padding(bottom = 20.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                ServersSection(
                    servers = servers,
                    problem = connection.problem,
                    onConnect = onConnect,
                    modifier = Modifier
                        .weight(1f)
                        .verticalScroll(rememberScrollState())
                        .padding(top = 28.dp),
                )
                ManualAddressLink(onManualAddress)
            }
        }
    } else {
        Column(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .verticalScroll(rememberScrollState())
                .padding(start = 20.dp, end = 20.dp, top = 28.dp, bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(36.dp),
        ) {
            BrandHeader()
            NameSection(musicianName, onNameChange)
            ServersSection(servers, connection.problem, onConnect)
            ManualAddressLink(onManualAddress)
        }
    }
}

/** The app's icon, its name and "MONITOR" under it. */
@Composable
private fun BrandHeader() {
    val colors = BandEarTheme.colors
    Row(
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(44.dp)
                .background(colorResource(R.color.bandear_icon_ground), BandEarShapes.Control),
            contentAlignment = Alignment.Center,
        ) {
            Image(
                painter = painterResource(R.drawable.bandear_mark),
                contentDescription = null,
                modifier = Modifier.size(width = 24.dp, height = 31.dp),
            )
        }
        Column {
            Text(text = "BandEar", style = MaterialTheme.typography.titleLarge, color = colors.text)
            Text(text = "MONITOR", style = BandEarType.Caps, color = colors.textSecondary)
        }
    }
}

@Composable
private fun NameSection(musicianName: String, onNameChange: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FieldLabel("YOUR NAME")
        BandEarTextField(
            value = musicianName,
            onValueChange = onNameChange,
            label = "Your name",
            modifier = Modifier.fillMaxWidth(),
            placeholder = "Your name",
            large = true,
            capitalization = KeyboardCapitalization.Words,
            maxLength = MAX_NAME_LENGTH,
        )
        Text(
            text = "The engineer sees this name on the desktop.",
            style = MaterialTheme.typography.bodySmall,
            color = BandEarTheme.colors.textSecondary,
        )
    }
}

@Composable
private fun ServersSection(
    servers: List<ServerEndpoint>,
    problem: String?,
    onConnect: (ServerEndpoint) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("SERVERS ON THIS NETWORK", style = BandEarType.Caps, color = colors.textSecondary)
            Searching()
        }
        if (problem != null) {
            StatusBanner(
                token = ColorToken.RED,
                title = "Could not connect",
                detail = problem,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (servers.isEmpty()) {
            NoServersYet()
        } else {
            servers.forEach { server ->
                key(server) { ServerCard(server, onConnect = { onConnect(server) }) }
            }
        }
        Text(
            text = "Servers appear here on their own. Tap one to join; the phone stays connected and " +
                "reconnects on its own if the Wi-Fi drops.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

/** Discovery keeps listening while this screen shows, found servers or not. */
@Composable
private fun Searching() {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier.semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator(
            modifier = Modifier.size(14.dp),
            color = colors.blue,
            strokeWidth = 2.dp,
            trackColor = Color.Transparent,
        )
        Text("Searching", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
    }
}

/** One found server: the whole card is its single Connect action, the amber key only shows it. */
@Composable
private fun ServerCard(server: ServerEndpoint, onConnect: () -> Unit) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 68.dp)
            .clip(BandEarShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .clickable(role = Role.Button, onClickLabel = "Connect", onClick = onConnect)
            .padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .background(colors.surfaceRaised, BandEarShapes.Control),
            contentAlignment = Alignment.Center,
        ) {
            Icon(LineIcons.Server, contentDescription = null, tint = colors.text, modifier = Modifier.size(20.dp))
        }
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = "BandEar server",
                style = MaterialTheme.typography.titleSmall.copy(fontSize = 16.sp),
                color = colors.text,
            )
            Text(
                text = server.label,
                style = BandEarType.NumberSmall.copy(fontSize = 13.sp),
                color = colors.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Box(
            modifier = Modifier
                .heightIn(min = 44.dp)
                .background(colors.accent, BandEarShapes.Control)
                .padding(horizontal = 18.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("Connect", style = MaterialTheme.typography.labelLarge, color = colors.onAccent)
        }
    }
}

@Composable
private fun NoServersYet() {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 68.dp)
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(horizontal = 16.dp, vertical = 14.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
    ) {
        Text(
            text = "No servers found yet",
            style = MaterialTheme.typography.titleSmall.copy(fontSize = 16.sp),
            color = colors.text,
        )
        Text(
            text = "Check that this phone is on the same Wi-Fi as the BandEar desktop.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

/** A row between two hairlines that opens the manual address form, with a chevron at its end. */
@Composable
private fun ManualAddressLink(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .drawBehind {
                val hairline = 1.dp.toPx()
                drawRect(colors.border, size = Size(size.width, hairline))
                drawRect(colors.border, topLeft = Offset(0f, size.height - hairline), size = Size(size.width, hairline))
            }
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = "Enter the server address manually",
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
            color = colors.text,
        )
        Icon(ChevronEnd, contentDescription = null, tint = colors.neutral, modifier = Modifier.size(18.dp))
    }
}

/** A chevron pointing right: this opens another screen. */
private val ChevronEnd: ImageVector by lazy { lineIcon("Open", "M9 6l6 6l-6 6") }

@Preview(
    name = "Connect · server found",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenFoundPreview() {
    ConnectPreview(PreviewSamples.notConnected.musicianName, PreviewSamples.notConnected.servers, ConnectionUiState())
}

@Preview(
    name = "Connect · server found, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenFoundLandscapePreview() {
    val state = PreviewSamples.notConnected
    ConnectPreview(state.musicianName, state.servers, ConnectionUiState(), landscape = true)
}

@Preview(
    name = "Connect · searching",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenSearchingPreview() {
    ConnectPreview(PreviewSamples.searching.musicianName, PreviewSamples.searching.servers, ConnectionUiState())
}

@Preview(
    name = "Connect · refused",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenRefusedPreview() {
    val state = PreviewSamples.refused
    ConnectPreview(state.musicianName, state.servers, state.connection)
}

@Preview(
    name = "Connect · refused, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenRefusedLandscapePreview() {
    val state = PreviewSamples.refused
    ConnectPreview(state.musicianName, state.servers, state.connection, landscape = true)
}

@Composable
private fun ConnectPreview(
    name: String,
    servers: List<ServerEndpoint>,
    connection: ConnectionUiState,
    landscape: Boolean = false,
) {
    BandEarTheme {
        ConnectScreen(
            musicianName = name,
            servers = servers,
            connection = connection,
            onNameChange = {},
            onConnect = {},
            onManualAddress = {},
            landscape = landscape,
        )
    }
}
