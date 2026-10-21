package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/**
 * Whether a text field should replace what it shows ([shown]) with the text it was given from
 * outside ([outside]).
 *
 * While the musician types, the outside text is the echo of earlier keys and can lag behind them,
 * so it is ignored until the field loses focus; otherwise a late echo would undo the last key. An
 * outside change while the field is not being edited (a restored name, a form filled by a
 * session) is shown.
 */
fun adoptOutsideText(outside: String, shown: String, focused: Boolean): Boolean = !focused && outside != shown

/**
 * A field's text and its selection, as plain numbers so the editing rules can be tested on the
 * JVM. Equal ends are a cursor; [selectionStart] comes after [selectionEnd] when the selection was
 * made backwards.
 */
data class FieldText(
    val text: String,
    val selectionStart: Int = text.length,
    val selectionEnd: Int = selectionStart,
)

/**
 * What a field limited to [maxLength] characters keeps when an edit turns [shown] into [proposed].
 *
 * An edit that fits, or that does not make the text longer, is kept as it is, so a text that is
 * already too long (given from outside) can still be shortened. Otherwise the text already there
 * is kept and only what the edit inserts is cut to fit, as the old field's length filter did: a
 * name pasted in front of a name keeps the whole name after it. The cut is one character shorter
 * when it would split a character made of two UTF-16 units (an emoji, say), and the cursor goes
 * after the part that was kept. An edit none of whose text fits changes nothing, not even the
 * selection it would have replaced: at the limit, or over it, further keys and pastes do nothing.
 */
fun keptEdit(shown: FieldText, proposed: FieldText, maxLength: Int): FieldText {
    if (proposed.text.length <= maxLength || proposed.text.length <= shown.text.length) return proposed
    val edit = editAtSelection(shown, proposed) ?: editByComparison(shown.text, proposed.text)
    val room = maxLength - (shown.text.length - edit.replacedLength)
    val kept = fitted(edit.inserted, room)
    if (kept.isEmpty()) return shown
    val text = shown.text.substring(0, edit.start) + kept + shown.text.substring(edit.start + edit.replacedLength)
    return FieldText(text, edit.start + kept.length)
}

/** [proposed] as [keptEdit] keeps it, for a plain text whose cursor is at its end. */
fun keptEdit(shown: String, proposed: String, maxLength: Int): String =
    keptEdit(FieldText(shown), FieldText(proposed), maxLength).text

/** An edit of a text: [replacedLength] characters from [start] replaced by [inserted]. */
private class TextEdit(val start: Int, val replacedLength: Int, val inserted: String)

/**
 * The edit as typing and pasting make it: the selection, or nothing at the cursor, replaced by
 * new text. Null when [proposed] is not [shown] with its selection replaced, as after an edit made
 * somewhere else (an autocorrection, say).
 */
private fun editAtSelection(shown: FieldText, proposed: FieldText): TextEdit? {
    val length = shown.text.length
    val start = minOf(shown.selectionStart, shown.selectionEnd).coerceIn(0, length)
    val end = maxOf(shown.selectionStart, shown.selectionEnd).coerceIn(0, length)
    val insertedLength = proposed.text.length - (length - (end - start))
    if (insertedLength < 0) return null
    val keepsStart = proposed.text.regionMatches(0, shown.text, 0, start)
    val keepsEnd = proposed.text.regionMatches(start + insertedLength, shown.text, end, length - end)
    if (!keepsStart || !keepsEnd) return null
    return TextEdit(start, end - start, proposed.text.substring(start, start + insertedLength))
}

/**
 * The edit read from the texts alone: what lies between the start and the end they share. Neither
 * end splits a character made of two UTF-16 units.
 */
private fun editByComparison(shown: String, proposed: String): TextEdit {
    val prefix = shown.commonPrefixWith(proposed).length
    var suffix = shown.commonSuffixWith(proposed).length.coerceAtMost(minOf(shown.length, proposed.length) - prefix)
    if (suffix > 0 && Character.isLowSurrogate(shown[shown.length - suffix])) suffix--
    return TextEdit(prefix, shown.length - prefix - suffix, proposed.substring(prefix, proposed.length - suffix))
}

/** The start of [inserted] that fits in [room] characters without splitting a two-unit character. */
private fun fitted(inserted: String, room: Int): String = when {
    room <= 0 -> ""
    room >= inserted.length -> inserted
    Character.isHighSurrogate(inserted[room - 1]) -> inserted.substring(0, room - 1)
    else -> inserted.substring(0, room)
}

/** [proposed] as [keptEdit] keeps it; an edit kept or refused whole keeps its own cursor. */
private fun keptEdit(shown: TextFieldValue, proposed: TextFieldValue, maxLength: Int): TextFieldValue {
    val shownText = shown.fieldText()
    val proposedText = proposed.fieldText()
    return when (val kept = keptEdit(shownText, proposedText, maxLength)) {
        proposedText -> proposed
        shownText -> shown
        else -> TextFieldValue(kept.text, TextRange(kept.selectionStart, kept.selectionEnd))
    }
}

private fun TextFieldValue.fieldText() = FieldText(text, selection.start, selection.end)

/** How a text field's text is set: in the text face, or in the number face for addresses and ports. */
enum class FieldFace { TEXT, NUMBERS }

/**
 * A single-line text field in the BandEar look: a bordered box that turns amber while it is being
 * edited. [label] is what a screen reader calls it; a visible label goes above it as a [FieldLabel].
 *
 * It keeps what is typed itself and reports every change through [onValueChange], so a caller that
 * echoes the text back later (through a ViewModel's state) never moves the cursor or loses a key.
 * With [maxLength] it takes no more characters than that (see [keptEdit]).
 */
@Composable
fun BandEarTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    face: FieldFace = FieldFace.TEXT,
    large: Boolean = false,
    fill: Color = BandEarTheme.colors.surface,
    keyboardType: KeyboardType = KeyboardType.Text,
    capitalization: KeyboardCapitalization = KeyboardCapitalization.None,
    imeAction: ImeAction = ImeAction.Done,
    onDone: () -> Unit = {},
    enabled: Boolean = true,
    maxLength: Int = Int.MAX_VALUE,
) {
    val colors = BandEarTheme.colors
    var shown by remember { mutableStateOf(TextFieldValue(value, TextRange(value.length))) }
    var focused by remember { mutableStateOf(false) }
    LaunchedEffect(value, focused) {
        if (adoptOutsideText(value, shown.text, focused)) shown = TextFieldValue(value, TextRange(value.length))
    }
    val textStyle = fieldTextStyle(face, large).copy(color = if (enabled) colors.text else colors.textSecondary)
    val shape = if (large) BandEarShapes.Card else BandEarShapes.Control

    BasicTextField(
        value = shown,
        onValueChange = { proposed ->
            val next = keptEdit(shown, proposed, maxLength)
            val changed = next.text != shown.text
            shown = next
            if (changed) onValueChange(next.text)
        },
        modifier = modifier
            .heightIn(min = if (large) 52.dp else 48.dp)
            .onFocusChanged { focused = it.isFocused }
            .semantics { contentDescription = label },
        enabled = enabled,
        singleLine = true,
        textStyle = textStyle,
        keyboardOptions = KeyboardOptions(
            capitalization = capitalization,
            keyboardType = keyboardType,
            imeAction = imeAction,
        ),
        keyboardActions = KeyboardActions(onDone = { onDone() }),
        cursorBrush = SolidColor(colors.accent),
        decorationBox = { innerField ->
            Box(
                modifier = Modifier
                    .background(fill, shape)
                    .border(1.dp, if (focused) colors.accent else colors.border, shape)
                    .padding(horizontal = if (large) 16.dp else 14.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (shown.text.isEmpty() && placeholder.isNotEmpty()) {
                    Text(text = placeholder, style = textStyle, color = colors.textSecondary)
                }
                innerField()
            }
        },
    )
}

@Composable
private fun fieldTextStyle(face: FieldFace, large: Boolean): TextStyle = when (face) {
    FieldFace.TEXT -> MaterialTheme.typography.bodyLarge.copy(fontSize = if (large) 18.sp else 16.sp)
    FieldFace.NUMBERS -> BandEarType.Number.copy(fontWeight = FontWeight.Medium, fontSize = 15.sp)
}

/**
 * The visible label over a field: small capitals ("YOUR NAME") or, inside a card, plain secondary
 * text ("Audio port"). The field itself carries the name for a screen reader, so this is hidden
 * from it rather than read twice.
 */
@Composable
fun FieldLabel(text: String, modifier: Modifier = Modifier, caps: Boolean = true) {
    Text(
        text = text,
        modifier = modifier.clearAndSetSemantics {},
        style = if (caps) BandEarType.Caps else MaterialTheme.typography.bodySmall,
        color = BandEarTheme.colors.textSecondary,
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun BandEarTextFieldPreview() {
    BandEarTheme {
        var name by remember { mutableStateOf("Carlos D") }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FieldLabel("YOUR NAME")
            BandEarTextField(name, { name = it }, "Your name", Modifier.fillMaxWidth(), large = true)
            FieldLabel("Server address", caps = false)
            BandEarTextField(
                value = "",
                onValueChange = {},
                label = "Server address",
                modifier = Modifier.fillMaxWidth(),
                placeholder = "192.168.1.34",
                face = FieldFace.NUMBERS,
                fill = BandEarTheme.colors.ground,
            )
        }
    }
}
