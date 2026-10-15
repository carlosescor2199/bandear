package com.mixlink.android.ui.screens

/** The small capitals over the list of saved mixes, with how many there are. */
fun savedMixesTitle(count: Int): String = "YOUR MIXES · $count"

/** The line under a saved mix's name: only the active one has one, and it says so in words. */
fun savedMixDetail(active: Boolean): String? = if (active) "Active now" else null

/** A rename can be confirmed once the new name is not blank and differs from the old one. */
fun canRename(from: String, to: String): Boolean {
    val target = to.trim()
    return target.isNotEmpty() && target != from
}
