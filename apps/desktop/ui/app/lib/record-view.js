// What the Record screen shows, worked out from the console state (see recording.js for the
// shapes the desk sends). Pure functions, no DOM: the screen only renders these plain objects.
//
// - Track rows: one per recorded unit of the running device, as the engine lays out the tracks (a
//   linked pair is one stereo track, either side arms it; hidden inputs record too), then the
//   stereo local mix, which the settings' `includeLocal` arms.
// - Takes: one row per take the recordings folder holds, newest first; the take being recorded
//   shows the length the status reports.
// - The REC chip: shown on every screen while a take records, is being finished or has failed.
// - The disk: free space and minutes left, the recorder's while a take records, else the estimate
//   for the kept settings, with a warning below LOW_MINUTES and a stronger one below
//   CRITICAL_MINUTES.
// - The waveform lanes are drawn from a bounded peak history per track (createPeakHistory), fed by
//   the `meters` event while a take records: no engine call, nothing that grows.

import { sentenceWithReason } from "./format.js";
import { isRecordingNow, lastTake, recordingStatus } from "./console-state.js";
import {
  RECORD_FORMATS,
  formatDuration,
  formatTakeEnd,
  isArmed,
  isRecording,
  recordUnits,
} from "./recording.js";

/** Below this many minutes of recording space the disk is said to be low. */
export const LOW_MINUTES = 30;

/** Below this many minutes the warning says to act before the take fills the disk. */
export const CRITICAL_MINUTES = 10;

const FORMAT_LABELS = Object.freeze({
  int24: "WAV 24-bit · one file per track",
  float32: "WAV 32-bit float · one file per track",
});

const FORMAT_WORDS = Object.freeze({ int24: "24-bit", float32: "32-bit float" });

/** Why arming and the settings cannot change during a take. */
const LOCKED_WHILE_RECORDING =
  "Arming and the settings stay as they are while a take records; they apply from the next take.";

function plural(count, noun) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function sentence(text) {
  const trimmed = String(text ?? "")
    .trim()
    .replace(/\.+$/, "");
  return trimmed ? `${trimmed[0].toUpperCase()}${trimmed.slice(1)}.` : null;
}

// The inputs of the running device as the engine records them, or none while no engine runs.
function deviceChannels(state) {
  const status = state.engine.status;
  return status ? status.channels.slice(0, status.sourceChannels) : [];
}

/**
 * One row per recorded unit of the running device, left to right: `{ key, input, inputs, sides,
 * stereo, name, source, armed }`. `inputs` are 1-based (what arming takes), `sides` 0-based (what
 * the meters are indexed by), `input` the first. A row is called as the engine names its track: the
 * input's name, else "CH 3" or "CH 3-4" for a pair.
 */
export function trackRows(state) {
  const channels = deviceChannels(state);
  const armed = state.recordingSettings?.armed ?? [];
  return recordUnits(channels).map((sides) => {
    const inputs = sides.map((side) => side + 1);
    const stereo = inputs.length === 2;
    const channel = channels[sides[0]];
    const numbers = stereo ? `${inputs[0]}-${inputs[1]}` : String(inputs[0]);
    const name = String(channel.name ?? "").trim() || `CH ${numbers}`;
    let source = stereo ? `IN ${inputs[0]}–${inputs[1]} · stereo` : `IN ${inputs[0]}`;
    if (sides.every((side) => channels[side].hidden)) {
      source += " · hidden";
    }
    return {
      key: `in-${inputs[0]}`,
      input: inputs[0],
      inputs,
      sides,
      stereo,
      name,
      source,
      armed: isArmed(armed, inputs[0], channels),
    };
  });
}

/**
 * The stereo local mix's row, `{ key: "local", local: true, name, source, armed }`, armed by the
 * settings' `includeLocal`; null while the settings are unknown.
 */
export function localTrackRow(state) {
  const settings = state.recordingSettings;
  if (!settings) {
    return null;
  }
  return {
    key: "local",
    local: true,
    name: "Local mix",
    source: "This computer's mix · stereo",
    armed: Boolean(settings.includeLocal),
  };
}

/**
 * The settings change pressing a row's arm button asks for: the row armed or disarmed (a pair as
 * one, see armedAfter), or the local mix on or off. The arming is a delta (`arm`, see
 * settingsRequest) applied when the request is built, so two quick presses on two rows both land.
 * `armedNow` is what the press toggles: the row as shown, or what a press still on its way asked.
 */
export function armChange(state, row, armedNow = row.armed) {
  if (row.local) {
    return { includeLocal: !armedNow };
  }
  return { arm: { input: row.input, on: !armedNow, channels: deviceChannels(state) } };
}

/**
 * When a take started (an RFC 3339 time) in the window's local time, `offsetMinutes` east of UTC:
 * "2026-10-16 21:04", or "Unknown".
 */
export function formatTakeStarted(started, offsetMinutes) {
  const time = Date.parse(started);
  if (!Number.isFinite(time)) {
    return "Unknown";
  }
  const local = new Date(time + (Number(offsetMinutes) || 0) * 60_000);
  const pad = (number) => String(number).padStart(2, "0");
  return (
    `${local.getUTCFullYear()}-${pad(local.getUTCMonth() + 1)}-${pad(local.getUTCDate())} ` +
    `${pad(local.getUTCHours())}:${pad(local.getUTCMinutes())}`
  );
}

const END_LABELS = Object.freeze({
  stopped: { text: "Stopped", kind: "ok" },
  engineStopped: { text: "Engine stopped", kind: "warn" },
  diskError: { text: "Disk error", kind: "error" },
  interrupted: { text: "Interrupted", kind: "warn" },
  recording: { text: "Recording", kind: "live" },
});

/**
 * How a take ended, short and whole: `{ text, kind, detail }`, `kind` one of "ok", "warn", "error"
 * and "live", `detail` the sentence formatTakeEnd says.
 */
export function takeEndLabel(take) {
  const label = END_LABELS[take?.ended] ?? { text: "Ended", kind: "ok" };
  return { ...label, detail: formatTakeEnd(take) };
}

// "3 tracks + local mix", "1 track", "Local mix only".
function tracksSummary(inputTracks, local) {
  if (inputTracks === 0) {
    return local ? "Local mix only" : "Nothing armed";
  }
  return `${plural(inputTracks, "track")}${local ? " + local mix" : ""}`;
}

/**
 * One row of the takes table: `{ key, folder, name, started, length, tracks, markers, end,
 * dropped, recording }`, its start in the window's local time (`offsetMinutes` east of UTC).
 */
export function takeRow(take, offsetMinutes) {
  const tracks = take.tracks ?? [];
  const local = tracks.some((track) => track.kind === "local");
  const markers = take.markers?.length ?? 0;
  return {
    key: take.folder,
    folder: take.folder,
    name: take.name,
    started: formatTakeStarted(take.started, offsetMinutes),
    length: formatDuration(take.seconds),
    tracks: tracksSummary(tracks.length - (local ? 1 : 0), local),
    markers: markers === 0 ? "No markers" : plural(markers, "marker"),
    end: takeEndLabel(take),
    dropped: take.droppedFrames > 0 ? `${plural(take.droppedFrames, "frame")} missing` : null,
    recording: take.ended === "recording",
  };
}

/**
 * The takes table, newest first as listed (none until listed). The take being recorded now shows
 * the length the status reports, which moves on four times a second.
 */
export function takeRows(state, offsetMinutes) {
  const recording = recordingStatus(state);
  return (state.takes.list ?? []).map((take) => {
    const live = isRecording(recording) && take.folder === recording.takeFolder;
    return takeRow(live ? { ...take, seconds: recording.seconds } : take, offsetMinutes);
  });
}

/** `seconds` in words: "12 minutes 34 seconds", "1 hour 2 minutes 5 seconds", "0 seconds". */
export function spokenDuration(seconds) {
  const whole = Math.max(0, Math.floor(Number(seconds) || 0));
  const parts = [
    [Math.floor(whole / 3600), "hour"],
    [Math.floor((whole % 3600) / 60), "minute"],
    [whole % 60, "second"],
  ]
    .filter(([count]) => count > 0)
    .map(([count, noun]) => plural(count, noun));
  return parts.length > 0 ? parts.join(" ") : "0 seconds";
}

/** A take's name from its folder: the folder's own name without its time stamp. */
export function takeNameOf(folder) {
  const base = String(folder ?? "")
    .split(/[\\/]/)
    .pop();
  return base.replace(/^\d{4}-\d{2}-\d{2}_\d{6}_/, "");
}

/**
 * The REC chip, or null while no take records: `{ kind, word, time, detail, label }`, `kind`
 * "recording", "stopping" (its files are being finished) or "failed", `label` the chip's
 * accessible name.
 */
export function recChip(state) {
  const recording = recordingStatus(state);
  const time = formatDuration(recording.seconds);
  const detail = plural(recording.tracks?.length ?? 0, "track");
  const spoken = spokenDuration(recording.seconds);
  switch (recording.state) {
    case "recording":
      return {
        kind: "recording",
        word: "REC",
        time,
        detail,
        label: `Recording, ${spoken}, ${detail}. Open Record`,
      };
    case "stopping":
      return {
        kind: "stopping",
        word: "Finishing",
        time,
        detail,
        label: `Finishing the take's files after ${spoken}. Open Record`,
      };
    case "failed":
      return {
        kind: "failed",
        word: "REC stopped",
        time,
        detail,
        label: `${sentenceWithReason("The recording stopped", recording.reason)} Open Record`,
      };
    default:
      return null;
  }
}

/** A size in the decimal units a file manager shows: "214 GB", "1.3 GB", "705 MB". */
export function formatBytes(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = Math.max(0, Number(bytes) || 0);
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

// "45 min" up to two hours, then whole hours: "34 h".
function formatMinutes(minutes) {
  return minutes < 120 ? `${minutes} min` : `${Math.floor(minutes / 60)} h`;
}

/** The warning for `minutes` of recording space left, or null when there is enough (or unknown). */
function diskWarning(minutes) {
  if (minutes === null || minutes === undefined || minutes >= LOW_MINUTES) {
    return null;
  }
  const verb = minutes === 1 ? "is" : "are";
  if (minutes < CRITICAL_MINUTES) {
    const lead =
      minutes <= 0
        ? "No recording space is left"
        : `Only about ${plural(minutes, "minute")} of recording space ${verb} left`;
    return {
      level: "critical",
      text: `${lead}: free some disk space or choose another folder before the take fills it.`,
    };
  }
  return {
    level: "low",
    text: `About ${plural(minutes, "minute")} of recording space ${verb} left on this disk.`,
  };
}

/**
 * The disk: `{ free, left, problem, warning }`. While a take records, what the recorder reports;
 * otherwise the estimate for the kept settings, and why it cannot be made (`problem`).
 */
export function diskView(state) {
  const recording = recordingStatus(state);
  const live = isRecording(recording);
  const estimate = state.estimate;
  const free = live ? recording.freeBytes : (estimate?.freeBytes ?? null);
  const minutes = live ? recording.minutesLeft : (estimate?.minutesLeft ?? null);
  return {
    free: free === null || free === undefined ? "Free space unknown" : `${formatBytes(free)} free`,
    left: minutes === null || minutes === undefined ? null : `about ${formatMinutes(minutes)} left`,
    problem: live ? null : sentence(estimate?.problem),
    warning: diskWarning(minutes ?? null),
  };
}

/**
 * A bounded history of peak levels for one track's waveform lane: `capacity` levels at most, the
 * oldest dropped as a new one comes, kept in a fixed array, so it never grows however long a take
 * runs. `forEach(fn)` and `at(index)` go from the oldest.
 */
export function createPeakHistory(capacity) {
  const size = Math.max(1, Math.floor(Number(capacity) || 0));
  const levels = new Float32Array(size);
  let start = 0;
  let length = 0;
  return {
    capacity: size,
    get length() {
      return length;
    },
    push(level) {
      if (length < size) {
        levels[(start + length) % size] = level;
        length += 1;
      } else {
        levels[start] = level;
        start = (start + 1) % size;
      }
    },
    at(index) {
      return levels[(start + index) % size];
    },
    forEach(visit) {
      for (let index = 0; index < length; index += 1) {
        visit(levels[(start + index) % size], index);
      }
    },
    clear() {
      start = 0;
      length = 0;
    },
  };
}

function markerRows(markers) {
  return (markers ?? []).map((marker, index) => ({
    key: String(index),
    time: formatDuration(marker.seconds),
    name: marker.name,
  }));
}

/**
 * The transport: what can be pressed, the position, the state in words, the markers and what a
 * take would record. `local.atStart` is true once Back to start was pressed after a take; until
 * then the position shows the last take's length and its markers.
 */
export function transportView(state, local) {
  const recording = recordingStatus(state);
  const live = isRecording(recording);
  const engineRuns = state.engine.phase === "running";
  const last = lastTake(state);
  let seconds = 0;
  let markers = [];
  let markersOf = null;
  if (live || recording.state === "failed") {
    seconds = recording.seconds;
    markers = recording.markers;
    markersOf = takeNameOf(recording.takeFolder);
  } else if (last && !local.atStart) {
    seconds = last.seconds;
    markers = last.markers;
    markersOf = last.name;
  }
  let stateWord = "Ready to record";
  if (recording.state === "recording") {
    stateWord = `Recording · ${takeNameOf(recording.takeFolder)}`;
  } else if (recording.state === "stopping") {
    stateWord = "Finishing the take's files…";
  } else if (recording.state === "failed") {
    stateWord = "The take stopped on an error";
  } else if (!engineRuns) {
    stateWord = "Start the engine to record";
  }
  const rows = trackRows(state).filter((row) => row.armed);
  const settings = state.recordingSettings;
  const format = settings?.format ?? RECORD_FORMATS[0];
  return {
    recording: live,
    stateKind: recording.state,
    canRecord: engineRuns && !live,
    canStop: live || recording.state === "failed",
    canMark: recording.state === "recording",
    canRewind: !live && recording.state !== "failed" && seconds > 0,
    position: formatDuration(seconds),
    stateWord,
    markers: markerRows(markers),
    markersOf,
    armed: tracksSummary(rows.length, Boolean(settings?.includeLocal)),
    format: `WAV · 48 kHz · ${FORMAT_WORDS[format] ?? format}`,
  };
}

/**
 * The screen's messages: the last command's `outcome` `{ ok, text }`, a take that failed, a takes
 * listing that failed, and why arming is `locked` while a take records; each null when there is
 * nothing to say.
 */
export function recordMessages(state) {
  const recording = recordingStatus(state);
  const outcome = state.recordingOutcome;
  return {
    outcome: outcome ? { ok: outcome.ok, text: outcome.message } : null,
    failure:
      recording.state === "failed"
        ? sentenceWithReason("The take stopped recording", recording.reason)
        : null,
    takes: state.takes.error
      ? sentenceWithReason("The takes could not be listed", state.takes.error)
      : null,
    locked: isRecordingNow(state) ? LOCKED_WHILE_RECORDING : null,
  };
}

/**
 * The settings panel: `{ known, folder, defaultFolder, isDefault, notice, format, formats,
 * includeLocal, locked }`; `known` is false until the desk's settings were read, and `locked` is
 * true while a take records.
 */
export function settingsView(state) {
  const settings = state.recordingSettings;
  return {
    known: settings !== null,
    folder: settings?.folder ?? "",
    defaultFolder: settings?.defaultFolder ?? "",
    isDefault: settings?.folderIsDefault ?? true,
    notice: settings?.notice ?? null,
    format: settings?.format ?? RECORD_FORMATS[0],
    formats: RECORD_FORMATS.map((value) => ({ value, label: FORMAT_LABELS[value] ?? value })),
    includeLocal: Boolean(settings?.includeLocal),
    locked: isRecordingNow(state),
  };
}
