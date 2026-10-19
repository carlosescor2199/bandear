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
