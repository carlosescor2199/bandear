// Recording takes: the pure part of the console's recording, which the Record screen renders. No
// DOM.
//
// The desk records the armed inputs of the running device into a take folder, a file per input
// and one stereo file per linked pair, plus the local mix when asked (see the desktop's
// commands/recording.rs). How it records is the desk's own settings (`recording_settings`):
// `{ folder, defaultFolder, folderIsDefault, format: "int24" | "float32", includeLocal, armed:
// [1-based inputs], notice }`. The status's `recording` says whether a take records: `{ state:
// "idle" | "recording" | "stopping" | "failed", reason, takeFolder, frames, seconds,
// droppedFrames, bytesWritten, tracks, markers: [{ frame, seconds, name }], freeBytes, minutesLeft
// }`, and `lastTake` is the last take that ended. A take as the desk lists it (`list_takes`) or
// answers a stop: `{ folder, name, started, sampleRate, format, device, tracks: [{ file, kind,
// channels, name, instrument, frames }], frames, seconds, markers, droppedFrames, ended:
// "stopped" | "engineStopped" | "diskError" | "recording" | "interrupted", reason }`.

import { sentenceWithReason } from "./format.js";

/** The recorder while no take records. */
export const IDLE_RECORDING = Object.freeze({
  state: "idle",
  reason: null,
  takeFolder: null,
  frames: 0,
  seconds: 0,
  droppedFrames: 0,
  bytesWritten: 0,
  tracks: Object.freeze([]),
  markers: Object.freeze([]),
  freeBytes: null,
  minutesLeft: null,
});

/** The formats a take is recorded in, the default first. */
export const RECORD_FORMATS = Object.freeze(["int24", "float32"]);

/** Why the input device or its driver cannot change while a take records. */
export const RECORDING_BLOCKS_SWITCH =
  "Stop the recording before switching the input device or its driver: a take never mixes two " +
  "devices.";

/** The recorder a status reports, or the idle one for a status without it. */
export function recordingOf(status) {
  return status?.recording ?? IDLE_RECORDING;
}

/** True while a take records or its files are being finished: the input cannot change. */
export function isRecording(recording) {
  return recording?.state === "recording" || recording?.state === "stopping";
}

/**
 * The recorded units of a device's inputs, as the engine lays out the tracks: `[first, second]`
 * (0-based) for an input linked to the next one, else `[index]`. Hidden inputs record too.
 */
export function recordUnits(channels) {
  const units = [];
  const list = channels ?? [];
  for (let index = 0; index < list.length; index += 1) {
    if (list[index]?.link && index + 1 < list.length) {
      units.push([index, index + 1]);
      index += 1;
    } else {
      units.push([index]);
    }
  }
  return units;
}

function unitOf(input, channels) {
  const index = input - 1;
  return recordUnits(channels).find((unit) => unit.includes(index)) ?? [index];
}

/**
 * True when `input` (1-based) records with `armed` on a device with `channels` (a status's): it is
 * armed, or the other side of its linked pair is, since either side arms the pair.
 */
export function isArmed(armed, input, channels) {
  const kept = new Set(armed ?? []);
  return unitOf(input, channels).some((index) => kept.has(index + 1));
}

/**
 * The armed inputs after arming (`on`) or disarming `input` (1-based), ascending and each once.
 * Disarming one side of a linked pair disarms the pair: either side alone would still record it.
 */
export function armedAfter(armed, input, on, channels) {
  const next = new Set(armed ?? []);
  if (on) {
    next.add(input);
  } else {
    for (const index of unitOf(input, channels)) {
      next.delete(index + 1);
    }
  }
  return [...next].sort((one, other) => one - other);
}

/**
 * The whole `set_recording_settings` request for `settings` (a `recording_settings` answer, or
 * null for the defaults) with `change` (`{ folder?, format?, includeLocal?, armed?, arm? }`) on
 * top. The folder is null for the default folder. `arm` (`{ input, on, channels }`) arms or
 * disarms one input on the armed inputs the request is built on (see armedAfter), so arm presses
 * made before an earlier one was answered compose instead of replacing each other.
 */
export function settingsRequest(settings, change = {}) {
  const current = {
    folder: settings && !settings.folderIsDefault ? settings.folder : null,
    format: settings?.format ?? RECORD_FORMATS[0],
    includeLocal: Boolean(settings?.includeLocal),
    armed: [...(settings?.armed ?? [])],
  };
  const next = { ...current };
  for (const field of Object.keys(current)) {
    if (change[field] !== undefined) {
      next[field] = change[field];
    }
  }
  if (change.arm) {
    const { input, on, channels } = change.arm;
    next.armed = armedAfter(next.armed, input, on, channels);
  }
  if (typeof next.folder === "string" && next.folder.trim() === "") {
    next.folder = null;
  }
  return next;
}

/** `seconds` as a clock: "0:07", "3:25", "1:02:03". */
export function formatDuration(seconds) {
  const whole = Math.max(0, Math.floor(Number(seconds) || 0));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, "0");
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

function plural(count, noun) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function channelList(inputs) {
  return inputs.map((input) => `CH ${input}`).join(", ");
}

/**
 * What a start did, from the `start_recording` answer `{ recording, folder, armed, skipped,
 * includeLocal, format }`, for a take called `name`.
 */
export function formatStarted({ recording, folder, skipped }, name) {
  const tracks = recording?.tracks?.length ?? 0;
  let words = `Recording "${name}" on ${plural(tracks, "track")} in ${folder}.`;
  if (skipped?.length > 0) {
    const verb = skipped.length === 1 ? "is" : "are";
    words += ` ${channelList(skipped)} ${verb} armed but not on this device, so not recorded.`;
  }
  return words;
}

/** How a take ended, in words. */
export function formatTakeEnd(take) {
  switch (take?.ended) {
    case "stopped":
      return "stopped";
    case "engineStopped":
      return "ended when the engine stopped";
    case "diskError":
      return sentenceWithReason("stopped by a disk error", take.reason).replace(/\.$/, "");
    case "recording":
      return "recording now";
    case "interrupted":
      return "interrupted before it was finished; its files hold what was written";
    default:
      return "ended";
  }
}

/** What a finished stop did: the take, its length, its tracks and anything lost. */
export function formatStopped(take) {
  let words =
    `"${take.name}" ${formatTakeEnd(take)}: ${formatDuration(take.seconds)} on ` +
    `${plural(take.tracks?.length ?? 0, "track")}.`;
  if (take.droppedFrames > 0) {
    words += ` ${plural(take.droppedFrames, "frame")} the disk could not keep up with are missing.`;
  }
  return words;
}

/** What `list_takes` left out, in words, or null when it read every take. */
export function formatUnreadable({ unreadable }) {
  if (!unreadable) {
    return null;
  }
  const verb = unreadable === 1 ? "is" : "are";
  return `${plural(unreadable, "take")} could not be read and ${verb} not listed.`;
}
