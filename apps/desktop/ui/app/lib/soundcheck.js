// The virtual soundcheck: the pure part of the console's soundcheck, which the Record screen and
// the shell's chip render. No DOM.
//
// A soundcheck plays a recorded take back into the mixer in place of the live inputs (see the
// desktop's commands/soundcheck.rs): the phones, the local output and the meters hear the take as
// they would hear the band. There is no separate "listen only" playback: a take's Play is a
// soundcheck start. The status's `soundcheck`, and every soundcheck command's answer, is `{ state:
// "off" | "playing" | "paused" | "failed", reason, takeFolder, takeName, positionFrames,
// positionSeconds, lengthFrames, lengthSeconds, looping, underrunFrames, clock: "timer" | "output"
// }`. "paused" is also a take that stopped at its end without looping (Play starts it again from
// its start); "failed" is a read error, after which only going back to the live inputs is left.
// While a soundcheck plays, nothing records and the input device stays as it is; while a take
// records, no soundcheck starts.

import { formatDuration } from "./recording.js";

/** Every take plays at the engine's rate. */
export const SOUNDCHECK_RATE = 48000;

/** The soundcheck while the inputs are live. */
export const IDLE_SOUNDCHECK = Object.freeze({
  state: "off",
  reason: null,
  takeFolder: null,
  takeName: null,
  positionFrames: 0,
  positionSeconds: 0,
  lengthFrames: 0,
  lengthSeconds: 0,
  looping: false,
  underrunFrames: 0,
  clock: "timer",
});

/** Why the input device or its driver cannot change while a soundcheck plays. */
export const SOUNDCHECK_BLOCKS_SWITCH =
  "Go back to the live inputs before switching the input device or its driver: a soundcheck " +
  "plays a take in place of the inputs.";

/** Why nothing records while a soundcheck plays. */
export const SOUNDCHECK_BLOCKS_RECORDING =
  "Go back to the live inputs before recording: a take never records a soundcheck's playback.";

/** Why no soundcheck starts while a take records. */
export const RECORDING_BLOCKS_SOUNDCHECK =
  "Stop the recording before the soundcheck: a take records the live inputs.";

/** The soundcheck a status reports, or the idle one for a status without it. */
export function soundcheckOf(status) {
  return status?.soundcheck ?? IDLE_SOUNDCHECK;
}

/** True while a take plays in place of the inputs (playing, paused or failed). */
export function isSoundchecking(soundcheck) {
  return Boolean(soundcheck) && soundcheck.state !== "off";
}

/** The frame `seconds` into a take of `lengthFrames` frames: whole, and within the take. */
export function frameAt(seconds, lengthFrames) {
  const frame = Math.round((Number(seconds) || 0) * SOUNDCHECK_RATE);
  return Math.min(Math.max(0, frame), Math.max(0, lengthFrames ?? 0));
}

/** What a soundcheck command did, in words: `action` and its answer `soundcheck`. */
export function formatSoundcheckDone(action, soundcheck) {
  const at = formatDuration(soundcheck?.positionSeconds ?? 0);
  const name = soundcheck?.takeName ?? "the take";
  switch (action) {
    case "start":
      return (
        `Soundcheck: "${name}" plays in place of the live inputs from ${at}` +
        `${soundcheck?.looping ? ", looping" : ""}.`
      );
    case "pause":
      return `Soundcheck paused at ${at}.`;
    case "resume":
      return `Soundcheck playing from ${at}.`;
    case "seek":
      return `Soundcheck moved to ${at}.`;
    case "loop":
      return soundcheck?.looping
        ? "The soundcheck starts the take again at its end."
        : "The soundcheck stops at the end of the take.";
    case "stop":
      return "Back to the live inputs.";
    default:
      return "";
  }
}

/** The lead of a refused soundcheck command's words, by `action`. */
export const SOUNDCHECK_REFUSED = Object.freeze({
  start: "The soundcheck did not start",
  pause: "The soundcheck was not paused",
  resume: "The soundcheck did not play on",
  seek: "The soundcheck did not move",
  loop: "The soundcheck's loop was not changed",
  stop: "The soundcheck did not stop",
});
