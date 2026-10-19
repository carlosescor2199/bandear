// Packet redundancy set from the desk (Network and system): what each musician's Redundancy
// control shows, and how the desktop's answer to a change is worded. No DOM.
//
// Per musician the engineer turns redundancy on (every packet is sent twice) or off, the default.
// It doubles that musician's bandwidth and nobody else's. By default the copy goes right after its
// original ("Copy: immediate"): no latency is added and every phone takes it, one that has the
// original dropping the copy as a duplicate. The copy may instead go 1-4 blocks later, so a Wi-Fi
// burst must last that long to take both; the phone then holds packets back that long so a late
// copy can fill its gap, which adds that many blocks of latency for that musician (the cost is said
// in milliseconds from the engine's block). Only a phone that announced "redundancy.hold"
// (`redundancyHoldSupported` on its status row) can, so for any other the delays are offered
// disabled with "needs a newer app", and the desktop refuses one (`unsupported`). Only a phone that
// announced "redundancy.recovered" (`redundancySupported`) reports the packets a delayed copy
// recovered; an immediate copy arrives in order and cannot be told from its original, so it is
// never counted.

import { sentenceWithReason } from "./format.js";
import { musicianKey } from "./link-health-view.js";

/** The delay of the copies, in blocks, the desktop uses when none is named: right after. */
export const DEFAULT_DELAY_BLOCKS = 0;
/** The longest delay of a copy, in blocks. */
export const MAX_DELAY_BLOCKS = 4;
/** What a row whose phone does not report recoveries says. */
export const NOT_REPORTED = "not reported by this phone";
/** What a delay offered to a phone that cannot hold packets back for it says. */
export const NEEDS_NEWER_APP = "needs a newer app";

/** The engine's block when the capture runs at the device's default buffer, at 48 kHz. */
const DEFAULT_BLOCK_FRAMES = 256;
const ENGINE_RATE = 48000;
/** One engine block in milliseconds at the default buffer: about 5.3 ms. */
export const DEFAULT_BLOCK_MS = (DEFAULT_BLOCK_FRAMES * 1000) / ENGINE_RATE;

function count(value) {
  return Number(value ?? 0).toLocaleString("en-US");
}

function blocks(delayBlocks) {
  return `${delayBlocks} ${delayBlocks === 1 ? "block" : "blocks"}`;
}

function milliseconds(value) {
  return `${value.toFixed(1)} ms`;
}

/**
 * How long one engine block lasts, in milliseconds, for the capture format the status reports: a
 * fixed buffer of `n` frames at the device's rate lasts n / rate (the engine sends one block per
 * callback, decimated to 48 kHz), anything else the engine's default block.
 */
export function blockMsOf(captureFormat) {
  const match = /^Fixed\((\d+)\)$/.exec(captureFormat?.bufferSize ?? "");
  const rate = Number(captureFormat?.sampleRate);
  if (match && rate > 0) {
    return (Number(match[1]) * 1000) / rate;
  }
  return DEFAULT_BLOCK_MS;
}

/** The latency a delay of `delayBlocks` adds to the phone that holds packets back for it, in ms. */
export function delayCostMs(delayBlocks, blockMs = DEFAULT_BLOCK_MS) {
  return delayBlocks * blockMs;
}

/** "immediate", or "2 blocks later (+10.7 ms)": a delay as the Copy choice names it. */
export function delayLabel(delayBlocks, blockMs = DEFAULT_BLOCK_MS) {
  if (delayBlocks === 0) {
    return "immediate";
  }
  return `${blocks(delayBlocks)} later (+${milliseconds(delayCostMs(delayBlocks, blockMs))})`;
}

/**
 * The Redundancy cell of a status row: `{ key, on, delayBlocks, holdSupported, delays, holdNote,
 * bandwidth, recovered, duplicates }`. `delayBlocks` is the copies' delay (null when off);
 * `delays` the Copy choices `{ value, label, disabled }`, immediate first, a delay disabled for a
 * phone that cannot hold packets back for it (`holdNote` then says it needs a newer app);
 * `bandwidth` what it costs in words, latency included; `recovered` the packets the phone reported
 * a copy recovered (or that it does not report them, or that immediate copies are not counted) and
 * `duplicates` the copies sent, both null when off. `blockMs` is one engine block (blockMsOf).
 */
export function redundancyView(musician, blockMs = DEFAULT_BLOCK_MS) {
  const on = Boolean(musician.redundancy?.on);
  const delayBlocks = on ? (musician.redundancy.delayBlocks ?? DEFAULT_DELAY_BLOCKS) : null;
  const holdSupported = Boolean(musician.redundancyHoldSupported);
  let recovered = null;
  if (on) {
    const packets = Number(musician.recovered ?? 0);
    if (!musician.redundancySupported) {
      recovered = `Recovered packets ${NOT_REPORTED}`;
    } else if (delayBlocks === 0) {
      recovered = "Recovered packets are counted for delayed copies only";
    } else {
      recovered = `${count(packets)} ${packets === 1 ? "packet" : "packets"} recovered`;
    }
  }
  let bandwidth = "Sends once; on doubles this musician's bandwidth";
  if (on) {
    bandwidth =
      delayBlocks === 0
        ? "2× bandwidth, no added latency"
        : `2× bandwidth, +${milliseconds(delayCostMs(delayBlocks, blockMs))} latency on this phone`;
  }
  const delays = [];
  for (let value = 0; value <= MAX_DELAY_BLOCKS; value += 1) {
    const label = `Copy: ${delayLabel(value, blockMs)}`;
    const disabled = value > 0 && !holdSupported;
    delays.push({ value, label: disabled ? `${label}, ${NEEDS_NEWER_APP}` : label, disabled });
  }
  return {
    key: musicianKey(musician),
    on,
    delayBlocks,
    holdSupported,
    delays,
    holdNote: holdSupported ? null : `Delayed copies: ${NEEDS_NEWER_APP}`,
    bandwidth,
    recovered,
    duplicates: on ? `${count(musician.duplicatesSent)} copies sent` : null,
  };
}

/**
 * The desktop's answer to a redundancy change (`set_musician_redundancy`) in words, for `name`'s
 * row: `{ ok, message }`, with what it does to that musician's bandwidth and latency (`blockMs` is
 * one engine block). A delay the phone cannot hold packets back for (`unsupported`) and a request
 * the desktop refused (`invalid`) are not done; a save that failed is said after the outcome.
 */
export function formatRedundancySet(answer, name, blockMs = DEFAULT_BLOCK_MS) {
  const { outcome, redundancy, saveError } = answer;
  if (outcome === "unsupported") {
    return {
      ok: false,
      message: `${name}'s phone ${NEEDS_NEWER_APP} for a delayed copy; an immediate copy works with any app.`,
    };
  }
  if (outcome !== "set") {
    return { ok: false, message: answer.message ?? "Redundancy was not set." };
  }
  const factor = answer.bandwidthFactor ?? (redundancy?.on ? 2 : 1);
  let message = `${name}'s audio is sent once again: their bandwidth is back to normal (×${factor}).`;
  if (redundancy?.on) {
    const delay = redundancy.delayBlocks ?? DEFAULT_DELAY_BLOCKS;
    message =
      delay === 0
        ? `${name}'s audio is now sent twice, each copy right after its original: their bandwidth doubles (×${factor}), with no added latency.`
        : `${name}'s audio is now sent twice, each copy ${blocks(delay)} later: their bandwidth doubles (×${factor}) and their phone plays ${milliseconds(delayCostMs(delay, blockMs))} later.`;
  }
  const result = { ok: true, message };
  if (saveError) {
    result.message += ` ${sentenceWithReason("It was not saved for the next start", saveError)}`;
  }
  return result;
}

/**
 * The engineer's Redundancy choice still to show over what the engine reports, or null. `asked` is
 * the choice sent (`{ on, delayBlocks }`, the delay null for the default), `cell` the row's
 * Redundancy cell (`redundancyView`), `said` the latest answer for this row and `seen` the one that
 * was there before. A choice is settled once the row reports it (on with that delay, or off), or
 * once an answer that was not there before came back.
 */
export function pendingRedundancyChoice(asked, cell, said, seen) {
  if (asked === null || asked === undefined) {
    return null;
  }
  if (said !== null && said !== undefined && said !== seen) {
    return null;
  }
  if (cell.on !== asked.on) {
    return asked;
  }
  const delay = asked.delayBlocks ?? DEFAULT_DELAY_BLOCKS;
  return !asked.on || cell.delayBlocks === delay ? null : asked;
}
