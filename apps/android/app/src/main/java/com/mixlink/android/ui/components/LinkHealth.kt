package com.mixlink.android.ui.components

import com.mixlink.android.ui.theme.ColorToken

/**
 * How the audio link to this phone is doing, with the same levels and words as the desktop:
 * the server classifies a link as good, warning or bad; a bad link that receives no packets is
 * silent, one whose reports stopped is lost, and a link with no report yet is unknown.
 */
enum class LinkHealth { GOOD, WARNING, BAD, SILENT, LOST, UNKNOWN }

/** The short word for [health], as the desktop shows it. */
fun linkHealthWords(health: LinkHealth): String = when (health) {
    LinkHealth.GOOD -> "Good"
    LinkHealth.WARNING -> "Warning"
    LinkHealth.BAD -> "Bad"
    LinkHealth.SILENT -> "Silent"
    LinkHealth.LOST -> "Lost"
    LinkHealth.UNKNOWN -> "Unknown"
}

/** What [health] means for the musician, in one short sentence. */
fun linkHealthMeaning(health: LinkHealth): String = when (health) {
    LinkHealth.GOOD -> "Clean audio."
    LinkHealth.WARNING -> "An underrun or a little loss."
    LinkHealth.BAD -> "Heavy loss or dropouts."
    LinkHealth.SILENT -> "No audio received."
    LinkHealth.LOST -> "Reports stopped arriving."
    LinkHealth.UNKNOWN -> "No report yet."
}

/** The colour of the link-health dot: green, amber, red for every kind of bad, grey for unknown. */
fun linkHealthToken(health: LinkHealth): ColorToken = when (health) {
    LinkHealth.GOOD -> ColorToken.GREEN
    LinkHealth.WARNING -> ColorToken.AMBER
    LinkHealth.BAD, LinkHealth.SILENT, LinkHealth.LOST -> ColorToken.RED
    LinkHealth.UNKNOWN -> ColorToken.NEUTRAL
}

/** What a screen reader says for the dot: the word and its meaning, never the colour. */
fun linkHealthDescription(health: LinkHealth): String =
    "Link health: ${linkHealthWords(health)}. ${linkHealthMeaning(health)}"
