package com.bandear.android.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.DangerButton
import com.bandear.android.ui.components.FieldLabel
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.BandEarTextField
import com.bandear.android.ui.components.PrimaryButton
import com.bandear.android.ui.components.SecondaryButton
import com.bandear.android.ui.components.isLandscapeLayout
import com.bandear.android.ui.state.SavedMixUiState
import com.bandear.android.ui.state.SavedMixesUiState
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/**
 * The saved mixes: save the current mix under a name, bring one back with Recall. Rename and
 * delete sit behind each mix's menu, and delete asks first, so a stray tap on stage never loses a
 * mix. With nothing saved the list says so, and only then.
 *
 * In portrait it is one column; in [landscape] the title and the save field sit beside the list,
 * and each side scrolls on its own when it does not fit.
 */
@Composable
fun SavedMixesScreen(
    savedMixes: SavedMixesUiState,
    onSave: (String) -> Boolean,
    onRecall: (String) -> Unit,
    onRename: (from: String, to: String) -> Unit,
    onDelete: (String) -> Unit,
    modifier: Modifier = Modifier,
    landscape: Boolean = isLandscapeLayout(),
) {
    val colors = BandEarTheme.colors
    var renaming by rememberSaveable { mutableStateOf<String?>(null) }
    var deleting by rememberSaveable { mutableStateOf<String?>(null) }
    // Kept here, above both layouts, so a name being typed outlasts the phone turning.
    var draftName by rememberSaveable { mutableStateOf("") }
    val mixes = remember(savedMixes) { savedMixes.mixes }

    val header = @Composable {
        Column(verticalArrangement = Arrangement.spacedBy(if (landscape) 6.dp else 8.dp)) {
            ScreenHeading("Saved mixes", landscape)
            Text(
                text = "Store your mix for a song or a set and bring it back with one tap.",
                style = MaterialTheme.typography.bodyMedium,
                color = colors.textSecondary,
            )
        }
    }
    val saveSection = @Composable {
        SaveSection(
            name = draftName,
            onNameChange = { draftName = it },
            onSave = onSave,
            notice = savedMixes.notice,
            landscape = landscape,
        )
    }
    val listSection = @Composable {
        MixesList(
            mixes = mixes,
            emptyMessage = savedMixes.emptyMessage,
            landscape = landscape,
            onRecall = onRecall,
            onRename = { renaming = it },
            onDelete = { deleting = it },
        )
    }

    if (landscape) {
        Row(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .padding(horizontal = 24.dp),
            horizontalArrangement = Arrangement.spacedBy(32.dp),
        ) {
            Column(
                modifier = Modifier
                    .weight(4f)
                    .fillMaxHeight()
                    .verticalScroll(rememberScrollState())
                    .padding(top = 22.dp, bottom = 20.dp),
                verticalArrangement = Arrangement.spacedBy(24.dp),
            ) {
                header()
                saveSection()
            }
            Column(
                modifier = Modifier
                    .weight(5f)
                    .fillMaxHeight()
                    .verticalScroll(rememberScrollState())
                    .padding(top = 30.dp, bottom = 20.dp),
            ) {
                listSection()
            }
        }
    } else {
        Column(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .verticalScroll(rememberScrollState())
                .padding(start = 20.dp, end = 20.dp, top = 28.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            header()
            saveSection()
            listSection()
        }
    }

    renaming?.let { from ->
        RenameDialog(
            from = from,
            onRename = { to ->
                renaming = null
                onRename(from, to)
            },
            onDismiss = { renaming = null },
        )
    }
    deleting?.let { name ->
        DeleteDialog(
            name = name,
            onDelete = {
                deleting = null
                onDelete(name)
            },
            onDismiss = { deleting = null },
        )
    }
}

/**
 * The name field and Save, with the notice of the last save, recall, rename or delete under them.
 * [onSave] says whether the mix was saved: only then is the field cleared, so a refused name stays
 * to be corrected.
 */
@Composable
private fun SaveSection(
    name: String,
    onNameChange: (String) -> Unit,
    onSave: (String) -> Boolean,
    notice: String?,
    landscape: Boolean,
) {
    val colors = BandEarTheme.colors
    val focusManager = LocalFocusManager.current
    val height = if (landscape) 48.dp else 52.dp

    fun save() {
        val saved = onSave(name)
        // Leave the field first, so it shows the cleared text instead of keeping what was typed.
        if (saved) focusManager.clearFocus()
        onNameChange(nameAfterSave(name, saved))
    }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        FieldLabel("SAVE THE CURRENT MIX")
        Row(horizontalArrangement = Arrangement.spacedBy(if (landscape) 8.dp else 10.dp)) {
            BandEarTextField(
                value = name,
                onValueChange = onNameChange,
                label = "Name for the current mix",
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = height),
                placeholder = "Name, e.g. Ballads",
                capitalization = KeyboardCapitalization.Sentences,
                onDone = ::save,
            )
            PrimaryButton("Save", onClick = ::save, modifier = Modifier.heightIn(min = height))
        }
        if (notice != null) {
            Text(
                text = notice,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                color = colors.text,
            )
        }
    }
}

/** "YOUR MIXES · N" over the saved mixes in one card, or over the empty state when there are none. */
@Composable
private fun MixesList(
    mixes: List<SavedMixUiState>,
    emptyMessage: String?,
    landscape: Boolean,
    onRecall: (String) -> Unit,
    onRename: (String) -> Unit,
    onDelete: (String) -> Unit,
) {
    val colors = BandEarTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(text = savedMixesTitle(mixes.size), style = BandEarType.Caps, color = colors.textSecondary)
        if (emptyMessage != null) {
            NoMixesYet(emptyMessage)
        } else {
            GroupedCard(outlined = landscape) {
                mixes.forEachIndexed { index, mix ->
                    key(mix.name) {
                        if (index > 0) GroupedCardDivider()
                        SavedMixRow(
                            mix = mix,
                            landscape = landscape,
                            onRecall = { onRecall(mix.name) },
                            onRename = { onRename(mix.name) },
                            onDelete = { onDelete(mix.name) },
                        )
                    }
                }
            }
            Text(
                text = "Rename and delete are under ⋮, so a stray tap on stage never deletes a mix.",
                style = MaterialTheme.typography.bodySmall,
                color = colors.textSecondary,
            )
        }
    }
}

/** One saved mix: its name, "Active now" under the one in use, Recall and the ⋮ key. */
@Composable
private fun SavedMixRow(
    mix: SavedMixUiState,
    landscape: Boolean,
    onRecall: () -> Unit,
    onRename: () -> Unit,
    onDelete: () -> Unit,
) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = mix.name,
                style = MaterialTheme.typography.bodyLarge.copy(
                    fontSize = if (landscape) 15.sp else 16.sp,
                    fontWeight = if (mix.active) FontWeight.SemiBold else FontWeight.Medium,
                ),
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val detail = savedMixDetail(mix.active)
            if (detail != null) {
                Text(
                    text = detail,
                    style = MaterialTheme.typography.bodySmall.copy(fontSize = if (landscape) 12.sp else 13.sp),
                    color = colors.accent,
                )
            }
        }
        SecondaryButton(
            text = "Recall",
            onClick = onRecall,
            modifier = Modifier
                .heightIn(min = 48.dp)
                .semantics { contentDescription = "Recall ${mix.name}" },
        )
        MixMenu(name = mix.name, onRename = onRename, onDelete = onDelete)
    }
}

/** The ⋮ key of a saved mix, with Rename and Delete; neither acts before its dialog is confirmed. */
@Composable
private fun MixMenu(name: String, onRename: () -> Unit, onDelete: () -> Unit) {
    val colors = BandEarTheme.colors
    var open by remember { mutableStateOf(false) }
    Box {
        Box(
            modifier = Modifier
                .size(48.dp)
                .clip(BandEarShapes.Card)
                .clickable(role = Role.Button) { open = true }
                .semantics { contentDescription = "More actions for $name" },
            contentAlignment = Alignment.Center,
        ) {
            Icon(LineIcons.MoreActions, contentDescription = null, tint = colors.textSecondary, modifier = Modifier.size(20.dp))
        }
        DropdownMenu(
            expanded = open,
            onDismissRequest = { open = false },
            containerColor = colors.surfaceRaised,
            tonalElevation = 0.dp,
            border = BorderStroke(1.dp, colors.border),
        ) {
            DropdownMenuItem(
                text = { Text("Rename") },
                onClick = {
                    open = false
                    onRename()
                },
                colors = MenuDefaults.itemColors(textColor = colors.text),
            )
            DropdownMenuItem(
                text = { Text("Delete") },
                onClick = {
                    open = false
                    onDelete()
                },
                colors = MenuDefaults.itemColors(textColor = colors.red),
            )
        }
    }
}

/** Asks for the new name; [onRename] gets it trimmed, and only once it can be confirmed. */
@Composable
private fun RenameDialog(from: String, onRename: (String) -> Unit, onDismiss: () -> Unit) {
    val colors = BandEarTheme.colors
    var to by rememberSaveable(from) { mutableStateOf(from) }

    fun confirm() {
        renameTarget(from, to)?.let(onRename)
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            PrimaryButton("Rename", onClick = ::confirm, enabled = canRename(from, to))
        },
        dismissButton = { SecondaryButton("Cancel", onClick = onDismiss) },
        title = { Text("Rename \"$from\"") },
        text = {
            BandEarTextField(
                value = to,
                onValueChange = { to = it },
                label = "New name",
                modifier = Modifier.fillMaxWidth(),
                fill = colors.ground,
                capitalization = KeyboardCapitalization.Sentences,
                onDone = ::confirm,
            )
        },
        containerColor = colors.surface,
        titleContentColor = colors.text,
        textContentColor = colors.text,
        tonalElevation = 0.dp,
    )
}

@Composable
private fun DeleteDialog(name: String, onDelete: () -> Unit, onDismiss: () -> Unit) {
    val colors = BandEarTheme.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { DangerButton("Delete", onClick = onDelete) },
        dismissButton = { SecondaryButton("Cancel", onClick = onDismiss) },
        title = { Text("Delete \"$name\"?") },
        text = { Text("The mix is removed from this phone. This cannot be undone.") },
        containerColor = colors.surface,
        titleContentColor = colors.text,
        textContentColor = colors.textSecondary,
        tonalElevation = 0.dp,
    )
}

@Composable
private fun NoMixesYet(message: String) {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(16.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(message, style = MaterialTheme.typography.titleSmall.copy(fontSize = 16.sp), color = colors.text)
        Text(
            text = "Save the current mix above, then bring it back here with one tap.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

@Composable
private fun SavedMixesPreview(savedMixes: SavedMixesUiState, landscape: Boolean = false) {
    BandEarTheme {
        SavedMixesScreen(savedMixes, { false }, {}, { _, _ -> }, {}, landscape = landscape)
    }
}

@Preview(
    name = "Saved mixes · three mixes",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesFilledPreview() = SavedMixesPreview(PreviewSamples.connected.savedMixes)

@Preview(
    name = "Saved mixes · three mixes, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesFilledLandscapePreview() = SavedMixesPreview(PreviewSamples.connected.savedMixes, landscape = true)

@Preview(
    name = "Saved mixes · empty",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesEmptyPreview() = SavedMixesPreview(PreviewSamples.emptyMixes.savedMixes)

@Preview(
    name = "Saved mixes · empty, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesEmptyLandscapePreview() = SavedMixesPreview(PreviewSamples.emptyMixes.savedMixes, landscape = true)
