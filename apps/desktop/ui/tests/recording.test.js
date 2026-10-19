import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import {
  actions,
  consoleReducer,
  initialState,
  isRecordingNow,
  lastTake,
  recordingStatus,
} from "../app/lib/console-state.js";
import {
  FAKE_CONFIG_FOLDER,
  FAKE_RECORDINGS_FOLDER,
  createFakeBridge,
  sampleStatus,
} from "../app/lib/fake-bridge.js";
import {
  IDLE_RECORDING,
  RECORDING_BLOCKS_SWITCH,
  armedAfter,
  formatDuration,
  formatStarted,
  formatStopped,
  formatUnreadable,
  isArmed,
  isRecording,
  recordUnits,
  settingsRequest,
} from "../app/lib/recording.js";

// 2026-10-16 19:04:05 UTC.
const START = Date.UTC(2026, 9, 16, 19, 4, 5);

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
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
}

const LINKED = [{ link: false }, { link: true }, { link: false }, { link: false }];

test("the engine api sends the recording commands with the desktop's argument names", async () => {
  const sent = [];
  const api = createEngineApi({
    invoke: (command, args) => {
      sent.push([command, args]);
      return Promise.resolve(null);
    },
    listen: () => Promise.resolve(() => {}),
  });

  await api.recordingSettings();
  await api.setRecordingSettings({ folder: null, format: "int24", includeLocal: false, armed: [1] });
  await api.startRecording("Show", 120);
  await api.stopRecording();
  await api.addMarker("Chorus");
  await api.recordEstimate();
  await api.listTakes();
  await api.openTakeFolder("D:\\Takes\\2026-10-16_210405_Show");

  assert.deepEqual(sent, [
    ["recording_settings", undefined],
    [
      "set_recording_settings",
      { settings: { folder: null, format: "int24", includeLocal: false, armed: [1] } },
    ],
    ["start_recording", { takeName: "Show", utcOffsetMinutes: 120 }],
    ["stop_recording", undefined],
    ["add_marker", { name: "Chorus" }],
    ["record_estimate", undefined],
    ["list_takes", undefined],
    ["open_take_folder", { folder: "D:\\Takes\\2026-10-16_210405_Show" }],
  ]);
});

test("either side of a linked pair arms the pair, and disarming a side disarms both", () => {
  assert.deepEqual(recordUnits(LINKED), [[0], [1, 2], [3]]);
  assert.equal(isArmed([3], 2, LINKED), true, "CH 3 arms the pair CH 2-3");
  assert.equal(isArmed([3], 4, LINKED), false);
  assert.deepEqual(armedAfter([1, 3], 2, false, LINKED), [1], "both sides go");
  assert.deepEqual(armedAfter([4], 1, true, LINKED), [1, 4]);
  assert.deepEqual(armedAfter([4], 4, true, LINKED), [4], "once each");
  assert.deepEqual(armedAfter(null, 2, true, []), [2], "an input the status does not list");
});

test("a settings request is the whole settings with the change on top, the default folder null", () => {
  assert.deepEqual(settingsRequest(null), {
    folder: null,
    format: "int24",
    includeLocal: false,
    armed: [],
  });
  const kept = {
    folder: "D:\\Takes",
    defaultFolder: FAKE_RECORDINGS_FOLDER,
    folderIsDefault: false,
    format: "float32",
    includeLocal: true,
    armed: [1, 2],
    notice: null,
  };
  assert.deepEqual(settingsRequest(kept, { armed: [5] }), {
    folder: "D:\\Takes",
    format: "float32",
    includeLocal: true,
    armed: [5],
  });
  assert.equal(settingsRequest(kept, { folder: "  " }).folder, null, "blank is the default");
  assert.equal(settingsRequest({ ...kept, folderIsDefault: true }).folder, null);
});

test("lengths, starts, stops and unreadable takes are said in words", () => {
  assert.equal(formatDuration(7.9), "0:07");
  assert.equal(formatDuration(205), "3:25");
  assert.equal(formatDuration(3723), "1:02:03");
  assert.equal(
    formatStarted(
      { recording: { tracks: ["Kick", "Local mix"] }, folder: "D:\\Takes", skipped: [9, 10] },
      "Show",
    ),
    'Recording "Show" on 2 tracks in D:\\Takes. CH 9, CH 10 are armed but not on this device, ' +
      "so not recorded.",
  );
  assert.equal(
    formatStopped({ name: "Show", ended: "stopped", seconds: 205, tracks: [{}], droppedFrames: 0 }),
    '"Show" stopped: 3:25 on 1 track.',
  );
  assert.equal(
    formatStopped({
      name: "Show",
      ended: "diskError",
      reason: "the disk is full.",
      seconds: 4,
      tracks: [{}, {}],
      droppedFrames: 256,
    }),
    '"Show" stopped by a disk error: the disk is full: 0:04 on 2 tracks. 256 frames the disk ' +
      "could not keep up with are missing.",
  );
  assert.equal(formatUnreadable({ unreadable: 0 }), null);
  assert.equal(formatUnreadable({ unreadable: 1 }), "1 take could not be read and is not listed.");
});

test("the store keeps the recording settings and takes past a run's end, the estimate not", () => {
  const settings = { folder: "D:\\Takes", folderIsDefault: false, armed: [1] };
  const takes = { folder: "D:\\Takes", takes: [{ name: "Show" }], unreadable: 2, missing: false };
  const recording = { ...IDLE_RECORDING, state: "recording", frames: 48_000, seconds: 1 };
  const status = { ...sampleStatus(), recording, lastTake: { name: "Earlier" } };

  const state = reduce([
    actions.status(status),
    actions.recordingSettings(settings),
    actions.takes(takes),
    actions.estimate({ bytesPerSecond: 144_000 }),
    actions.recordingOutcome("start", true, "Recording."),
  ]);

  assert.equal(isRecordingNow(state), true);
  assert.equal(recordingStatus(state).seconds, 1);
  assert.equal(lastTake(state).name, "Earlier");
  assert.deepEqual(state.takes, {
    list: [{ name: "Show" }],
    folder: "D:\\Takes",
    unreadable: 2,
    missing: false,
    error: null,
  });
  assert.deepEqual(state.recordingOutcome, { action: "start", ok: true, message: "Recording." });

  const failed = reduce([actions.takesFailed("the folder could not be read")], state);
  assert.deepEqual(failed.takes.list, [{ name: "Show" }], "the last list known stays");
  assert.equal(failed.takes.error, "the folder could not be read");

  const ended = reduce([actions.engine("idle")], state);
  assert.equal(ended.recordingSettings, settings);
  assert.equal(ended.takes.list.length, 1);
  assert.equal(ended.estimate, null);
  assert.deepEqual(recordingStatus(ended), IDLE_RECORDING);
  assert.equal(isRecording(recordingStatus(ended)), false);
  assert.equal(lastTake(ended), null);
  assert.equal(reduce([actions.recordingOutcome(null)], state).recordingOutcome, null);
});

test("the console reads the recording settings once connected and two quick changes both land", async () => {
  const { fake } = clocked();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  assert.deepEqual(app.store.getState().recordingSettings, {
    folder: FAKE_RECORDINGS_FOLDER,
    defaultFolder: FAKE_RECORDINGS_FOLDER,
    folderIsDefault: true,
    format: "int24",
    includeLocal: false,
    armed: [],
    notice: null,
  });

  const [armed, local] = await Promise.all([
    app.setRecordingSettings({ armed: [2, 1] }),
    app.setRecordingSettings({ includeLocal: true }),
  ]);

  assert.equal(armed.ok, true);
  assert.equal(local.ok, true);
  const settings = app.store.getState().recordingSettings;
  assert.deepEqual(settings.armed, [1, 2], "the first change is not lost to the second");
  assert.equal(settings.includeLocal, true);
  assert.deepEqual(fake.savedRecording(), {
    folder: null,
    format: "int24",
    includeLocal: true,
    armed: [1, 2],
  });

  const refused = await app.setRecordingSettings({ folder: "Recordings" });
  assert.equal(refused.ok, false);
  assert.match(refused.message, /^The recording settings were not changed: .*whole path/);
  assert.equal(app.store.getState().recordingSettings.folderIsDefault, true, "nothing changed");
  assert.deepEqual(app.store.getState().recordingOutcome, {
    action: "settings",
    ok: false,
    message: refused.message,
  });
  const next = await app.setRecordingSettings({ format: "float32" });
  assert.equal(next.ok, true, "a refusal never stops a later change");
  assert.equal(app.store.getState().recordingSettings.format, "float32");
  assert.deepEqual(app.store.getState().recordingSettings.armed, [1, 2]);
  app.dispose();
});

test("the fake refuses the settings the desk refuses", async () => {
  const fake = createFakeBridge();
  const settings = (change) => ({ folder: null, format: "int24", includeLocal: false, armed: [], ...change });

  for (const [change, word] of [
    [{ folder: "Takes" }, "whole path"],
    [{ folder: "D:\\Shows\\..\\Takes" }, ".."],
    [{ folder: `${FAKE_CONFIG_FOLDER}\\Takes` }, "settings folder"],
    [{ format: "mp3" }, "int24"],
    [{ armed: [0] }, "CH 0"],
    [{ armed: [256] }, "CH 256"],
    [{ armd: [1] }, "armd"],
  ]) {
    await assert.rejects(
      fake.invoke("set_recording_settings", { settings: settings(change) }),
      (error) => String(error).includes(word),
      word,
    );
  }
  const kept = await fake.invoke("set_recording_settings", {
    settings: settings({ folder: ` ${FAKE_RECORDINGS_FOLDER} `, armed: [3, 1, 3] }),
  });
  assert.equal(kept.folderIsDefault, true, "the default typed out is the default");
  assert.deepEqual(kept.armed, [1, 3]);
});

test("a take records the armed inputs, grows with time, takes markers and stops into the takes list", async () => {
  const { fake, time } = clocked();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1, 2, 9], includeLocal: true });

  const started = await app.startRecording("Soundcheck");

  assert.equal(started.ok, true, started.message);
  assert.match(started.message, /^Recording "Soundcheck" on 3 tracks in /);
  assert.match(started.message, /CH 9 is armed but not on this device/);
  const [request] = commands(fake, "start_recording");
  assert.equal(request.args.takeName, "Soundcheck");
  assert.equal(request.args.utcOffsetMinutes, -new Date().getTimezoneOffset(), "the window's");
  let recording = recordingStatus(app.store.getState());
  assert.equal(recording.state, "recording");
  assert.deepEqual(recording.tracks, ["Kick", "Snare", "Local mix"]);
  assert.ok(recording.takeFolder.startsWith(`${FAKE_RECORDINGS_FOLDER}\\`));
  assert.match(
    recording.takeFolder.slice(FAKE_RECORDINGS_FOLDER.length + 1),
    /^2026-10-1[67]_\d{6}_Soundcheck$/,
  );

  time.now += 2_000;
  await statusNow(app, fake);
  recording = recordingStatus(app.store.getState());
  assert.equal(recording.frames, 96_000, "frames advance with time");
  assert.equal(recording.seconds, 2);
  assert.ok(recording.bytesWritten > 0);

  const chorus = await app.addMarker("Chorus");
  assert.deepEqual(chorus.marker, { frame: 96_000, seconds: 2, name: "Chorus" });
  assert.equal(chorus.message, 'Marked "Chorus" at 0:02.');
  time.now += 500;
  const unnamed = await app.addMarker("  ");
  assert.equal(unnamed.marker.name, "Marker 2");
  assert.equal(recordingStatus(app.store.getState()).markers.length, 2);

  time.now += 1_000;
  const stopped = await app.stopRecording();

  assert.equal(stopped.ok, true, stopped.message);
  assert.equal(stopped.message, '"Soundcheck" stopped: 0:03 on 3 tracks.');
  assert.equal(stopped.take.frames, 168_000);
  assert.equal(recordingStatus(app.store.getState()).state, "idle");
  assert.equal(lastTake(app.store.getState()).name, "Soundcheck");
  const listed = app.store.getState().takes;
  assert.equal(listed.list[0].name, "Soundcheck", "the takes are read again");
  assert.equal(listed.list[0].ended, "stopped");
  assert.equal(listed.list[0].markers.length, 2);
  const marker = await app.addMarker("Late");
  assert.equal(marker.ok, false);
  assert.match(marker.message, /^The marker was not added: no take is recording/);
  app.dispose();
});

test("a stop whose files are still being finished is asked again, and given up on in words", async () => {
  const { fake } = clocked();
  const app = createConsoleApp({ bridge: fake, stopRetryMs: 1, stopRetries: 3 });
  await app.ready;
  await app.setRecordingSettings({ armed: [1] });
  await app.startRecording("One");
  fake.holdStop(2);

  const stopped = await app.stopRecording();

  assert.equal(stopped.ok, true, stopped.message);
  assert.equal(commands(fake, "stop_recording").length, 3, "asked until the files were finished");

  await app.startRecording("Two");
  fake.holdStop(10);
  const waiting = await app.stopRecording();
  assert.equal(waiting.ok, false);
  assert.match(waiting.message, /still being finished/);
  assert.match(waiting.message, /Stop again/);
  assert.equal(recordingStatus(app.store.getState()).state, "stopping");
  assert.equal(isRecordingNow(app.store.getState()), true);
  app.dispose();
});

test("a start with no engine or nothing armed is refused in words", async () => {
  const idle = createFakeBridge();
  const stopped = createConsoleApp({ bridge: idle, autoStart: false });
  await stopped.ready;
  const noEngine = await stopped.startRecording("Show");
  assert.equal(noEngine.ok, false);
  assert.match(noEngine.message, /^The recording did not start: the engine is not running/);
  stopped.dispose();

  const { fake } = clocked();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  const nothing = await app.startRecording("Show");
  assert.equal(nothing.ok, false);
  assert.match(nothing.message, /nothing is armed/);
  await app.setRecordingSettings({ armed: [7, 8] });
  const elsewhere = await app.startRecording("Show");
  assert.match(elsewhere.message, /CH 7, CH 8 are past its 6 inputs/);
  assert.equal(recordingStatus(app.store.getState()).state, "idle");
  app.dispose();
});

test("a take recording blocks a device switch and a driver change, saying why", async () => {
  const { fake } = clocked();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1] });
  await app.startRecording("Show");
  const startsBefore = commands(fake, "start_engine").length;

  await app.switchDevice("Microphone (USB audio)");
  const changed = await app.applyAudio({ bufferFrames: 128 }, { keepDevice: true });

  assert.equal(commands(fake, "switch_device").length, 0, "the switch never went out");
  assert.equal(commands(fake, "start_engine").length, startsBefore, "nor a restart");
  assert.deepEqual(app.store.getState().commandError, {
    action: "switch",
    message: RECORDING_BLOCKS_SWITCH,
  });
  assert.deepEqual(changed, { ok: false, error: RECORDING_BLOCKS_SWITCH });
  assert.equal(recordingStatus(app.store.getState()).state, "recording", "the take goes on");
  await assert.rejects(
    fake.invoke("switch_device", { device: "Microphone (USB audio)" }),
    /stop the recording before switching the input device/,
    "the engine refuses it too",
  );
  app.dispose();
});

test("an engine that stops ends the take, which the list then says", async () => {
  const { fake, time } = clocked();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1] });
  await app.startRecording("Show");
  time.now += 1_000;

  await app.stop();
  await app.refreshTakes();

  const [take] = app.store.getState().takes.list;
  assert.equal(take.ended, "engineStopped");
  assert.equal(take.frames, 48_000);
  app.dispose();
});

const OLDER = {
  folder: `${FAKE_RECORDINGS_FOLDER}\\2026-10-14_200000_Rehearsal`,
  name: "Rehearsal",
  started: "2026-10-14T18:00:00Z",
  sampleRate: 48000,
  format: "int24",
  device: "Demo interface",
  tracks: [],
  frames: 480_000,
  seconds: 10,
  markers: [],
  droppedFrames: 0,
  ended: "stopped",
  reason: null,
};
const NEWER = {
  ...OLDER,
  folder: `${FAKE_RECORDINGS_FOLDER}\\2026-10-15_200000_Crash`,
  name: "Crash",
  started: "2026-10-15T18:00:00Z",
  ended: "interrupted",
};

test("the takes are listed newest first, say what was left out, and only a listed one opens", async () => {
  const { fake } = clocked({ savedTakes: [OLDER, NEWER], unreadableTakes: 1 });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.equal(await app.refreshTakes(), null);

  const takes = app.store.getState().takes;
  assert.deepEqual(
    takes.list.map((take) => take.name),
    ["Crash", "Rehearsal"],
  );
  assert.equal(takes.unreadable, 1);
  assert.equal(takes.folder, FAKE_RECORDINGS_FOLDER);
  const opened = await app.openTakeFolder(NEWER.folder);
  assert.deepEqual(opened, { ok: true, message: `Opened ${NEWER.folder}.` });
  assert.deepEqual(fake.openedFolders(), [NEWER.folder]);
  const refused = await app.openTakeFolder("C:\\Windows");
  assert.equal(refused.ok, false);
  assert.match(refused.message, /^The take folder was not opened: .*not a take folder/);
  assert.deepEqual(fake.openedFolders(), [NEWER.folder], "nothing else was opened");

  fake.failNext("list_takes", "the folder could not be read");
  assert.notEqual(await app.refreshTakes(), null);
  assert.equal(app.store.getState().takes.list.length, 2, "the last list stays");
  assert.equal(app.store.getState().takes.error, "the folder could not be read");
  app.dispose();
});

test("the estimate follows the settings and says why it has no rate", async () => {
  const { fake } = clocked({ freeBytes: 17_280_000_000 });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.setRecordingSettings({ armed: [1, 2] });

  await app.refreshEstimate();

  assert.deepEqual(app.store.getState().estimate, {
    folder: FAKE_RECORDINGS_FOLDER,
    bytesPerSecond: 288_000,
    freeBytes: 17_280_000_000,
    minutesLeft: 1000,
    problem: null,
  });
  await app.setRecordingSettings({ format: "float32", includeLocal: true });
  assert.equal(
    app.store.getState().estimate.bytesPerSecond,
    2 * 192_000 + 384_000,
    "a settings change estimates again: two mono files and the stereo local mix in float",
  );

  await app.stop();
  const idle = await fake.invoke("record_estimate");
  assert.equal(idle.bytesPerSecond, null);
  assert.match(idle.problem, /not running/);
  app.dispose();
});
