// The phone's playback buffer set from the desk (Network and system): what each musician's Buffer
// control shows, what a change asks the desktop for, and how its answer is worded. No DOM.
//
// Per musician the engineer picks Auto (the phone adapts its own cushion, the default) or Fixed, a
// length in whole milliseconds the phone keeps without adapting. Only a phone that announced
// "playback.buffer" takes one (`bufferSupported` on its status row); any other shows "needs a
// newer app" and no control. The phone rounds a fixed length up to whole packets and says the
// cushion it plays in its reports, shown as "plays 42.7 ms" from the row's newest link second
// (`linkLatest.cushionMs`).

import { musicianLabel, sentenceWithReason } from "./format.js";
import { musicianKey } from "./link-health-view.js";

/** The shortest and longest fixed buffer the desk sets, in milliseconds. */
export const MIN_BUFFER_MS = 5;
export const MAX_BUFFER_MS = 500;
/** The length Fixed starts at for a musician who had none. */
export const DEFAULT_FIXED_MS = 40;
/** What a row whose phone takes no buffer from the desk says. */
export const NEEDS_NEWER_APP = "needs a newer app";

/**
 * The Buffer cell of a status row: `{ key, supported, mode, ms, playing, note }`. `mode` is "auto"
 * or "fixed", `ms` the fixed length (null for Auto), `playing` the cushion the phone reports
 * ("plays 42.7 ms", null before it reports one), and `note` "needs a newer app" for a phone that
 * takes no buffer (null otherwise).
 */
export function bufferView(musician) {
  const supported = Boolean(musician.bufferSupported);
  const buffer = musician.buffer ?? { mode: "auto" };
  const fixed = buffer.mode === "fixed" && Number.isInteger(buffer.ms);
  const cushion = musician.linkLatest?.cushionMs;
  return {
    key: musicianKey(musician),
    supported,
    mode: fixed ? "fixed" : "auto",
    ms: fixed ? buffer.ms : null,
    playing: Number.isFinite(cushion) ? `plays ${cushion.toFixed(1)} ms` : null,
    note: supported ? null : NEEDS_NEWER_APP,
  };
}

/**
 * What the engineer's choice asks for: `{ ok: true, mode, ms }` (`ms` null for Auto) or `{ ok:
 * false, message }`. A fixed length is whole milliseconds from MIN_BUFFER_MS to MAX_BUFFER_MS, as
 * typed (`msText`, a string or a number).
 */
export function bufferRequest(mode, msText) {
  if (mode === "auto") {
    return { ok: true, mode: "auto", ms: null };
  }
  if (mode !== "fixed") {
    return { ok: false, message: `The buffer is Auto or Fixed, not ${mode}.` };
  }
  const text = String(msText ?? "").trim();
  if (!/^\d+$/.test(text)) {
    return {
      ok: false,
      message: `Type the buffer in whole milliseconds, from ${MIN_BUFFER_MS} to ${MAX_BUFFER_MS}.`,
    };
  }
  const ms = Number(text);
  if (ms < MIN_BUFFER_MS || ms > MAX_BUFFER_MS) {
    return {
      ok: false,
      message: `A fixed buffer is from ${MIN_BUFFER_MS} to ${MAX_BUFFER_MS} ms, not ${ms} ms.`,
    };
  }
  return { ok: true, mode: "fixed", ms };
}

// A buffer in words: "Auto" or "a fixed 40 ms buffer".
function describe(buffer) {
  return buffer?.mode === "fixed" ? `a fixed ${buffer.ms} ms buffer` : "Auto";
}

/**
 * The desktop's answer to a buffer change (`set_musician_buffer`) in words, for `name`'s row:
 * `{ ok, message }`. Sent and kept for a phone that is away are done (`ok`); a phone that needs a
 * newer app and a request the desktop refused are not. A save that failed is said after it.
 */
export function formatBufferSet(answer, name) {
  const { outcome, buffer, saveError } = answer;
  let result;
  if (outcome === "sent") {
    result = {
      ok: true,
      message:
        buffer?.mode === "fixed"
          ? `Sent ${name}'s phone ${describe(buffer)}.`
          : `${name}'s phone is back on Auto.`,
    };
  } else if (outcome === "notConnected") {
    result = {
      ok: true,
      message: `Kept for ${name}: their phone gets ${describe(buffer)} when it is connected again.`,
    };
  } else if (outcome === "unsupported") {
    return {
      ok: false,
      message: `${name}'s phone ${NEEDS_NEWER_APP} to take its buffer from the desk.`,
    };
  } else {
    return { ok: false, message: answer.message ?? "The buffer was not set." };
  }
  if (saveError) {
    result.message += ` ${sentenceWithReason("It was not saved for the next start", saveError)}`;
  }
  return result;
}

/**
 * The engineer's Buffer choice still to show over what the engine reports, or null. `asked` is the
 * choice sent (`{ mode, ms }`, `ms` as typed), `buffer` the row's Buffer cell (`bufferView`),
 * `said` the latest answer for this row and `seen` the one that was there before. A choice is
 * settled once the row reports it, compared by value (a typed "040" is the 40 reported), or once an
 * answer that was not there before came back (a refused length is never reported).
 */
export function pendingBufferChoice(asked, buffer, said, seen) {
  if (asked === null || asked === undefined) {
    return null;
  }
  if (said !== null && said !== undefined && said !== seen) {
    return null;
  }
  if (buffer.mode !== asked.mode) {
    return asked;
  }
  if (buffer.mode === "auto") {
    return null;
  }
  const typed = String(asked.ms ?? "").trim();
  return /^\d+$/.test(typed) && Number(typed) === buffer.ms ? null : asked;
}

/** The name the desk shows for the musician `key` names among `musicians`, else the key itself. */
export function nameForKey(musicians, key) {
  const musician = musicians.find((candidate) => musicianKey(candidate) === key);
  return musician ? musicianLabel(musician) : key;
}
