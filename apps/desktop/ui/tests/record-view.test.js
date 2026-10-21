// What the Record screen shows (record-view.js), worked out from the console state without a DOM:
// the track rows, the takes table, the REC chip, the disk estimate and its warnings, the
// transport, and the bounded peak history the waveform lanes are drawn from.
import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";
import {
  CRITICAL_MINUTES,
  LOW_MINUTES,
  armChange,
  createPeakHistory,
  diskView,
  formatBytes,
  formatTakeStarted,
  localTrackRow,
  recChip,
  recordMessages,
  settingsView,
  spokenDuration,
  takeEndLabel,
  takeNameOf,
  takeRow,
  takeRows,
  trackRows,
  transportView,
} from "../app/lib/record-view.js";
import { IDLE_RECORDING, settingsRequest } from "../app/lib/recording.js";

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

const SETTINGS = Object.freeze({
  folder: "D:\\Recordings",
  defaultFolder: "C:\\Users\\Demo\\Music\\BandEar\\Recordings",
  folderIsDefault: false,
  format: "int24",
  includeLocal: false,
  armed: [1, 4],
  notice: null,
});

// The sample status (Kick, Snare, Overheads, Bass, Lead vox, Backing vox) with `changes` on top.
function statusWith(changes = {}) {
  const status = sampleStatus();
  if (changes.channels) {
    status.channels = status.channels.map((channel, index) => ({
      ...channel,
      ...(changes.channels[index] ?? {}),
    }));
  }
  return { ...status, ...changes, channels: status.channels };
}

function running({ status = statusWith(), settings = SETTINGS } = {}) {
  return reduce([actions.status(status), actions.recordingSettings(settings)]);
}

function recordingNow(changes = {}) {
  return {
    ...IDLE_RECORDING,
    state: "recording",
    takeFolder: "D:\\Recordings\\2026-10-16_210405_Soundcheck",
    frames: 754 * 48000,
    seconds: 754,
    tracks: ["Kick", "Bass"],
    markers: [{ frame: 65 * 48000, seconds: 65, name: "Song 1" }],
    freeBytes: 214_000_000_000,
    minutesLeft: 2040,
    ...changes,
  };
}

test("each recorded unit of the device is one track row, a linked pair one stereo row", () => {
  const status = statusWith({ channels: { 2: { link: true }, 5: { name: "" } } });
  const rows = trackRows(running({ status, settings: { ...SETTINGS, armed: [4, 5] } }));

  assert.deepEqual(
    rows.map(({ key, inputs, stereo, name, source, armed }) => ({
      key,
      inputs,
      stereo,
      name,
      source,
      armed,
    })),
    [
      { key: "in-1", inputs: [1], stereo: false, name: "Kick", source: "IN 1", armed: false },
      { key: "in-2", inputs: [2], stereo: false, name: "Snare", source: "IN 2", armed: false },
      {
        key: "in-3",
        inputs: [3, 4],
        stereo: true,
        name: "Overheads",
        source: "IN 3–4 · stereo",
        armed: true,
      },
      { key: "in-5", inputs: [5], stereo: false, name: "Lead vox", source: "IN 5", armed: true },
      { key: "in-6", inputs: [6], stereo: false, name: "CH 6", source: "IN 6", armed: false },
    ],
  );
  assert.deepEqual(rows[2].sides, [2, 3], "a row's sides are the 0-based inputs its meter reads");
});

test("an unnamed pair is called by both its inputs, and a hidden input still records", () => {
  const status = statusWith({
    channels: { 0: { name: "", link: true }, 5: { hidden: true } },
  });
  const rows = trackRows(running({ status }));

  assert.equal(rows[0].name, "CH 1-2");
  assert.equal(rows.at(-1).source, "IN 6 · hidden");
  assert.equal(rows.length, 5);
});

test("there are no track rows while no engine runs: the inputs depend on the device", () => {
  assert.deepEqual(trackRows(reduce([actions.recordingSettings(SETTINGS)])), []);
});

test("arming a row changes the armed inputs, a pair as one, and the local row the local mix", () => {
  const status = statusWith({ channels: { 2: { link: true } } });
  const state = running({ status, settings: { ...SETTINGS, armed: [1, 4] } });
  const rows = trackRows(state);

  // The change is applied when its request is built, on the settings answered last.
  const armedAfterPress = (row, settings = state.recordingSettings) =>
    settingsRequest(settings, armChange(state, row)).armed;

  assert.deepEqual(armedAfterPress(rows[1]), [1, 2, 4], "Snare armed");
  assert.deepEqual(armedAfterPress(rows[0]), [4], "Kick disarmed");
  assert.deepEqual(armedAfterPress(rows[2]), [1], "the pair disarmed whole");
  assert.deepEqual(armChange(state, localTrackRow(state)), { includeLocal: true });
  assert.deepEqual(
    armedAfterPress(rows[1], { ...SETTINGS, armed: [1, 4, 6] }),
    [1, 2, 4, 6],
    "a press made before an earlier one was answered keeps the earlier one",
  );
});

test("the local mix row records when the settings say so", () => {
  const off = localTrackRow(running());
  assert.equal(off.key, "local");
  assert.equal(off.local, true);
  assert.equal(off.name, "Local mix");
  assert.equal(off.armed, false);

  assert.equal(localTrackRow(running({ settings: { ...SETTINGS, includeLocal: true } })).armed, true);
  assert.equal(localTrackRow(initialState), null, "no row while the settings are unknown");
});

test("a take's start reads in the window's local time", () => {
  assert.equal(formatTakeStarted("2026-10-16T19:04:05Z", 120), "2026-10-16 21:04");
  assert.equal(formatTakeStarted("2026-10-16T23:30:00Z", 60), "2026-10-17 00:30");
  assert.equal(formatTakeStarted("2026-10-16T01:00:00Z", -300), "2026-10-15 20:00");
  assert.equal(formatTakeStarted("not a date", 0), "Unknown");
});

test("a take row says its name, start, length, tracks, markers and how it ended", () => {
  const take = {
    folder: "D:\\Recordings\\2026-10-16_210405_Soundcheck",
    name: "Soundcheck",
    started: "2026-10-16T19:04:05Z",
    seconds: 3725.4,
    tracks: [
      { kind: "input", name: "Kick" },
      { kind: "pair", name: "Overheads" },
      { kind: "local", name: "Local mix" },
    ],
    markers: [{ name: "Song 1" }],
    droppedFrames: 0,
    ended: "stopped",
    reason: null,
  };

  assert.deepEqual(takeRow(take, 120), {
    key: take.folder,
    folder: take.folder,
    name: "Soundcheck",
    started: "2026-10-16 21:04",
    length: "1:02:05",
    tracks: "2 tracks + local mix",
    markers: "1 marker",
    end: { text: "Stopped", kind: "ok", detail: "stopped" },
    dropped: null,
    recording: false,
  });

  const local = takeRow({ ...take, tracks: [{ kind: "local" }], markers: [] }, 0);
  assert.equal(local.tracks, "Local mix only");
  assert.equal(local.markers, "No markers");
  const lossy = takeRow({ ...take, droppedFrames: 480 }, 0);
  assert.equal(lossy.dropped, "480 frames missing");
});

test("every way a take ends has a short word and its whole sentence", () => {
  assert.deepEqual(takeEndLabel({ ended: "stopped" }), {
    text: "Stopped",
    kind: "ok",
    detail: "stopped",
  });
  assert.deepEqual(takeEndLabel({ ended: "engineStopped" }), {
    text: "Engine stopped",
    kind: "warn",
    detail: "ended when the engine stopped",
  });
  assert.deepEqual(takeEndLabel({ ended: "diskError", reason: "the disk is full" }), {
    text: "Disk error",
    kind: "error",
    detail: "stopped by a disk error: the disk is full",
  });
  assert.equal(takeEndLabel({ ended: "interrupted" }).text, "Interrupted");
  assert.equal(takeEndLabel({ ended: "interrupted" }).kind, "warn");
  assert.deepEqual(takeEndLabel({ ended: "recording" }), {
    text: "Recording",
    kind: "live",
    detail: "recording now",
  });
});

test("the take being recorded shows the length the status reports, not the listed one", () => {
  const listed = {
    folder: "D:\\Recordings\\2026-10-16_210405_Soundcheck",
    name: "Soundcheck",
    started: "2026-10-16T19:04:05Z",
    seconds: 3,
    tracks: [{ kind: "input" }],
    markers: [],
    droppedFrames: 0,
    ended: "recording",
  };
  const state = reduce(
    [
      actions.takes({ folder: "D:\\Recordings", takes: [listed], unreadable: 0, missing: false }),
    ],
    running({ status: { ...statusWith(), recording: recordingNow() } }),
  );

  const [row] = takeRows(state, 0);
  assert.equal(row.length, "12:34");
  assert.equal(row.recording, true);
  assert.deepEqual(takeRows(initialState, 0), [], "nothing listed yet");
});

test("the REC chip shows while a take records, with its time and tracks, and leads to Record", () => {
  assert.equal(recChip(running()), null, "nothing while no take records");

  const chip = recChip(running({ status: { ...statusWith(), recording: recordingNow() } }));
  assert.deepEqual(chip, {
    kind: "recording",
    word: "REC",
    time: "12:34",
    detail: "2 tracks",
    label: "Recording, 12 minutes 34 seconds, 2 tracks. Open Record",
  });

  const stopping = recChip(
    running({ status: { ...statusWith(), recording: recordingNow({ state: "stopping" }) } }),
  );
  assert.equal(stopping.kind, "stopping");
  assert.equal(stopping.word, "Finishing");

  const failed = recChip(
    running({
      status: {
        ...statusWith(),
        recording: recordingNow({ state: "failed", reason: "the disk is full" }),
      },
    }),
  );
  assert.equal(failed.kind, "failed");
  assert.equal(failed.word, "REC stopped");
  assert.equal(failed.label, "The recording stopped: the disk is full. Open Record");
});

test("a duration is spoken in words", () => {
  assert.equal(spokenDuration(0), "0 seconds");
  assert.equal(spokenDuration(1), "1 second");
  assert.equal(spokenDuration(754), "12 minutes 34 seconds");
  assert.equal(spokenDuration(3600), "1 hour");
  assert.equal(spokenDuration(3725.9), "1 hour 2 minutes 5 seconds");
});

test("sizes read in the units a file manager uses", () => {
  assert.equal(formatBytes(214_000_000_000), "214 GB");
  assert.equal(formatBytes(1_340_000_000), "1.3 GB");
  assert.equal(formatBytes(705_000_000), "705 MB");
  assert.equal(formatBytes(2_500_000_000_000), "2.5 TB");
  assert.equal(formatBytes(512), "512 B");
});

test("the disk estimate reads the free space and the minutes left before a take", () => {
  const state = reduce(
    [
      actions.estimate({
        folder: "D:\\Recordings",
        bytesPerSecond: 288_000,
        freeBytes: 214_000_000_000,
        minutesLeft: 12_384,
        problem: null,
      }),
    ],
    running(),
  );

  assert.deepEqual(diskView(state), {
    free: "214 GB free",
    left: "about 206 h left",
    problem: null,
    warning: null,
  });
});

test("while a take records, the disk reads what the recorder reports", () => {
  const state = running({
    status: { ...statusWith(), recording: recordingNow({ freeBytes: 9_000_000_000, minutesLeft: 95 }) },
  });

  const view = diskView(state);
  assert.equal(view.free, "9.0 GB free");
  assert.equal(view.left, "about 95 min left");
});

test("the disk warns below the low and critical thresholds of minutes left", () => {
  assert.equal(LOW_MINUTES, 30);
  assert.equal(CRITICAL_MINUTES, 10);
  const at = (minutesLeft) =>
    diskView(
      running({
        status: { ...statusWith(), recording: recordingNow({ minutesLeft }) },
      }),
    ).warning;

  assert.equal(at(30), null, "30 minutes is enough");
  assert.deepEqual(at(29), {
    level: "low",
    text: "About 29 minutes of recording space are left on this disk.",
  });
  assert.equal(at(10).level, "low");
  assert.deepEqual(at(9), {
    level: "critical",
    text:
      "Only about 9 minutes of recording space are left: free some disk space or choose " +
      "another folder before the take fills it.",
  });
  assert.equal(at(1).text.startsWith("Only about 1 minute of recording space is left"), true);
  assert.equal(at(0).level, "critical");
  assert.equal(at(null), null, "an unknown rate warns of nothing");
});

test("an estimate that cannot be made says why, and nothing unknown is guessed", () => {
  const state = reduce(
    [
      actions.estimate({
        folder: "D:\\Recordings",
        bytesPerSecond: null,
        freeBytes: 214_000_000_000,
        minutesLeft: null,
        problem: "arm at least one input, or record the local mix",
      }),
    ],
    running(),
  );

  assert.deepEqual(diskView(state), {
    free: "214 GB free",
    left: null,
    problem: "Arm at least one input, or record the local mix.",
    warning: null,
  });
  assert.deepEqual(diskView(initialState), {
    free: "Free space unknown",
    left: null,
    problem: null,
    warning: null,
  });
});

test("the peak history keeps the newest levels up to its capacity and never grows", () => {
  const history = createPeakHistory(4);
  assert.equal(history.capacity, 4);
  assert.equal(history.length, 0);

  for (let level = 1; level <= 1000; level += 1) {
    history.push(level);
    assert.equal(history.length <= 4, true);
  }
  assert.equal(history.length, 4);
  const seen = [];
  history.forEach((level, index) => seen.push([index, level]));
  assert.deepEqual(seen, [
    [0, 997],
    [1, 998],
    [2, 999],
    [3, 1000],
  ]);
  assert.equal(history.at(0), 997, "the oldest first");
  assert.equal(history.at(3), 1000);

  history.clear();
  assert.equal(history.length, 0);
  history.push(7);
  assert.equal(history.at(0), 7);
});

test("the peak history keeps what it is given in order until it is full", () => {
  const history = createPeakHistory(3);
  history.push(10);
  history.push(20);
  const seen = [];
  history.forEach((level) => seen.push(level));
  assert.deepEqual(seen, [10, 20]);
  assert.equal(createPeakHistory(0).capacity, 1, "at least one level");
});

test("a take's name is its folder's name without the time stamp", () => {
  assert.equal(takeNameOf("D:\\Recordings\\2026-10-16_210405_Soundcheck"), "Soundcheck");
  assert.equal(takeNameOf("D:\\Recordings\\2026-10-16_210405_Take (2)"), "Take (2)");
  assert.equal(takeNameOf("/home/demo/Recordings/2026-10-16_210405_Show"), "Show");
  assert.equal(takeNameOf(null), "");
});

test("the transport records while an engine runs and stops and marks while a take records", () => {
  const idle = transportView(running(), { atStart: false });
  assert.equal(idle.canRecord, true);
  assert.equal(idle.canStop, false);
  assert.equal(idle.canMark, false);
  assert.equal(idle.recording, false);
  assert.equal(idle.position, "0:00");
  assert.equal(idle.stateWord, "Ready to record");
  assert.equal(idle.armed, "2 tracks");
  assert.equal(idle.format, "WAV · 48 kHz · 24-bit");

  const live = transportView(
    running({ status: { ...statusWith(), recording: recordingNow() } }),
    { atStart: false },
  );
  assert.equal(live.canRecord, false);
  assert.equal(live.canStop, true);
  assert.equal(live.canMark, true);
  assert.equal(live.canRewind, false);
  assert.equal(live.recording, true);
  assert.equal(live.position, "12:34");
  assert.equal(live.stateWord, "Recording · Soundcheck");
  assert.deepEqual(live.markers, [{ key: "0", time: "1:05", name: "Song 1" }]);

  const off = transportView(reduce([actions.recordingSettings(SETTINGS)]), { atStart: false });
  assert.equal(off.canRecord, false);
  assert.equal(off.stateWord, "Start the engine to record");
});

test("after a take the position shows its length until Back to start", () => {
  const take = {
    folder: "D:\\Recordings\\2026-10-16_210405_Soundcheck",
    name: "Soundcheck",
    seconds: 95,
    tracks: [],
    markers: [{ seconds: 30, name: "Chorus" }],
    ended: "stopped",
  };
  const state = running({ status: { ...statusWith(), lastTake: take } });

  const after = transportView(state, { atStart: false });
  assert.equal(after.position, "1:35");
  assert.equal(after.canRewind, true);
  assert.deepEqual(after.markers, [{ key: "0", time: "0:30", name: "Chorus" }]);
  assert.equal(after.markersOf, "Soundcheck");

  const rewound = transportView(state, { atStart: true });
  assert.equal(rewound.position, "0:00");
  assert.equal(rewound.canRewind, false);
});

test("a float take and the local mix read in the transport's summary", () => {
  const view = transportView(
    running({ settings: { ...SETTINGS, format: "float32", includeLocal: true, armed: [] } }),
    { atStart: false },
  );
  assert.equal(view.format, "WAV · 48 kHz · 32-bit float");
  assert.equal(view.armed, "Local mix only");
  const both = transportView(running({ settings: { ...SETTINGS, includeLocal: true } }), {
    atStart: false,
  });
  assert.equal(both.armed, "2 tracks + local mix");
  const none = transportView(running({ settings: { ...SETTINGS, armed: [] } }), { atStart: false });
  assert.equal(none.armed, "Nothing armed");
});

test("the screen says each command's outcome, a failed take and a takes listing that failed", () => {
  const said = reduce(
    [
      actions.recordingOutcome("start", false, "The recording did not start: nothing is armed."),
      actions.takesFailed("the folder is gone"),
    ],
    running({
      status: {
        ...statusWith(),
        recording: recordingNow({ state: "failed", reason: "the disk is full" }),
      },
    }),
  );

  assert.deepEqual(recordMessages(said), {
    outcome: { ok: false, text: "The recording did not start: nothing is armed." },
    failure: "The take stopped recording: the disk is full.",
    takes: "The takes could not be listed: the folder is gone.",
    locked: null,
  });

  const recording = recordMessages(
    running({ status: { ...statusWith(), recording: recordingNow() } }),
  );
  assert.equal(recording.outcome, null);
  assert.equal(recording.failure, null);
  assert.equal(
    recording.locked,
    "Arming and the settings stay as they are while a take records; they apply from the next take.",
  );
});

test("the settings show the folder, its default and notice, the format and the local mix", () => {
  const view = settingsView(running({ settings: { ...SETTINGS, notice: "The Music folder…" } }));
  assert.deepEqual(view, {
    known: true,
    folder: "D:\\Recordings",
    defaultFolder: "C:\\Users\\Demo\\Music\\BandEar\\Recordings",
    isDefault: false,
    notice: "The Music folder…",
    format: "int24",
    formats: [
      { value: "int24", label: "WAV 24-bit · one file per track" },
      { value: "float32", label: "WAV 32-bit float · one file per track" },
    ],
    includeLocal: false,
    locked: false,
  });
  assert.equal(settingsView(initialState).known, false);
  assert.equal(
    settingsView(running({ status: { ...statusWith(), recording: recordingNow() } })).locked,
    true,
  );
});
