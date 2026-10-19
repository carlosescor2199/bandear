import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import {
  LOCAL_MIX,
  actions,
  consoleReducer,
  initialState,
  pendingEdits,
  viewMix,
} from "../app/lib/console-state.js";
import { createFakeBridge, sampleOutputs, sampleStatus } from "../app/lib/fake-bridge.js";
import {
  LOCAL_OUTPUT_HINT,
  formatLocalOutputSet,
  localOutputView,
  localTab,
  outputLabel,
  outputOptions,
  outputValue,
  parseOutputValue,
} from "../app/lib/local-mix.js";
import { editKey } from "../app/lib/mix.js";
import {
  channelStrips,
  masterStrip,
  mixMessages,
  mixNotice,
  mixTabs,
} from "../app/lib/mix-view.js";

const LEAD = "192.168.1.3:50000";
const DEMO = "Demo interface";
const SPEAKERS = "Speakers (Realtek Audio)";

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
}

function withOutput(localOutput) {
  const status = sampleStatus();
  status.localOutput = { ...status.localOutput, ...localOutput };
  return reduce([actions.status(status)]);
}

const playingDemo = {
  device: DEMO,
  firstChannel: 2,
  state: "running",
  reason: null,
  underrunFrames: 0,
  droppedFrames: 0,
  notice: null,
};

// The select

test("the local mix out offers none first, then each stereo pair of every output device", () => {
  assert.deepEqual(outputOptions(sampleOutputs()), [
    { value: "", label: "None (lowest load)" },
    { value: outputValue(DEMO, 0), label: "Demo interface · outputs 1/2" },
    { value: outputValue(DEMO, 2), label: "Demo interface · outputs 3/4" },
    { value: outputValue(SPEAKERS, 0), label: "Speakers (Realtek Audio) · outputs 1/2" },
  ]);
  assert.deepEqual(
    outputOptions([
      { name: "Mono out", channels: 1 },
      { name: "Odd out", channels: 3 },
      { name: "Not at 48 kHz", channels: 0 },
    ]).map(({ label }) => label),
    ["None (lowest load)", "Odd out · outputs 1/2"],
    "only whole pairs: a mono device, a lone last output and a device with none offer nothing",
  );
  assert.equal(outputLabel("Volt 4", 0), "Volt 4 · outputs 1/2");
});

test("a choice reads back from the select's value, whatever the device is called", () => {
  for (const [device, firstChannel] of [
    [DEMO, 2],
    ["Out | A (B)", 0],
  ]) {
    assert.deepEqual(parseOutputValue(outputValue(device, firstChannel)), { device, firstChannel });
  }
  assert.equal(parseOutputValue(""), null, "None");
  assert.equal(parseOutputValue("not a choice"), null);
  assert.equal(parseOutputValue("-2|Demo"), null);
});

test("with an engine the select shows the outputs listed and the one in use", () => {
  const listed = reduce([actions.outputs(sampleOutputs())], withOutput(playingDemo));

  const view = localOutputView(listed);

  assert.equal(view.value, outputValue(DEMO, 2));
  assert.equal(view.disabled, false);
  assert.equal(view.options.length, 4);
  assert.equal(view.hint, LOCAL_OUTPUT_HINT);
  assert.equal(view.error, null);
  assert.deepEqual(view.state, {
    kind: "running",
    text: "Playing on Demo interface · outputs 3/4: no gaps, nothing dropped.",
    alert: null,
  });
});

test("the state of the output is said in words with its counters, a failure as an alert", () => {
  const counted = localOutputView(
    reduce(
      [actions.outputs(sampleOutputs())],
      withOutput({ ...playingDemo, underrunFrames: 1, droppedFrames: 12480 }),
    ),
  );
  assert.equal(
    counted.state.text,
    "Playing on Demo interface · outputs 3/4: 1 silent frame filled in, 12,480 frames dropped " +
      "to hold the delay.",
  );

  const failed = localOutputView(
    reduce(
      [actions.outputs(sampleOutputs())],
      withOutput({ ...playingDemo, state: "failed", reason: "the device was unplugged" }),
    ),
  );
  assert.equal(failed.state.kind, "failed");
  assert.equal(failed.value, outputValue(DEMO, 2), "the output that failed stays chosen");
  assert.equal(
    failed.state.alert,
    "Demo interface · outputs 3/4 stopped: the device was unplugged. Choose it again, or " +
      "another output.",
  );

  const off = localOutputView(reduce([actions.outputs(sampleOutputs())], withOutput({})));
  assert.equal(off.value, "");
  assert.deepEqual(off.state, {
    kind: "off",
    text: "Off: the local mix plays nowhere.",
    alert: null,
  });
});

test("a kept output the engine did not open is said, and one not offered now still shows", () => {
  const notice =
    "The saved local mix output, Volt 4 · outputs 1/2, was not opened: the running audio " +
    "driver does not offer it. The local mix plays nowhere until you choose an output.";
  const said = localOutputView(
    reduce([actions.outputs(sampleOutputs())], withOutput({ notice })),
  );
  assert.equal(said.notice, notice);

  const gone = localOutputView(
    reduce(
      [actions.outputs([])],
      withOutput({ ...playingDemo, state: "failed", reason: "unplugged" }),
    ),
  );
  assert.deepEqual(gone.options, [
    { value: "", label: "None (lowest load)" },
    { value: outputValue(DEMO, 2), label: "Demo interface · outputs 3/4 (not offered now)" },
  ]);
});

test("with no engine, or before the outputs are listed, the select waits and says why", () => {
  const idle = localOutputView(initialState);
  assert.equal(idle.disabled, true);
  assert.equal(idle.value, "");
  assert.deepEqual(idle.options, [{ value: "", label: "None (lowest load)" }]);
  assert.equal(idle.state.text, "Start the engine to choose where the local mix plays.");

  const listing = localOutputView(withOutput({}));
  assert.equal(listing.disabled, true);
  assert.equal(listing.state.text, "Listing the outputs…");

  const failed = localOutputView(reduce([actions.outputsFailed("no audio service")], withOutput({})));
  assert.equal(failed.error, "The outputs could not be listed: no audio service.");
  assert.equal(failed.disabled, false, "None can still be chosen");
});

test("what a change of the output did is said in words", () => {
  const output = { device: DEMO, firstChannel: 2 };
  assert.equal(
    formatLocalOutputSet({ output, applied: true, saved: true, saveError: null }),
    "The local mix plays on Demo interface · outputs 3/4.",
  );
  assert.equal(
    formatLocalOutputSet({ output: null, applied: true, saved: true, saveError: null }),
    "The local mix plays nowhere now.",
  );
  assert.equal(
    formatLocalOutputSet({ output, applied: false, saved: true, saveError: null }),
    "Demo interface · outputs 3/4 is kept: the local mix plays there from the next start.",
  );
  assert.equal(
    formatLocalOutputSet({ output, applied: true, saved: false, saveError: "disk full" }),
    "The local mix plays on Demo interface · outputs 3/4. It was not saved for the next start: " +
      "disk full.",
  );
});

// The tab and the store

test("the Local mix tab comes last and says where the local mix plays", () => {
  const running = withOutput(playingDemo);

  const tabs = mixTabs(running);

  assert.equal(tabs.length, sampleStatus().musicians.length + 1);
  assert.deepEqual(tabs.at(-1), {
    address: LOCAL_MIX,
    label: "Local mix",
    link: {
      level: "local",
      label: "This computer · Demo interface · outputs 3/4",
      detail: "The local mix plays on this computer",
    },
    selected: false,
  });
  assert.equal(localTab(withOutput({})).link.label, "This computer · no output");
  const failed = localTab(withOutput({ ...playingDemo, state: "failed", reason: "unplugged" }));
  assert.equal(failed.link.level, "warning");
  assert.equal(failed.link.label, "This computer · output stopped");
  assert.equal(localTab(initialState), null, "no engine, no local mix");
  assert.equal(mixTabs(initialState).length, 0);
});

test("the local mix is picked like a musician, kept across statuses and forgotten when a run ends", () => {
  const picked = reduce([actions.select(LOCAL_MIX)], withOutput({}));
  assert.equal(picked.selected, LOCAL_MIX);
  assert.equal(localTab(picked).selected, true);

  const again = reduce([actions.status(sampleStatus())], picked);
  assert.equal(again.selected, LOCAL_MIX, "a status keeps the local mix open");

  const ended = reduce([actions.engine("idle")], again);
  assert.equal(ended.localMix, null);
  assert.equal(viewMix(ended, LOCAL_MIX), null);
});

test("edits of the local mix wait under its id and an answer settles them; a cap, limit or lock never waits", () => {
  const running = withOutput({});
  const edited = reduce(
    [
      actions.mixEdit(LOCAL_MIX, {
        [editKey("volumePercent")]: 60,
        [editKey("channelGains", 1)]: 30,
        [editKey("maxLevelPercent")]: 50,
        [editKey("engineerLimitPercent")]: 40,
        [editKey("lockedChannels", 0)]: true,
      }),
    ],
    running,
  );

  assert.deepEqual(pendingEdits(edited, LOCAL_MIX), {
    volumePercent: 60,
    "channelGains.1": 30,
  });
  assert.equal(viewMix(edited, LOCAL_MIX).volumePercent, 60);

  const answer = { ...sampleStatus().localMix, volumePercent: 60, revision: 50 };
  const settled = reduce(
    [actions.mixApplied(LOCAL_MIX, { volumePercent: 60 }, answer)],
    edited,
  );
  assert.deepEqual(pendingEdits(settled, LOCAL_MIX), { "channelGains.1": 30 });
  assert.equal(settled.localMix.revision, 50);

  const refused = reduce([actions.mixFailed(LOCAL_MIX, { "channelGains.1": 30 }, "no")], settled);
  assert.deepEqual(pendingEdits(refused, LOCAL_MIX), {});
  assert.equal(
    mixMessages(refused).error,
    "A change to the local mix was not applied: no.",
  );
});

test("a local mix change event updates the local mix by revision and never a musician's", () => {
  const running = withOutput({});
  const newer = { ...sampleStatus().localMix, volumePercent: 33, revision: 90 };

  const changed = reduce(
    [actions.mixChanged({ run: 1, id: LOCAL_MIX, origin: "desktop", mix: newer })],
    running,
  );

  assert.equal(changed.localMix.volumePercent, 33);
  assert.deepEqual(
    changed.musicians.map((musician) => musician.mix),
    running.musicians.map((musician) => musician.mix),
  );
  const older = reduce(
    [
      actions.mixChanged({
        run: 1,
        id: LOCAL_MIX,
        origin: "desktop",
        mix: { ...newer, volumePercent: 1, revision: 2 },
      }),
    ],
    changed,
  );
  assert.equal(older.localMix.volumePercent, 33, "an older copy never goes back");
});

test("the local mix's strips have no lock and its master no cap or limit", () => {
  const running = reduce([actions.select(LOCAL_MIX)], withOutput({}));

  const strips = channelStrips(running, LOCAL_MIX);
  assert.ok(strips.length > 0);
  for (const strip of strips) {
    assert.equal(strip.lockable, false);
    assert.equal(strip.following, null);
  }
  assert.ok(channelStrips(running, LEAD).every((strip) => strip.lockable === true));

  assert.deepEqual(masterStrip(running, LOCAL_MIX), {
    volume: 100,
    cap: null,
    limit: null,
    muted: false,
    readout: "100%",
    ceilingNote: null,
  });
});

test("with no phones the local mix can still be opened and shows its strips", () => {
  const status = sampleStatus();
  status.musicians = [];
  const empty = reduce([actions.status(status)]);
  assert.deepEqual(mixNotice(empty), { kind: "no-musicians" });
  assert.equal(mixTabs(empty).length, 1, "the Local mix tab alone");

  const local = reduce([actions.select(LOCAL_MIX)], empty);
  assert.equal(mixNotice(local), null);
});

// The fake engine

test("the fake lists the outputs only while it runs, and under ASIO only the driver's own", async () => {
  const fake = createFakeBridge();
  await assert.rejects(fake.invoke("output_devices"), /not running/);

  await fake.invoke("start_engine", { config: {} });
  assert.deepEqual(await fake.invoke("output_devices"), sampleOutputs());

  await fake.invoke("start_engine", { config: { host: "asio", deviceFilter: "Demo ASIO" } });
  assert.deepEqual(await fake.invoke("output_devices"), [{ name: "Demo ASIO driver", channels: 4 }]);
});

test("the fake plays the local mix on an output it offers and refuses one it does not", async () => {
  const fake = createFakeBridge({ running: true });

  const taken = await fake.invoke("set_local_output", {
    output: { device: DEMO, firstChannel: 2 },
  });
  assert.deepEqual(taken, {
    output: { device: DEMO, firstChannel: 2 },
    localOutput: { ...playingDemo },
    applied: true,
    saved: true,
    saveError: null,
  });

  for (const [output, says] of [
    [{ device: "Headphones (USB)", firstChannel: 0 }, /not an output/],
    [{ device: SPEAKERS, firstChannel: 2 }, /past/],
    [{ device: "", firstChannel: 0 }, /name/],
    [{ device: DEMO, firstChannel: 0, channel: 1 }, /unknown field/],
  ]) {
    await assert.rejects(fake.invoke("set_local_output", { output }), says);
  }
  assert.deepEqual(
    fake.savedLocal().output,
    { device: DEMO, firstChannel: 2 },
    "a refused choice keeps the one in use",
  );
  const status = await fake.invoke("engine_status");
  assert.equal(status.localOutput.device, DEMO);

  const off = await fake.invoke("set_local_output", { output: null });
  assert.equal(off.localOutput.state, "off");
  assert.equal(fake.savedLocal().output, null);
});

test("with the fake stopped an output is kept for the next start", async () => {
  const fake = createFakeBridge();

  const kept = await fake.invoke("set_local_output", {
    output: { device: SPEAKERS, firstChannel: 0 },
  });

  assert.equal(kept.applied, false);
  assert.equal(kept.localOutput, null);
  await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");
  assert.equal(status.localOutput.state, "running");
  assert.equal(status.localOutput.device, SPEAKERS);
});

test("the fake merges a local mix change, keeps it by group id and announces it as local", async () => {
  const fake = createFakeBridge({ running: true });
  const heard = [];
  await fake.listen("mix-changed", (change) => heard.push(change));

  const mix = await fake.invoke("set_local_mix", {
    mix: { channelGains: [80, 40], groupLevels: [70, 30], volumePercent: 85 },
  });

  assert.equal(mix.volumePercent, 85);
  assert.deepEqual(mix.channelGains.slice(0, 2), [80, 40]);
  assert.deepEqual(heard.map(({ id, origin, address }) => [id, origin, address]), [
    [LOCAL_MIX, "desktop", undefined],
  ]);
  assert.equal(heard[0].mix.revision, mix.revision);
  const kept = fake.savedLocal().mix;
  assert.deepEqual(kept.groups, { drums: 70, vocals: 30 });
  assert.deepEqual(
    kept.channels.slice(0, 2).map(({ gain }) => gain),
    [80, 40],
  );
  assert.equal(kept.channels.length, sampleStatus().sourceChannels);
  assert.equal(kept.volumePercent, 85);
  await assert.rejects(
    fake.invoke("set_local_mix", { mix: { maxLevelPercent: 50 } }),
    /no level cap/,
  );
  await fake.invoke("stop_engine");
  await assert.rejects(fake.invoke("set_local_mix", { mix: { volumePercent: 1 } }), /not running/);
});

test("a fake start plays the kept local mix by group id and opens only an output it offers", async () => {
  const fake = createFakeBridge({
    savedLocal: {
      mix: {
        channels: [{ gain: 20, pan: 50, muted: false, solo: false }],
        groups: { vocals: 10, gone: 5 },
        volumePercent: 70,
        muted: false,
      },
      output: { device: "OUTPUT 1/2 (Volt 4)", firstChannel: 0 },
    },
  });

  await fake.invoke("start_engine", { config: {} });

  const status = await fake.invoke("engine_status");
  assert.equal(status.localMix.volumePercent, 70);
  assert.equal(status.localMix.channelGains[0], 20);
  assert.deepEqual(status.localMix.groupLevels.slice(0, 2), [100, 10]);
  assert.equal(status.localOutput.state, "off", "never another device in its place");
  assert.match(status.localOutput.notice, /OUTPUT 1\/2 \(Volt 4\) · outputs 1\/2, was not opened/);

  await fake.invoke("set_local_output", { output: { device: DEMO, firstChannel: 0 } });
  assert.equal((await fake.invoke("engine_status")).localOutput.notice, null);
});

test("the fake's meters carry the local mix's peak", async () => {
  const fake = createFakeBridge({ running: true });
  const heard = [];
  await fake.listen("meters", (meters) => heard.push(meters));

  fake.emitMeters();
  await fake.invoke("set_local_mix", { mix: { muted: true } });
  fake.emitMeters();

  assert.equal(typeof heard[0].local.left, "number");
  assert.deepEqual(heard[1].local, { left: 0, right: 0 }, "a muted local mix is silent");
});

test("a fake session keeps the local mix and a load takes it, an older one leaving the desk's own", async () => {
  const fake = createFakeBridge({ running: true, clock: () => 1000 });
  await fake.invoke("set_local_mix", { mix: { volumePercent: 40 } });
  await fake.invoke("set_local_output", { output: { device: SPEAKERS, firstChannel: 0 } });
  const { session } = await fake.invoke("save_session", { name: "Show" });
  await fake.invoke("set_local_mix", { mix: { volumePercent: 90 } });
  await fake.invoke("set_local_output", { output: null });

  const loaded = await fake.invoke("load_session", { id: session.id });

  assert.equal(loaded.localLoaded, true);
  assert.equal(loaded.localMix.volumePercent, 40);
  assert.equal(loaded.localOutputNotice, null);
  const status = await fake.invoke("engine_status");
  assert.equal(status.localMix.volumePercent, 40);
  assert.equal(status.localOutput.device, SPEAKERS);

  const older = createFakeBridge({
    running: true,
    savedSessions: [{ id: "old", name: "Old", savedAt: 1, session: {}, mixes: {} }],
  });
  await older.invoke("set_local_output", { output: { device: DEMO, firstChannel: 0 } });
  const kept = await older.invoke("load_session", { id: "old" });
  assert.equal(kept.localLoaded, false);
  assert.equal(kept.localMix, null);
  assert.deepEqual(older.savedLocal().output, { device: DEMO, firstChannel: 0 });
});

// The engine API and the console

test("the engine API sends the local mix and its output with the argument names the desktop takes", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => calls.push([command, args]),
    listen: async () => () => {},
  });

  await api.setLocalMix({ volumePercent: 80 });
  await api.outputDevices();
  await api.setLocalOutput({ device: DEMO, firstChannel: 2 });
  await api.setLocalOutput(null);

  assert.deepEqual(calls, [
    ["set_local_mix", { mix: { volumePercent: 80 } }],
    ["output_devices", undefined],
    ["set_local_output", { output: { device: DEMO, firstChannel: 2 } }],
    ["set_local_output", { output: null }],
  ]);
});

test("the console sends local mix edits with set_local_mix, one at a time, and takes the answer", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  fake.holdAnswers("set_local_mix");

  app.select(LOCAL_MIX);
  app.edit(LOCAL_MIX, { [editKey("channelGains", 1)]: 30 });
  app.edit(LOCAL_MIX, { [editKey("channelGains", 1)]: 35 });
  app.edit(LOCAL_MIX, { [editKey("volumePercent")]: 70 });
  await settle();

  assert.equal(commands(fake, "set_local_mix").length, 1, "one request in flight");
  assert.deepEqual(commands(fake, "set_local_mix")[0].args, {
    mix: { channelGains: [100, 30] },
  });
  assert.equal(viewMix(app.store.getState(), LOCAL_MIX).channelGains[1], 35);
  fake.releaseAnswers();
  await settle();
  fake.releaseAnswers();
  await settle();

  assert.deepEqual(commands(fake, "set_local_mix")[1].args, {
    mix: { channelGains: [100, 35], volumePercent: 70 },
  });
  assert.deepEqual(pendingEdits(app.store.getState(), LOCAL_MIX), {});
  assert.equal(app.store.getState().localMix.volumePercent, 70);
  assert.equal(commands(fake, "set_mix").length, 0, "never a musician's command");
  app.dispose();
});

test("the console lists the outputs and changes the local output, saying what it did", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.equal(await app.loadOutputs(), null);
  assert.deepEqual(app.store.getState().outputs.list, sampleOutputs());

  const chosen = await app.setLocalOutput({ device: DEMO, firstChannel: 2 });
  assert.deepEqual(chosen, {
    ok: true,
    message: "The local mix plays on Demo interface · outputs 3/4.",
  });
  assert.equal(app.store.getState().engine.status.localOutput.state, "running");

  const refused = await app.setLocalOutput({ device: "Nowhere", firstChannel: 0 });
  assert.equal(refused.ok, false);
  assert.match(refused.message, /^The local mix output was not changed: .*Nowhere/);

  await fake.invoke("stop_engine");
  app.store.dispatch(actions.engine("idle"));
  assert.notEqual(await app.loadOutputs(), null, "no engine, no outputs");
  assert.equal(app.store.getState().outputs.list, null);
  app.dispose();
});

test("a loaded session says it took the saved local mix, and why its output was not opened", async () => {
  const { formatSessionLoaded } = await import("../app/lib/sessions.js");
  const outcome = {
    session: { name: "Show" },
    applied: true,
    saved: true,
    saveError: null,
    mixes: [],
    skipped: [],
    refused: [],
    damaged: [],
    audioLoaded: false,
    audioSkipped: null,
    localLoaded: true,
    localMix: sampleStatus().localMix,
    localOutputNotice:
      "The saved local mix output, Volt 4 · outputs 1/2, was not opened: the running audio " +
      "driver does not offer it. The local mix plays nowhere until you choose an output.",
  };
  const words = { labelAt: (address) => address, nameOf: ({ key }) => key };

  const said = formatSessionLoaded(outcome, words);

  assert.ok(said.includes("The local mix is the saved one."), said);
  assert.ok(said.endsWith(outcome.localOutputNotice), said);
  const older = formatSessionLoaded(
    { ...outcome, localLoaded: false, localMix: null, localOutputNotice: null },
    words,
  );
  assert.ok(!older.includes("local mix"), older);
});
