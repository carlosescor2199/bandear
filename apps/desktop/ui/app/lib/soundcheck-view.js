// What the Record screen's virtual soundcheck card, its transport's Play and its takes table show
// of the soundcheck, and the shell's SOUNDCHECK chip, worked out from the console state (see
// soundcheck.js for the shapes). Pure functions, no DOM.
//
// - The card: with the inputs live, the takes a soundcheck can play (never the take being
//   recorded) with the one Play would start, a loop choice for the start, and why it cannot start;
//   while a take plays, its name, position and length with a seek slider, pause or play, the loop,
//   "back to live inputs", what paces it (the local output or a timer), the dropouts and a failure.
// - Play has one target: the take chosen in the card, else the last take recorded, else the newest
//   listed. There is no separate "listen only" playback: Play is a soundcheck start.

import { isRecordingNow, lastTake, soundcheckStatus } from "./console-state.js";
import { sentenceWithReason } from "./format.js";
import { formatDuration } from "./recording.js";
import { RECORDING_BLOCKS_SOUNDCHECK, SOUNDCHECK_RATE, isSoundchecking } from "./soundcheck.js";

const BADGES = Object.freeze({ off: "Off", playing: "Playing", paused: "Paused", failed: "Failed" });

const NO_TAKES =
  "Record a take first: its tracks then play back here in place of the inputs, so every mix and " +
  "the phones can be set with nobody playing.";

const NO_ENGINE = "Start the engine to play a take in place of the inputs.";

function engineRuns(state) {
  return state.engine.phase === "running";
}

/** The takes a soundcheck can play, as listed: `{ folder, name, label }`, never the one recording. */
export function playableTakes(state) {
  return (state.takes.list ?? [])
    .filter((take) => take.ended !== "recording" && (take.frames ?? 0) > 0)
    .map((take) => ({
      folder: take.folder,
      name: take.name,
      label: `${take.name} · ${formatDuration(take.seconds)}`,
    }));
}

/**
 * The take folder Play starts: `chosen` when it can be played, else the last take recorded, else
 * the newest listed; null without one.
 */
export function soundcheckTarget(state, chosen) {
  const playable = playableTakes(state);
  const can = (folder) => folder && playable.some((take) => take.folder === folder);
  if (can(chosen)) {
    return chosen;
  }
  const last = lastTake(state)?.folder;
  if (can(last)) {
    return last;
  }
  return playable[0]?.folder ?? null;
}

/** Why a soundcheck cannot start now, or null when it can. */
function startHint(state, target) {
  if (!engineRuns(state)) {
    return NO_ENGINE;
  }
  if (isRecordingNow(state)) {
    return RECORDING_BLOCKS_SOUNDCHECK;
  }
  return target ? null : NO_TAKES;
}

/** A take stopped at its end without looping: paused there. */
function atEnd(soundcheck) {
  return (
    soundcheck.state === "paused" &&
    soundcheck.lengthFrames > 0 &&
    soundcheck.positionFrames >= soundcheck.lengthFrames
  );
}

function underrunWords(frames) {
  if (!frames) {
    return "No dropouts.";
  }
  const ms = Math.round((frames * 1000) / SOUNDCHECK_RATE);
  return `${ms} ms of silence where the disk fell behind.`;
}

/**
 * The virtual soundcheck card: `{ on, kind, badge, takeName, position, length, slider: { max,
 * value, disabled }, canPause, canResume, resumeWord, canStop, looping, canLoop, clock, underruns,
 * failure, choices, chosen, canStart, startHint }`. `chosen` is the take picked in the card,
 * `drag` the seconds the slider is being dragged to, `loopNext` the loop asked for the next start.
 */
export function soundcheckView(state, { chosen = null, drag = null, loopNext = false } = {}) {
  const soundcheck = soundcheckStatus(state);
  const on = isSoundchecking(soundcheck);
  const failed = soundcheck.state === "failed";
  const target = soundcheckTarget(state, chosen);
  const seconds = drag ?? soundcheck.positionSeconds;
  const hint = on ? null : startHint(state, target);
  return {
    on,
    kind: soundcheck.state,
    badge: atEnd(soundcheck) ? "At the end" : (BADGES[soundcheck.state] ?? soundcheck.state),
    takeName: soundcheck.takeName,
    position: formatDuration(seconds),
    length: formatDuration(soundcheck.lengthSeconds),
    slider: failed
      ? { max: soundcheck.lengthSeconds, value: seconds, disabled: true }
      : { max: soundcheck.lengthSeconds, value: seconds },
    canPause: soundcheck.state === "playing",
    canResume: soundcheck.state === "paused",
    resumeWord: atEnd(soundcheck) ? "Play again" : "Play",
    canStop: on,
    looping: on ? soundcheck.looping : Boolean(loopNext),
    canLoop: !failed,
    clock:
      soundcheck.clock === "output"
        ? "Paced by the local output."
        : "Paced by a timer: no local output plays.",
    underruns: underrunWords(soundcheck.underrunFrames),
    failure: failed ? sentenceWithReason("The soundcheck stopped playing", soundcheck.reason) : null,
    choices: playableTakes(state),
    chosen: target,
    canStart: !on && hint === null,
    startHint: hint,
  };
}

/**
 * What the transport's Play does: `{ can, action, folder, label }`, `action` "start" (the target
 * take, `folder`), "pause" or "resume".
 */
export function soundcheckPlay(state, chosen) {
  const soundcheck = soundcheckStatus(state);
  if (soundcheck.state === "playing") {
    return { can: true, action: "pause", folder: null, label: "Pause the soundcheck" };
  }
  if (soundcheck.state === "paused") {
    return { can: true, action: "resume", folder: null, label: "Play the soundcheck on" };
  }
  if (soundcheck.state === "failed") {
    return { can: false, action: null, folder: null, label: "The soundcheck stopped on an error" };
  }
  const folder = soundcheckTarget(state, chosen);
  if (!folder) {
    return { can: false, action: "start", folder: null, label: "Record a take to play it back" };
  }
  const name = playableTakes(state).find((take) => take.folder === folder)?.name ?? "the take";
  return {
    can: engineRuns(state) && !isRecordingNow(state),
    action: "start",
    folder,
    label: `Play "${name}" in place of the live inputs`,
  };
}

/**
 * A take row's Soundcheck button: `{ playing, disabled, title }`. Only one take plays at a time,
 * never the take being recorded, never while one records or without an engine.
 */
export function takeSoundcheck(state, take) {
  const soundcheck = soundcheckStatus(state);
  if (isSoundchecking(soundcheck) && soundcheck.takeFolder === take.folder) {
    return {
      playing: true,
      disabled: true,
      title: "This take plays in place of the live inputs now",
    };
  }
  const recording = take.ended === "recording";
  const blocked =
    recording ||
    !engineRuns(state) ||
    isRecordingNow(state) ||
    isSoundchecking(soundcheck) ||
    !((take.frames ?? 0) > 0);
  return {
    playing: false,
    disabled: blocked,
    title: recording
      ? "This take is being recorded"
      : `Play "${take.name}" in place of the live inputs`,
  };
}

/**
 * The SOUNDCHECK chip, or null while the inputs are live: `{ kind, word, time, detail, label }`,
 * `kind` "soundcheck", "soundcheck-paused" or "soundcheck-failed", `label` its accessible name.
 */
export function soundcheckChip(state) {
  const soundcheck = soundcheckStatus(state);
  if (!isSoundchecking(soundcheck)) {
    return null;
  }
  const name = soundcheck.takeName ?? "the take";
  const time = formatDuration(soundcheck.positionSeconds);
  const whole = Math.floor(soundcheck.positionSeconds ?? 0);
  const at = `${whole} second${whole === 1 ? "" : "s"}`;
  switch (soundcheck.state) {
    case "failed":
      return {
        kind: "soundcheck-failed",
        word: "SOUNDCHECK stopped",
        time,
        detail: name,
        label: `${sentenceWithReason("The soundcheck stopped playing", soundcheck.reason)} Open Record`,
      };
    case "paused":
      return {
        kind: "soundcheck-paused",
        word: "SOUNDCHECK",
        time,
        detail: `${name} · paused`,
        label: `Soundcheck: "${name}" is paused in place of the live inputs, at ${at}. Open Record`,
      };
    default:
      return {
        kind: "soundcheck",
        word: "SOUNDCHECK",
        time,
        detail: name,
        label: `Soundcheck: "${name}" plays in place of the live inputs, at ${at}. Open Record`,
      };
  }
}
