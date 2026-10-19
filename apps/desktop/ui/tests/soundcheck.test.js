// The virtual soundcheck in the console: the engine api's commands, the store and its selectors,
// the console app's commands and refusals, and the fake engine mirroring the desktop's rules.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import {
  actions,
  consoleReducer,
  initialState,
  isSoundcheckNow,
  soundcheckStatus,
} from "../app/lib/console-state.js";
import { FAKE_RECORDINGS_FOLDER, createFakeBridge, sampleStatus } from "../app/lib/fake-bridge.js";
import { audioSetupView } from "../app/lib/network-view.js";
import {
  IDLE_SOUNDCHECK,
  RECORDING_BLOCKS_SOUNDCHECK,
  SOUNDCHECK_BLOCKS_RECORDING,
  SOUNDCHECK_BLOCKS_SWITCH,
  formatSoundcheckDone,
  frameAt,
  isSoundchecking,
  soundcheckOf,
} from "../app/lib/soundcheck.js";

// 2026-10-16 19:04:05 UTC.
const START = Date.UTC(2026, 9, 16, 19, 4, 5);

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
}

/** Lets the console's promises run until `done` holds (a few event-loop turns at most). */
async function until(done) {
  for (let turn = 0; turn < 50 && !done(); turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** A fake running engine whose clock the test moves. */
function clocked(options = {}) {
  const time = { now: START };
  const fake = createFakeBridge({ running: true, clock: () => time.now, ...options });
  return { fake, time };
}

/** Reads the fake's status into the app, as the next `status` event would bring it. */
async function statusNow(app, fake) {
  app.store.dispatch(actions.status(await fake.invoke("engine_status")));
  return soundcheckStatus(app.store.getState());
}

/** A console on a fake engine with a three-second take of CH 1 and CH 2 recorded. */
async function withTake(options = {}) {
  const { fake, time } = clocked(options);
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1, 2] });
  await app.startRecording("Show");
  time.now += 3_000;
  const stopped = await app.stopRecording();
  assert.equal(stopped.ok, true, stopped.message);
  return { fake, time, app, take: stopped.take };
}

/** A take as `list_takes` lists it, of `tracks`, three seconds long. */
function savedTake(name, tracks, extra = {}) {
  return {
    folder: `${FAKE_RECORDINGS_FOLDER}\\2026-10-15_200000_${name}`,
    name,
    started: "2026-10-15T18:00:00Z",
    sampleRate: 48000,
    format: "int24",
    device: "Demo interface",
    tracks: tracks.map((track) => ({ instrument: "other", frames: 144_000, ...track })),
    frames: 144_000,
    seconds: 3,
    markers: [],
    droppedFrames: 0,
    ended: "stopped",
    reason: null,
    ...extra,
  };
}

test("the engine api sends the soundcheck commands with the desktop's argument names", async () => {
  const sent = [];
  const api = createEngineApi({
    invoke: (command, args) => {
      sent.push([command, args]);
      return Promise.resolve(null);
    },
    listen: () => Promise.resolve(() => {}),
  });

  await api.startSoundcheck("D:\\Takes\\2026-10-16_210405_Show", 96000, true);
  await api.pauseSoundcheck();
  await api.resumeSoundcheck();
  await api.seekSoundcheck(48000);
  await api.setSoundcheckLoop(false);
  await api.stopSoundcheck();

  assert.deepEqual(sent, [
    [
      "start_soundcheck",
      { takeFolder: "D:\\Takes\\2026-10-16_210405_Show", startFrame: 96000, looping: true },
    ],
    ["pause_soundcheck", undefined],
    ["resume_soundcheck", undefined],
    ["seek_soundcheck", { frame: 48000 }],
    ["set_soundcheck_loop", { looping: false }],
    ["stop_soundcheck", undefined],
  ]);
});

test("the soundcheck's shapes, frames and words", () => {
  assert.equal(soundcheckOf(null), IDLE_SOUNDCHECK);
  assert.equal(soundcheckOf({ soundcheck: { state: "paused" } }).state, "paused");
  assert.equal(isSoundchecking(IDLE_SOUNDCHECK), false);
  for (const state of ["playing", "paused", "failed"]) {
    assert.equal(isSoundchecking({ state }), true, state);
  }
  assert.equal(frameAt(1.5, 144_000), 72_000);
  assert.equal(frameAt(9, 144_000), 144_000, "never past the end");
  assert.equal(frameAt(-1, 144_000), 0);
  const playing = { takeName: "Show", positionSeconds: 62, looping: true };
  assert.equal(
    formatSoundcheckDone("start", playing),
    'Soundcheck: "Show" plays in place of the live inputs from 1:02, looping.',
  );
  assert.equal(formatSoundcheckDone("pause", playing), "Soundcheck paused at 1:02.");
  assert.equal(formatSoundcheckDone("stop", IDLE_SOUNDCHECK), "Back to the live inputs.");
});

test("the store shows the soundcheck of the status and takes a command's answer for the shown run", () => {
  let state = initialState;
  assert.equal(soundcheckStatus(state), IDLE_SOUNDCHECK, "no engine: the inputs are live");
  assert.equal(isSoundcheckNow(state), false);

  state = consoleReducer(state, actions.status(sampleStatus()));
  assert.equal(soundcheckStatus(state).state, "off");
  const playing = { ...IDLE_SOUNDCHECK, state: "playing", takeName: "Show", lengthFrames: 48_000 };
  state = consoleReducer(state, actions.soundcheck(playing, state.engine.run));
  assert.equal(soundcheckStatus(state).state, "playing", "the answer shows at once");
  assert.equal(isSoundcheckNow(state), true);

  const ended = consoleReducer(state, actions.soundcheck(IDLE_SOUNDCHECK, state.engine.run + 5));
  assert.equal(soundcheckStatus(ended).state, "playing", "an answer of another run is not taken");
});

test("a take plays in place of the inputs from its start, moves, pauses, loops and goes back live", async () => {
  const { fake, time, app, take } = await withTake();

  const started = await app.startSoundcheck(take.folder);
  assert.equal(started.ok, true, started.message);
  assert.equal(started.message, 'Soundcheck: "Show" plays in place of the live inputs from 0:00.');
  const [request] = commands(fake, "start_soundcheck");
  assert.deepEqual(request.args, { takeFolder: take.folder, startFrame: 0, looping: false });
  let soundcheck = soundcheckStatus(app.store.getState());
  assert.equal(soundcheck.state, "playing");
  assert.equal(soundcheck.takeName, "Show");
  assert.equal(soundcheck.takeFolder, take.folder);
  assert.equal(soundcheck.lengthFrames, 144_000);
  assert.equal(soundcheck.clock, "timer", "no local output plays");

  time.now += 1_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.positionFrames, 48_000, "the position advances with the clock");
  assert.equal(soundcheck.positionSeconds, 1);

  const moved = await app.seekSoundcheck(96_000);
  assert.equal(moved.ok, true, moved.message);
  assert.equal(moved.message, "Soundcheck moved to 0:02.");
  const paused = await app.pauseSoundcheck();
  assert.equal(paused.message, "Soundcheck paused at 0:02.");
  time.now += 5_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "paused");
  assert.equal(soundcheck.positionFrames, 96_000, "a paused take holds its position");

  await app.resumeSoundcheck();
  time.now += 2_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "paused", "the end of a take that does not loop pauses it");
  assert.equal(soundcheck.positionFrames, 144_000, "at its end");
  const again = await app.resumeSoundcheck();
  assert.equal(again.message, "Soundcheck playing from 0:00.", "Play starts it again");

  const looping = await app.setSoundcheckLoop(true);
  assert.equal(looping.message, "The soundcheck starts the take again at its end.");
  time.now += 4_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "playing");
  assert.equal(soundcheck.positionFrames, 48_000, "four seconds into a three-second loop");

  const live = await app.stopSoundcheck();
  assert.equal(live.ok, true, live.message);
  assert.equal(live.message, "Back to the live inputs.");
  assert.equal(soundcheckStatus(app.store.getState()).state, "off");
  assert.equal(isSoundcheckNow(app.store.getState()), false);
  app.dispose();
});

test("a seek plays a take stopped at its end and keeps a paused one paused, as the engine does", async () => {
  const { fake, time, app, take } = await withTake();
  await app.startSoundcheck(take.folder);
  time.now += 4_000;
  let soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "paused", "stopped at its end");
  assert.equal(soundcheck.positionFrames, 144_000);

  const fromEnd = await app.seekSoundcheck(48_000);
  assert.equal(fromEnd.soundcheck.state, "playing", "a seek from the end plays at once");
  assert.equal(fromEnd.soundcheck.positionFrames, 48_000);
  time.now += 1_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "playing");
  assert.equal(soundcheck.positionFrames, 96_000, "and plays on from the new frame");

  await app.pauseSoundcheck();
  const whilePaused = await app.seekSoundcheck(24_000);
  assert.equal(whilePaused.soundcheck.state, "paused", "a take the engineer paused stays paused");
  time.now += 1_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "paused");
  assert.equal(soundcheck.positionFrames, 24_000, "held at the new frame");

  const toEnd = await app.seekSoundcheck(144_000);
  assert.equal(toEnd.soundcheck.state, "paused", "moved to its end while paused, it stays paused");
  const again = await app.resumeSoundcheck();
  assert.equal(again.message, "Soundcheck playing from 0:00.", "played on from its end, it starts again");
  time.now += 1_000;
  soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.state, "playing");
  assert.equal(soundcheck.positionFrames, 48_000);
  app.dispose();
});

test("a soundcheck can start from a frame, looping, and plays on the local output's clock", async () => {
  const { fake, app, take } = await withTake();
  const output = await app.setLocalOutput({ device: "Speakers (Realtek Audio)", firstChannel: 0 });
  assert.equal(output.ok, true, output.message);

  const started = await app.startSoundcheck(take.folder, { startFrame: 72_000, looping: true });

  assert.equal(started.ok, true, started.message);
  assert.match(started.message, /from 0:01, looping\.$/);
  const soundcheck = await statusNow(app, fake);
  assert.equal(soundcheck.positionFrames, 72_000);
  assert.equal(soundcheck.looping, true);
  assert.equal(soundcheck.clock, "output", "the local output plays, so it paces the take");
  app.dispose();
});

test("while a soundcheck plays nothing records and the input device stays, saying why", async () => {
  const { fake, app, take } = await withTake();
  await app.startSoundcheck(take.folder);
  const startsBefore = commands(fake, "start_engine").length;
  const recordsBefore = commands(fake, "start_recording").length;

  const recorded = await app.startRecording("Late");
  await app.switchDevice("Microphone (USB audio)");
  const changed = await app.applyAudio({ bufferFrames: 128 }, { keepDevice: true });

  assert.equal(recorded.ok, false);
  assert.equal(recorded.message, SOUNDCHECK_BLOCKS_RECORDING);
  assert.equal(commands(fake, "start_recording").length, recordsBefore, "no start went out");
  assert.equal(commands(fake, "switch_device").length, 0, "nor a switch");
  assert.equal(commands(fake, "start_engine").length, startsBefore, "nor a restart");
  assert.deepEqual(app.store.getState().commandError, {
    action: "switch",
    message: SOUNDCHECK_BLOCKS_SWITCH,
  });
  assert.deepEqual(changed, { ok: false, error: SOUNDCHECK_BLOCKS_SWITCH });
  const setup = audioSetupView(app.store.getState(), {});
  assert.equal(setup.recordingLock, SOUNDCHECK_BLOCKS_SWITCH, "the audio setup says why");
  assert.equal(setup.deviceDisabled, true);
  assert.equal(setup.hostDisabled, true);
  assert.equal(setup.bufferDisabled, true);

  await assert.rejects(
    fake.invoke("start_recording", { takeName: "Late" }),
    /go back to the live inputs before recording/,
    "the engine refuses a take too",
  );
  await assert.rejects(
    fake.invoke("switch_device", { device: "Microphone (USB audio)" }),
    /go back to the live inputs before switching the input device/,
    "and a switch",
  );
  app.dispose();
});

test("a take recording leaves no soundcheck to start, saying why", async () => {
  const { fake, app, take } = await withTake();
  await app.startRecording("Second");

  const refused = await app.startSoundcheck(take.folder);

  assert.equal(refused.ok, false);
  assert.equal(refused.message, RECORDING_BLOCKS_SOUNDCHECK);
  assert.equal(commands(fake, "start_soundcheck").length, 0, "nothing went out");
  await assert.rejects(
    fake.invoke("start_soundcheck", { takeFolder: take.folder }),
    /a take is recording: stop the recording before the soundcheck/,
    "the engine refuses it too",
  );
  app.dispose();
});

test("the fake refuses what the desk refuses, in the desk's words", async () => {
  const pair = savedTake("Paired", [
    { file: "01-02 Kick.wav", kind: "pair", channels: [1, 2], name: "Kick" },
  ]);
  const wide = savedTake("Wide", [{ file: "09 CH 9.wav", kind: "input", channels: [9], name: "CH 9" }]);
  const empty = savedTake("Mix only", [
    { file: "Local mix.wav", kind: "local", channels: [], name: "Local mix" },
  ]);
  const slow = savedTake("Slow", [{ file: "01 Kick.wav", kind: "input", channels: [1], name: "Kick" }], {
    sampleRate: 44100,
  });
  const { fake, app, take } = await withTake({ savedTakes: [pair, wide, empty, slow] });
  await app.refreshTakes();
  const start = (folder, startFrame = 0) => app.startSoundcheck(folder, { startFrame });

  for (const [folder, said] of [
    [
      "D:\\Elsewhere\\2026-10-16_190000_Show",
      `The soundcheck did not start: D:\\Elsewhere\\2026-10-16_190000_Show is not a take folder in the recordings folder ${FAKE_RECORDINGS_FOLDER}.`,
    ],
    [
      pair.folder,
      "The soundcheck did not start: the take does not fit this device's inputs now: CH 1-2 were recorded as a linked pair, which they are not now; play it on the device and with the linked pairs it was recorded with.",
    ],
    [
      wide.folder,
      "The soundcheck did not start: the take does not fit this device's inputs now: the take recorded CH 9, and this device has 6 inputs; play it on the device and with the linked pairs it was recorded with.",
    ],
    [empty.folder, "The soundcheck did not start: the take has no input audio to play."],
    [
      slow.folder,
      "The soundcheck did not start: the take was recorded at 44100 Hz; only 48000 Hz takes play (there is no resampler).",
    ],
  ]) {
    const refused = await start(folder);
    assert.equal(refused.ok, false, folder);
    assert.equal(refused.message, said);
  }
  const past = await start(take.folder, 200_000);
  assert.equal(
    past.message,
    "The soundcheck did not start: 4.2 s is past the end of the take (3.0 s).",
  );
  assert.equal(soundcheckStatus(app.store.getState()).state, "off", "the inputs stay live");

  const nothing = await app.pauseSoundcheck();
  assert.equal(nothing.message, "The soundcheck was not paused: no soundcheck is playing.");
  await app.startSoundcheck(take.folder);
  const twice = await app.startSoundcheck(take.folder);
  assert.equal(
    twice.message,
    "The soundcheck did not start: a soundcheck is already playing a take: go back to the live inputs first.",
  );
  const beyond = await app.seekSoundcheck(500_000);
  assert.equal(
    beyond.message,
    "The soundcheck did not move: 10.4 s is past the end of the take (3.0 s).",
  );
  app.dispose();
});

test("a take whose reading fails says why, refuses the transport and still goes back live", async () => {
  const { fake, time, app, take } = await withTake();
  await app.startSoundcheck(take.folder);
  time.now += 1_000;

  fake.failSoundcheck('reading "01 Kick.wav" failed');
  time.now += 1_000;
  const soundcheck = await statusNow(app, fake);

  assert.equal(soundcheck.state, "failed");
  assert.equal(soundcheck.reason, 'reading "01 Kick.wav" failed');
  assert.equal(soundcheck.positionFrames, 48_000, "the position held where it failed");
  const refused = await app.resumeSoundcheck();
  assert.equal(
    refused.message,
    'The soundcheck did not play on: the soundcheck stopped playing: reading "01 Kick.wav" failed; go back to the live inputs to stop it.',
  );
  const live = await app.stopSoundcheck();
  assert.equal(live.ok, true, live.message);
  assert.equal(soundcheckStatus(app.store.getState()).state, "off");
  app.dispose();
});

test("an engine that stops ends the soundcheck; a quick run of seeks sends the first and the last", async () => {
  const { fake, app, take } = await withTake();
  await app.startSoundcheck(take.folder);

  fake.holdAnswers("seek_soundcheck");
  const sent = () => commands(fake, "seek_soundcheck").length;
  const seeks = [app.seekSoundcheck(12_000), app.seekSoundcheck(24_000), app.seekSoundcheck(36_000)];
  await until(() => sent() === 1);
  fake.releaseAnswers();
  await until(() => sent() === 2);
  fake.releaseAnswers();
  const answers = await Promise.all(seeks);
  assert.deepEqual(
    commands(fake, "seek_soundcheck").map(({ args }) => args.frame),
    [12_000, 36_000],
    "a seek overtaken before it went out never goes",
  );
  assert.ok(answers.every((answer) => answer.ok));
  assert.equal(soundcheckStatus(app.store.getState()).positionFrames, 36_000);

  await app.stop();
  const restarted = await fake.invoke("engine_status");
  assert.equal(restarted.state, "notRunning");
  await app.start();
  assert.equal(soundcheckStatus(app.store.getState()).state, "off", "a new engine's inputs are live");
  app.dispose();
});
