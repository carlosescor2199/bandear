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
