// Packet redundancy set from the desk (Network and system): what each musician's Redundancy
// control shows, and how the desktop's answer to a change is worded. No DOM.
//
// Per musician the engineer turns redundancy on (every packet is sent again a few blocks later, so
// a Wi-Fi burst must take both copies to cost audio) or off, the default. It doubles that
// musician's bandwidth and nobody else's. Every phone takes it: an older one drops the copies as
// duplicates. Only a phone that announced "redundancy.recovered" (`redundancySupported` on its
// status row) reports the packets a copy recovered; for any other the row says it is not reported.

import { sentenceWithReason } from "./format.js";
import { musicianKey } from "./link-health-view.js";

/** The delay of the copies, in blocks, the desktop uses when none is named. */
export const DEFAULT_DELAY_BLOCKS = 2;
/** What a row whose phone does not report recoveries says. */
export const NOT_REPORTED = "not reported by this phone";

function count(value) {
  return Number(value ?? 0).toLocaleString("en-US");
}

function blocks(delayBlocks) {
  return `${delayBlocks} ${delayBlocks === 1 ? "block" : "blocks"}`;
}

/**
 * The Redundancy cell of a status row: `{ key, on, delayBlocks, bandwidth, recovered, duplicates }`.
 * `delayBlocks` is the copies' delay (null when off), `bandwidth` what it costs in words,
 * `recovered` the packets the phone reported a copy recovered (or that it does not report them)
 * and `duplicates` the copies sent, both null when off.
 */
export function redundancyView(musician) {
  const on = Boolean(musician.redundancy?.on);
  const delayBlocks = on ? (musician.redundancy.delayBlocks ?? DEFAULT_DELAY_BLOCKS) : null;
  let recovered = null;
  if (on) {
    const packets = Number(musician.recovered ?? 0);
    recovered = musician.redundancySupported
      ? `${count(packets)} ${packets === 1 ? "packet" : "packets"} recovered`
      : `Recovered packets ${NOT_REPORTED}`;
  }
  return {
    key: musicianKey(musician),
    on,
    delayBlocks,
    bandwidth: on
      ? `2× bandwidth, each copy ${blocks(delayBlocks)} later`
      : "Sends once; on doubles this musician's bandwidth",
    recovered,
    duplicates: on ? `${count(musician.duplicatesSent)} copies sent` : null,
  };
}

/**
 * The desktop's answer to a redundancy change (`set_musician_redundancy`) in words, for `name`'s
 * row: `{ ok, message }`, with what it does to that musician's bandwidth. A request the desktop
 * refused (`invalid`) is not done; a save that failed is said after the outcome.
 */
export function formatRedundancySet(answer, name) {
  const { outcome, redundancy, saveError } = answer;
  if (outcome !== "set") {
    return { ok: false, message: answer.message ?? "Redundancy was not set." };
  }
  const factor = answer.bandwidthFactor ?? (redundancy?.on ? 2 : 1);
  const result = {
    ok: true,
    message: redundancy?.on
      ? `${name}'s audio is now sent twice, each copy ${blocks(redundancy.delayBlocks ?? DEFAULT_DELAY_BLOCKS)} later: their bandwidth doubles (×${factor}).`
      : `${name}'s audio is sent once again: their bandwidth is back to normal (×${factor}).`,
  };
  if (saveError) {
    result.message += ` ${sentenceWithReason("It was not saved for the next start", saveError)}`;
  }
  return result;
}

/**
 * The engineer's Redundancy choice still to show over what the engine reports, or null. `asked` is
 * the choice sent (`{ on }`), `cell` the row's Redundancy cell (`redundancyView`), `said` the latest
 * answer for this row and `seen` the one that was there before. A choice is settled once the row
 * reports it, or once an answer that was not there before came back.
 */
export function pendingRedundancyChoice(asked, cell, said, seen) {
  if (asked === null || asked === undefined) {
    return null;
  }
  if (said !== null && said !== undefined && said !== seen) {
    return null;
  }
  return cell.on === asked.on ? null : asked;
}

