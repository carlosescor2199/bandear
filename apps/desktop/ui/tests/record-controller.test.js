// The Record screen's controller (record-controller.js), without a DOM: the transport's busy
// guard, the peak histories a new take clears, arming, the folder field kept after a refusal, and
// the shell's REC chip strip.
import test from "node:test";
import assert from "node:assert/strict";

import { createConsoleApp } from "../app/lib/console-app.js";
import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { FAKE_RECORDINGS_FOLDER, createFakeBridge, sampleStatus } from "../app/lib/fake-bridge.js";
import { createRecordController, mountRecChip } from "../app/lib/record-controller.js";
import { trackRows } from "../app/lib/record-view.js";
import { IDLE_RECORDING } from "../app/lib/recording.js";
import { createStore } from "../app/lib/store.js";

const SETTINGS = Object.freeze({
  folder: "D:\\Recordings",
  defaultFolder: FAKE_RECORDINGS_FOLDER,
  folderIsDefault: false,
  format: "int24",
  includeLocal: false,
  armed: [1],
  notice: null,
});

function statusRecording(takeFolder, state = "recording") {
  return { ...sampleStatus(), recording: { ...IDLE_RECORDING, state, takeFolder } };
}

/** A store the test fills itself, as the console app's would be. */
function storeApp() {
  const store = createStore(consoleReducer, initialState);
  return { store };
}

async function connected() {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  return { fake, app };
}

test("a transport command waits for its answer: another pressed meanwhile is not run", async () => {
  const busy = [];
  const controller = createRecordController(storeApp(), {
    onChange: () => busy.push(controller.local.busy),
  });

  let finish;
  const started = controller.command("start", () => new Promise((resolve) => (finish = resolve)));
  assert.equal(controller.local.busy, "start");
  let stopped = false;
  assert.equal(
    await controller.command("stop", () => {
      stopped = true;
    }),
    false,
    "Stop pressed while the start is on its way",
  );
  assert.equal(stopped, false);

  finish();
  assert.equal(await started, true);
  assert.equal(controller.local.busy, null);
  await assert.rejects(controller.command("mark", () => Promise.reject(new Error("lost"))));
  assert.equal(controller.local.busy, null, "a failed command frees the buttons too");
  assert.deepEqual(busy, ["start", null, "mark", null]);
});

test("the lanes keep the armed tracks' peaks while recording and a new take starts them empty", () => {
  const app = storeApp();
  const controller = createRecordController(app);
  const at = (status) => {
    app.store.dispatch(actions.status(status));
    app.store.dispatch(actions.recordingSettings(SETTINGS));
    return app.store.getState();
  };

  let state = at(statusRecording("D:\\Recordings\\take-1"));
  controller.local.atStart = true;
  assert.equal(controller.followTake(state), true, "a take folder seen first");
  assert.equal(controller.local.atStart, false, "the position counts from the new take");
  const rows = trackRows(state);
  const levels = controller.takeMeters(rows, { channels: [0.5, 0.25, 0, 0, 0, 0] });
  assert.deepEqual(levels.slice(0, 2), [0.5, 0.25]);
  controller.takeMeters(rows, { channels: [0.75, 0, 0, 0, 0, 0] });
  assert.equal(controller.history("in-1").length, 2);
  assert.equal(controller.history("in-1").at(1), 0.75);
  assert.equal(controller.history("in-2"), undefined, "an unarmed track keeps nothing");
  assert.equal(controller.laneOf(rows[0], state), "recording");
  assert.equal(controller.laneOf(rows[1], state), "off");

  assert.equal(controller.followTake(state), false, "the same take");
  assert.equal(controller.history("in-1").length, 2);

  // After the take the lane stays drawn; nothing more is kept while idle.
  state = at(sampleStatus());
  controller.takeMeters(rows, { channels: [0.9, 0, 0, 0, 0, 0] });
  assert.equal(controller.history("in-1").length, 2);
  assert.equal(controller.laneOf(rows[0], state), "recording");

  state = at(statusRecording("D:\\Recordings\\take-2"));
  assert.equal(controller.followTake(state), true);
  assert.equal(controller.history("in-1").length, 0, "the next take starts its lanes empty");
  state = at(sampleStatus());
  assert.equal(controller.laneOf(rows[0], state), "armed", "armed, nothing recorded yet");
});

test("the local mix lane reads the louder side of the local levels", () => {
  const app = storeApp();
  const controller = createRecordController(app);
  app.store.dispatch(actions.status(statusRecording("D:\\Recordings\\take-1")));
  const levels = controller.takeMeters(
    [{ key: "local", local: true, armed: true }],
    { channels: [], local: { left: 0.25, right: 0.75 } },
  );
  assert.deepEqual(levels, [0.75]);
  assert.equal(controller.history("local").at(0), 0.75);
});

test("two quick arm presses on two tracks both land", async () => {
  const { app } = await connected();
  const controller = createRecordController(app);

  const [kick, snare, unknown] = await Promise.all([
    controller.arm("in-1"),
    controller.arm("in-2"),
    controller.arm("in-99"),
  ]);
  assert.equal(kick.ok, true, kick.message);
  assert.equal(snare.ok, true, snare.message);
  assert.equal(unknown, null, "a row the screen does not show");
  assert.deepEqual(app.store.getState().recordingSettings.armed, [1, 2]);
  app.dispose();
});

test("a second press on the same row before the first is answered toggles it back", async () => {
  const { fake, app } = await connected();
  const controller = createRecordController(app);
  const asked = () =>
    fake.calls
      .filter(({ command }) => command === "set_recording_settings")
      .map(({ args }) => args.settings);

  const [on, off] = await Promise.all([controller.arm("in-1"), controller.arm("in-1")]);
  assert.equal(on.ok, true, on.message);
  assert.equal(off.ok, true, off.message);
  assert.deepEqual(
    asked().map((settings) => settings.armed),
    [[1], []],
    "the second press disarms what the first armed",
  );
  assert.deepEqual(app.store.getState().recordingSettings.armed, []);

  // Once answered, a press toggles from what the desk keeps again.
  const again = await controller.arm("in-1");
  assert.equal(again.ok, true, again.message);
  assert.deepEqual(app.store.getState().recordingSettings.armed, [1]);

  // The local mix toggles from its pending intent the same way.
  await Promise.all([controller.arm("local"), controller.arm("local")]);
  assert.deepEqual(
    asked()
      .slice(-2)
      .map((settings) => settings.includeLocal),
    [true, false],
  );
  assert.equal(app.store.getState().recordingSettings.includeLocal, false);
  app.dispose();
});

test("a settings request or listing that fails still renders again and says why", async () => {
  const app = storeApp();
  app.store.dispatch(actions.status(sampleStatus()));
  app.store.dispatch(actions.recordingSettings(SETTINGS));
  app.setRecordingSettings = () => Promise.reject(new Error("the bridge went away"));
  app.refreshTakes = () => Promise.reject(new Error("no listing"));
  let changes = 0;
  const controller = createRecordController(app, { onChange: () => (changes += 1) });

  const armed = await controller.arm("in-2");
  assert.equal(armed.ok, false);
  assert.match(armed.message, /the bridge went away/);
  assert.equal(changes, 1, "rendered again after the failure");
  assert.equal(app.store.getState().recordingOutcome.ok, false);
  assert.match(app.store.getState().recordingOutcome.message, /the bridge went away/);

  const folder = await controller.useFolder("D:\\Takes");
  assert.equal(folder.ok, false);
  assert.equal(changes, 2);
  const byDefault = await controller.useDefaultFolder();
  assert.equal(byDefault.ok, false);
  assert.equal(changes, 3);
  const format = await controller.changeSettings({ format: "float32" });
  assert.equal(format.ok, false);
  assert.equal(changes, 4);

  // A kept folder whose listing fails is still said kept, and the screen renders again.
  app.setRecordingSettings = () => Promise.resolve({ ok: true, message: "Saved." });
  const kept = await controller.useFolder("D:\\Takes");
  assert.equal(kept.ok, true);
  assert.equal(changes, 5);
  assert.match(app.store.getState().takes.error, /no listing/);
});

test("a refused folder stays in the field to be corrected, a kept one shows as the desk has it", async () => {
  const { fake, app } = await connected();
  const controller = createRecordController(app);
  const state = () => app.store.getState();
  const listings = () => fake.calls.filter(({ command }) => command === "list_takes").length;

  assert.equal(controller.folderToShow(state(), false), FAKE_RECORDINGS_FOLDER);
  assert.equal(controller.folderToShow(state(), false), null, "already shown");

  const refused = await controller.useFolder("Recordings");
  assert.equal(refused.ok, false);
  assert.equal(controller.folderToShow(state(), false), null, "the typed text stays");

  const listed = listings();
  const kept = await controller.useFolder("D:\\Takes");
  assert.equal(kept.ok, true, kept.message);
  assert.equal(listings(), listed + 1, "the new folder's takes are listed");
  assert.equal(controller.folderToShow(state(), true), null, "never while the engineer types");
  assert.equal(controller.folderToShow(state(), false), "D:\\Takes");

  await controller.useDefaultFolder();
  assert.equal(controller.folderToShow(state(), false), FAKE_RECORDINGS_FOLDER);
  app.dispose();
});

/** A console on a fake engine whose clock the test moves, with a two-second take recorded. */
async function withTake() {
  const time = { now: Date.UTC(2026, 9, 16, 19, 4, 5) };
  const fake = createFakeBridge({ running: true, clock: () => time.now });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1] });
  await app.startRecording("First");
  time.now += 2_000;
  const first = (await app.stopRecording()).take;
  await app.startRecording("Second");
  time.now += 2_000;
  const second = (await app.stopRecording()).take;
  return { fake, time, app, first, second };
}

const soundcheckOf = (app) => app.store.getState().engine.status.soundcheck;

test("Play starts the chosen take as a soundcheck, pauses and resumes it; Stop goes back live", async () => {
  const { fake, app, first, second } = await withTake();
  const controller = createRecordController(app);

  controller.chooseTake(first.folder);
  const started = await controller.play();
  assert.equal(started.ok, true, started.message);
  assert.equal(soundcheckOf(app).takeFolder, first.folder, "the chosen take, not the last");
  assert.equal(soundcheckOf(app).state, "playing");

  assert.equal((await controller.play()).ok, true);
  assert.equal(soundcheckOf(app).state, "paused", "Play pauses a take playing");
  assert.equal((await controller.play()).ok, true);
  assert.equal(soundcheckOf(app).state, "playing", "and plays a paused one on");

  const recordsBefore = fake.calls.filter(({ command }) => command === "stop_recording").length;
  await controller.stop();
  assert.equal(soundcheckOf(app).state, "off", "back to the live inputs");
  assert.equal(
    fake.calls.filter(({ command }) => command === "stop_recording").length,
    recordsBefore,
    "no recording was stopped",
  );

  controller.chooseTake(null);
  await controller.play();
  assert.equal(soundcheckOf(app).takeFolder, second.folder, "with none chosen, the last take");
  await controller.stop();
  const fromRow = await controller.soundcheckTake(first.folder);
  assert.equal(fromRow.ok, true, fromRow.message);
  assert.equal(soundcheckOf(app).takeFolder, first.folder, "a take row's Soundcheck plays that take");
  app.dispose();
});

test("Back to start and the slider move the take, and a loop chosen before the start loops", async () => {
  const { fake, time, app, first } = await withTake();
  let changes = 0;
  const controller = createRecordController(app, { onChange: () => (changes += 1) });

  await controller.setLoop(true);
  assert.equal(controller.local.loopNext, true, "kept for the start");
  await controller.soundcheckTake(first.folder);
  const [start] = fake.calls.filter(({ command }) => command === "start_soundcheck");
  assert.equal(start.args.looping, true);

  time.now += 1_000;
  controller.drag(1.5);
  assert.equal(controller.local.drag, 1.5, "the slider shows where it is dragged to");
  const moved = await controller.seekTo(1.5);
  assert.equal(moved.ok, true, moved.message);
  assert.equal(controller.local.drag, null);
  assert.equal(soundcheckOf(app).positionFrames, 72_000);

  await controller.rewind();
  assert.equal(soundcheckOf(app).positionFrames, 0, "Back to start moves the take to its start");
  await controller.setLoop(false);
  assert.equal(soundcheckOf(app).looping, false, "the take playing stops looping");
  assert.ok(changes >= 5, "each change renders again");
  app.dispose();
});

/** Delivers the fake's held answers until every one of `pending` is settled; their results. */
async function released(fake, pending) {
  let settled = false;
  const all = Promise.all(pending).finally(() => (settled = true));
  for (let turn = 0; turn < 50 && !settled; turn += 1) {
    fake.releaseAnswers();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  return all;
}

const sentOf = (fake, name) => fake.calls.filter(({ command }) => command === name).length;

test("a move or loop pressed while Stop goes back live is not sent, so the stop's words stay", async () => {
  const { fake, app, first } = await withTake();
  const controller = createRecordController(app);
  await controller.soundcheckTake(first.folder);
  for (const name of ["stop_soundcheck", "seek_soundcheck", "set_soundcheck_loop"]) {
    fake.holdAnswers(name);
  }

  const stopping = controller.stop();
  assert.equal(controller.local.busy, "stop");
  const pressed = [controller.seekTo(1), controller.rewind(), controller.setLoop(true)];
  const [stopped, ...answers] = await released(fake, [stopping, ...pressed]);

  assert.deepEqual(answers, [null, null, null], "each was pressed while Stop was on its way");
  assert.equal(sentOf(fake, "seek_soundcheck"), 0, "no move went out");
  assert.equal(sentOf(fake, "set_soundcheck_loop"), 0, "nor the loop");
  assert.equal(stopped.ok, true, stopped.message);
  assert.deepEqual(app.store.getState().recordingOutcome, {
    action: "soundcheck",
    ok: true,
    message: "Back to the live inputs.",
  });
  assert.equal(soundcheckOf(app).state, "off");
  assert.equal(controller.local.busy, null);
  app.dispose();
});

test("moves on their way hold the other transport commands but not each other", async () => {
  const { fake, app, first } = await withTake();
  const controller = createRecordController(app);
  await controller.soundcheckTake(first.folder);
  fake.holdAnswers("seek_soundcheck");

  const moves = [controller.seekTo(0.5), controller.seekTo(1)];
  assert.equal(controller.local.busy, "move");
  assert.equal(await controller.stop(), null, "Stop waits for the moves");
  assert.equal(await controller.setLoop(true), null, "and so does the loop");
  const answers = await released(fake, moves);

  assert.ok(answers.every((answer) => answer.ok), "both moves were answered");
  assert.equal(soundcheckOf(app).positionFrames, 48_000, "the latest move wins");
  assert.equal(soundcheckOf(app).state, "playing", "nothing stopped it");
  assert.equal(sentOf(fake, "set_soundcheck_loop"), 0);
  assert.equal(controller.local.busy, null, "the buttons are free once both are answered");
  app.dispose();
});

test("the REC chip strip shows while a take records, is finished or failed, hidden otherwise", () => {
  const app = storeApp();
  const chip = { data: undefined };
  const strip = { hidden: false, querySelector: (selector) => (selector === "ml-rec-chip" ? chip : null) };

  const unsubscribe = mountRecChip(strip, app);
  assert.equal(strip.hidden, true, "no take");
  assert.equal(chip.data, null);

  app.store.dispatch(actions.status(statusRecording("D:\\Recordings\\take-1")));
  assert.equal(strip.hidden, false);
  assert.equal(chip.data.kind, "recording");
  app.store.dispatch(actions.status(statusRecording("D:\\Recordings\\take-1", "stopping")));
  assert.equal(chip.data.kind, "stopping");
  app.store.dispatch(actions.status(statusRecording("D:\\Recordings\\take-1", "failed")));
  assert.equal(strip.hidden, false);
  assert.equal(chip.data.kind, "failed");
  app.store.dispatch(actions.status(sampleStatus()));
  assert.equal(strip.hidden, true);

  unsubscribe();
  app.store.dispatch(actions.status(statusRecording("D:\\Recordings\\take-2")));
  assert.equal(strip.hidden, true, "nothing follows the store once unsubscribed");
});

test("the strip shows the SOUNDCHECK chip on every screen while a take plays in place of the inputs", () => {
  const app = storeApp();
  const rec = { data: undefined };
  const soundcheck = { data: undefined };
  const chips = { '[data-chip="rec"]': rec, '[data-chip="soundcheck"]': soundcheck };
  const strip = { hidden: false, querySelector: (selector) => chips[selector] ?? null };

  mountRecChip(strip, app);
  assert.equal(strip.hidden, true);
  app.store.dispatch(
    actions.status({
      ...sampleStatus(),
      soundcheck: {
        ...sampleStatus().soundcheck,
        state: "playing",
        takeFolder: "D:\\Recordings\\take-1",
        takeName: "Show",
      },
    }),
  );
  assert.equal(strip.hidden, false);
  assert.equal(rec.data, null, "no take records");
  assert.equal(soundcheck.data.kind, "soundcheck");
  assert.equal(soundcheck.data.word, "SOUNDCHECK");
  app.store.dispatch(actions.status(sampleStatus()));
  assert.equal(strip.hidden, true, "back to the live inputs");
});
