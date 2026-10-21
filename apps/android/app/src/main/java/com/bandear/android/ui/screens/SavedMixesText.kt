package com.bandear.android.ui.screens

/** The small capitals over the list of saved mixes, with how many there are. */
fun savedMixesTitle(count: Int): String = "YOUR MIXES · $count"

/** The line under a saved mix's name: only the active one has one, and it says so in words. */
fun savedMixDetail(active: Boolean): String? = if (active) "Active now" else null

/**
 * What the save field keeps after Save: nothing once the mix was [saved], and everything [typed]
 * when the save was refused, so the name can be corrected instead of typed again.
 */
fun nameAfterSave(typed: String, saved: Boolean): String = if (saved) "" else typed

/**
 * The name a rename confirms: the new name without the spaces around it, or null while it is blank
 * or the same as the old one.
 */
fun renameTarget(from: String, to: String): String? = to.trim().takeIf { it.isNotEmpty() && it != from }

/** A rename can be confirmed once the new name is not blank and differs from the old one. */
fun canRename(from: String, to: String): Boolean = renameTarget(from, to) != null
