package com.bandear.android.ui.components

import kotlin.math.abs
import kotlin.math.roundToInt

/** Hard left: the channel plays only in the left ear. */
const val PAN_LEFT = 0

/** The centre: the channel plays equally in both ears. */
const val PAN_CENTRE = 50

/** Hard right: the channel plays only in the right ear. */
const val PAN_RIGHT = 100

/**
 * How far the pan control's centre detent reaches on each side of the middle, as a share of the
 * thumb's travel: a touch anywhere in the middle tenth of the control reads as the centre, so the
 * centre is easy to find by feel and hard to miss by a hair.
 */
const val PAN_DETENT_SHARE = 0.05f

/**
 * The pan under a thumb whose edge is [fromLeftPx] from the left end of a travel [travelPx] long.
 *
 * The middle of the travel, [detentShare] of it on each side, is the centre; the rest maps evenly
 * onto the values on either side, so every pan can still be reached and the ends are hard left and
 * hard right. A point past either end reads as that end. A travel of nothing reads as the centre;
 * touch input never asks then, because [FaderGesture] ignores a control with no travel.
 */
fun panValueAt(fromLeftPx: Float, travelPx: Float, detentShare: Float = PAN_DETENT_SHARE): Int {
    val sidePx = sideTravel(travelPx, detentShare)
    if (sidePx <= 0f) return PAN_CENTRE
    val position = fromLeftPx.coerceIn(0f, travelPx)
    val rightSideStart = travelPx - sidePx
    return when {
        position <= sidePx -> (position / sidePx * PAN_CENTRE).roundToInt()
        position >= rightSideStart -> PAN_CENTRE + ((position - rightSideStart) / sidePx * PAN_CENTRE).roundToInt()
        else -> PAN_CENTRE
    }.coerceIn(PAN_LEFT, PAN_RIGHT)
}

/**
 * Where [pan] puts the thumb's edge, in pixels from the left end of a travel [travelPx] long: the
 * inverse of [panValueAt], used to draw the thumb. The centre sits exactly in the middle; a pan
 * outside [PAN_LEFT]..[PAN_RIGHT] is drawn at the nearest end.
 */
fun panPositionOf(pan: Int, travelPx: Float, detentShare: Float = PAN_DETENT_SHARE): Float {
    val value = pan.coerceIn(PAN_LEFT, PAN_RIGHT)
    val sidePx = sideTravel(travelPx, detentShare)
    return when {
        value < PAN_CENTRE -> value.toFloat() / PAN_CENTRE * sidePx
        value > PAN_CENTRE -> travelPx - sidePx + (value - PAN_CENTRE).toFloat() / PAN_CENTRE * sidePx
        else -> travelPx / 2f
    }
}

/** How much of the travel each side outside the detent spans. */
private fun sideTravel(travelPx: Float, detentShare: Float): Float =
    travelPx * (0.5f - detentShare.coerceIn(0f, 0.5f))

/**
 * A pan as the strips and the channel sheet write it: "C" at the centre, otherwise the side and
 * how far toward it, from "L2" next to the centre to "L100" hard left (and "R2" to "R100").
 */
fun panText(pan: Int): String {
    val amount = panAmount(pan)
    return when {
        amount == 0 -> "C"
        pan < PAN_CENTRE -> "L$amount"
        else -> "R$amount"
    }
}

/** What a screen reader says for a pan: "Centre", "Left 30", "Right 100". */
fun panSpokenText(pan: Int): String {
    val amount = panAmount(pan)
    return when {
        amount == 0 -> "Centre"
        pan < PAN_CENTRE -> "Left $amount"
        else -> "Right $amount"
    }
}

/** How far [pan] is from the centre, in percent of the way to its side: 0 at the centre, 100 at an end. */
private fun panAmount(pan: Int): Int =
    abs(pan.coerceIn(PAN_LEFT, PAN_RIGHT) - PAN_CENTRE) * 100 / (PAN_RIGHT - PAN_CENTRE)

/**
 * How a fader turns where its thumb is into a value and back. Offsets are pixels from the fader's
 * top end (where [FADER_MAX] sits: the top of a vertical fader, the right end of a horizontal
 * one) along a travel [travelPx] long, as [FaderGesture] measures them.
 */
internal interface FaderScale {
    /** The value with the thumb's edge [offsetPx] from the top end. */
    fun valueAt(offsetPx: Float, travelPx: Float): Int

    /** Where [value] puts the thumb's edge, in pixels from the top end. */
    fun offsetOf(value: Int, travelPx: Float): Float
}

/** The levels' even scale: 0 at one end, [FADER_MAX] at the other ([faderValueAt]). */
internal object LevelScale : FaderScale {
    override fun valueAt(offsetPx: Float, travelPx: Float): Int = faderValueAt(offsetPx, travelPx)

    override fun offsetOf(value: Int, travelPx: Float): Float = faderOffsetOf(value, travelPx)
}

/** The pan's scale, hard left to hard right with the centre detent ([panValueAt]). */
internal object PanScale : FaderScale {
    override fun valueAt(offsetPx: Float, travelPx: Float): Int = panValueAt(travelPx - offsetPx, travelPx)

    override fun offsetOf(value: Int, travelPx: Float): Float = travelPx - panPositionOf(value, travelPx)
}
