// What the Record screen and the shell show of the virtual soundcheck (soundcheck-view.js and the
// transport in record-view.js), worked out from the console state without a DOM.
import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";
import { recordMessages, takeRows, transportView } from "../app/lib/record-view.js";
import { IDLE_RECORDING } from "../app/lib/recording.js";
import {
  IDLE_SOUNDCHECK,
  RECORDING_BLOCKS_SOUNDCHECK,
  SOUNDCHECK_BLOCKS_RECORDING,
} from "../app/lib/soundcheck.js";
import {
  playableTakes,
  soundcheckChip,
  soundcheckTarget,
  soundcheckView,
} from "../app/lib/soundcheck-view.js";

const ROOT = "D:\\Recordings";

function take(name, extra = {}) {
  return {
    folder: `${ROOT}\\2026-10-16_2100${name.length}0_${name}`,
    name,
    started: "2026-10-16T19:00:00Z",
    sampleRate: 48000,
    format: "int24",
    device: "Demo interface",
    tracks: [{ file: "01 Kick.wav", kind: "input", channels: [1], name: "Kick", frames: 4_800_000 }],
    frames: 4_800_000,
    seconds: 100,
    markers: [],
    droppedFrames: 0,
    ended: "stopped",
    reason: null,
    ...extra,
  };
}

const SHOW = take("Show");
const LINE = take("Line check", { seconds: 62, frames: 2_976_000 });
const LIVE = take("Encore", { ended: "recording" });

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

/** A running engine's state with `status` changes, the takes listed and the phase running. */
function running({ status = {}, takes = [SHOW, LINE] } = {}) {
  return reduce([
    actions.status({ ...sampleStatus(), ...status }),
    actions.engine("running"),
    actions.takes({ folder: ROOT, takes, unreadable: 0, missing: false }),
  ]);
}

function soundcheckOn(changes = {}) {
  return {
    ...IDLE_SOUNDCHECK,
    state: "playing",
    takeFolder: SHOW.folder,
    takeName: "Show",
    positionFrames: 12 * 48000,
    positionSeconds: 12,
    lengthFrames: 100 * 48000,
    lengthSeconds: 100,
    ...changes,
  };
}

test("the takes a soundcheck can play leave out the take being recorded, and Play has a target", () => {
  const state = running({ takes: [LIVE, SHOW, LINE] });

  assert.deepEqual(playableTakes(state), [
    { folder: SHOW.folder, name: "Show", label: "Show · 1:40" },
    { folder: LINE.folder, name: "Line check", label: "Line check · 1:02" },
  ]);
  assert.equal(soundcheckTarget(state, LINE.folder), LINE.folder, "the take chosen");
  assert.equal(soundcheckTarget(state, LIVE.folder), SHOW.folder, "never the take recording");
  const afterLine = running({ status: { lastTake: LINE }, takes: [SHOW, LINE] });
  assert.equal(soundcheckTarget(afterLine, null), LINE.folder, "else the last take");
  assert.equal(soundcheckTarget(state, null), SHOW.folder, "else the newest");
  assert.equal(soundcheckTarget(running({ takes: [] }), null), null, "none without takes");
});

test("with the inputs live the card offers the takes and says when a soundcheck cannot start", () => {
  const view = soundcheckView(running(), { chosen: LINE.folder });
  assert.equal(view.on, false);
  assert.equal(view.badge, "Off");
  assert.equal(view.chosen, LINE.folder);
  assert.equal(view.choices.length, 2);
  assert.equal(view.canStart, true);
  assert.equal(view.startHint, null);
  assert.equal(view.looping, false);
  assert.equal(view.canLoop, true, "looping can be chosen before the start");
  assert.equal(soundcheckView(running(), { loopNext: true }).looping, true);

  const idle = soundcheckView(reduce([actions.takes({ folder: ROOT, takes: [SHOW] })]));
  assert.equal(idle.canStart, false);
  assert.equal(idle.startHint, "Start the engine to play a take in place of the inputs.");
  const recording = soundcheckView(
    running({ status: { recording: { ...IDLE_RECORDING, state: "recording", takeFolder: LIVE.folder } } }),
  );
  assert.equal(recording.canStart, false);
  assert.equal(recording.startHint, RECORDING_BLOCKS_SOUNDCHECK);
  const empty = soundcheckView(running({ takes: [] }));
  assert.equal(empty.canStart, false);
  assert.match(empty.startHint, /^Record a take first/);
});

test("a take playing shows where it is, what paces it and the dropouts", () => {
  const state = running({ status: { soundcheck: soundcheckOn({ looping: true, clock: "output" }) } });

  const view = soundcheckView(state);
  assert.equal(view.on, true);
  assert.equal(view.badge, "Playing");
  assert.equal(view.takeName, "Show");
  assert.equal(view.position, "0:12");
  assert.equal(view.length, "1:40");
  assert.deepEqual(view.slider, { max: 100, value: 12 });
  assert.equal(view.canPause, true);
  assert.equal(view.canResume, false);
  assert.equal(view.canStop, true);
  assert.equal(view.looping, true);
  assert.equal(view.clock, "Paced by the local output.");
  assert.equal(view.underruns, "No dropouts.");
  assert.equal(view.failure, null);
  assert.equal(view.canStart, false, "one take at a time");

  const dragged = soundcheckView(state, { drag: 47.5 });
  assert.equal(dragged.position, "0:47", "the slider shows where it is being dragged to");
  assert.equal(dragged.slider.value, 47.5);

  const timed = soundcheckView(
    running({ status: { soundcheck: soundcheckOn({ clock: "timer", underrunFrames: 9600 }) } }),
  );
  assert.equal(timed.clock, "Paced by a timer: no local output plays.");
  assert.equal(timed.underruns, "200 ms of silence where the disk fell behind.");
});

test("a paused take, one at its end and one that failed say what is left to do", () => {
  const paused = soundcheckView(running({ status: { soundcheck: soundcheckOn({ state: "paused" }) } }));
  assert.equal(paused.badge, "Paused");
  assert.equal(paused.canPause, false);
  assert.equal(paused.canResume, true);
  assert.equal(paused.resumeWord, "Play");

  const end = soundcheckOn({ state: "paused", positionFrames: 100 * 48000, positionSeconds: 100 });
  const atEnd = soundcheckView(running({ status: { soundcheck: end } }));
  assert.equal(atEnd.badge, "At the end");
  assert.equal(atEnd.resumeWord, "Play again");

  const failed = soundcheckView(
    running({
      status: { soundcheck: soundcheckOn({ state: "failed", reason: 'reading "01 Kick.wav" failed' }) },
    }),
  );
  assert.equal(failed.badge, "Failed");
  assert.equal(failed.failure, 'The soundcheck stopped playing: reading "01 Kick.wav" failed.');
  assert.equal(failed.canPause, false);
  assert.equal(failed.canResume, false);
  assert.equal(failed.canLoop, false);
  assert.equal(failed.slider.disabled, true);
  assert.equal(failed.canStop, true, "going back to the live inputs is left");
});

test("the SOUNDCHECK chip shows on every screen while a take plays in place of the inputs", () => {
  assert.equal(soundcheckChip(running()), null, "the inputs are live");
  const playing = soundcheckChip(running({ status: { soundcheck: soundcheckOn() } }));
  assert.deepEqual(playing, {
    kind: "soundcheck",
    word: "SOUNDCHECK",
    time: "0:12",
    detail: "Show",
    label: 'Soundcheck: "Show" plays in place of the live inputs, at 12 seconds. Open Record',
  });
  const paused = soundcheckChip(running({ status: { soundcheck: soundcheckOn({ state: "paused" }) } }));
  assert.equal(paused.kind, "soundcheck-paused");
  assert.equal(paused.detail, "Show · paused");
  const failed = soundcheckChip(
    running({ status: { soundcheck: soundcheckOn({ state: "failed", reason: "the disk is gone" }) } }),
  );
  assert.equal(failed.kind, "soundcheck-failed");
  assert.equal(failed.word, "SOUNDCHECK stopped");
  assert.match(failed.label, /^The soundcheck stopped playing: the disk is gone\. /);
});

test("the transport plays the chosen take as a soundcheck, then pauses, resumes and goes back live", () => {
  const live = transportView(running(), { atStart: false, chosenTake: LINE.folder });
  assert.deepEqual(live.play, {
    can: true,
    action: "start",
    folder: LINE.folder,
    label: 'Play "Line check" in place of the live inputs',
  });
  assert.equal(live.stopLabel, "Stop recording");

  const playing = transportView(running({ status: { soundcheck: soundcheckOn() } }), {
    atStart: false,
  });
  assert.equal(playing.soundcheck, true);
  assert.equal(playing.canRecord, false, "nothing records during a soundcheck");
  assert.equal(playing.canStop, true);
  assert.equal(playing.stopLabel, "Back to live inputs");
  assert.equal(playing.position, "0:12");
  assert.equal(playing.stateWord, 'Soundcheck · "Show" plays in place of the inputs');
  assert.equal(playing.stateKind, "soundcheck");
  assert.deepEqual(playing.play, { can: true, action: "pause", folder: null, label: "Pause the soundcheck" });
  assert.equal(playing.canRewind, true, "Back to start moves the take to its start");
  assert.equal(
    recordMessages(running({ status: { soundcheck: soundcheckOn() } })).locked,
    SOUNDCHECK_BLOCKS_RECORDING,
    "the screen says why nothing records",
  );
  assert.equal(recordMessages(running()).locked, null);

  const paused = transportView(
    running({ status: { soundcheck: soundcheckOn({ state: "paused" }) } }),
    { atStart: false },
  );
  assert.equal(paused.play.action, "resume");
  assert.equal(paused.play.label, "Play the soundcheck on");

  const failed = transportView(
    running({ status: { soundcheck: soundcheckOn({ state: "failed", reason: "gone" }) } }),
    { atStart: false },
  );
  assert.equal(failed.play.can, false);
  assert.equal(failed.canStop, true);

  const nothing = transportView(running({ takes: [] }), { atStart: false });
  assert.equal(nothing.play.can, false);
  assert.equal(nothing.play.label, "Record a take to play it back");
  const recording = transportView(
    running({ status: { recording: { ...IDLE_RECORDING, state: "recording", takeFolder: LIVE.folder } } }),
    { atStart: false },
  );
  assert.equal(recording.play.can, false, "no soundcheck while a take records");
});

test("each take row can start its soundcheck, and the one playing says so", () => {
  const rows = takeRows(running({ status: { soundcheck: soundcheckOn() } }), 0);
  assert.deepEqual(rows[0].soundcheck, {
    playing: true,
    disabled: true,
    title: "This take plays in place of the live inputs now",
  });
  assert.equal(rows[1].soundcheck.disabled, true, "one take at a time");

  const recording = { ...IDLE_RECORDING, state: "recording", takeFolder: LIVE.folder };
  const live = takeRows(running({ status: { recording }, takes: [LIVE, SHOW] }), 0);
  assert.equal(live[0].soundcheck.disabled, true, "not the take being recorded");
  assert.equal(live[1].soundcheck.disabled, true, "nor any take while one records");
  const idle = takeRows(running(), 0);
  assert.deepEqual(idle[1].soundcheck, {
    playing: false,
    disabled: false,
    title: 'Play "Line check" in place of the live inputs',
  });
});
