// Each input's channel processing at the desk: the strip's shape, ranges and response
// (processing.js), the bridge's processing commands, the fake bridge's mirror of them (a running
// engine for changes, a linked pair as one strip, the built-in presets protected, the processed
// meters and the processing a saved session carries) and the console's slices and actions.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { actions, channelStrip, consoleReducer, initialState } from "../app/lib/console-state.js";
import {
  createFakeBridge,
  factoryStripPresets,
  sampleStatus,
  syntheticMeters,
} from "../app/lib/fake-bridge.js";
import {
  STRIP_CHANNELS,
  channelList,
  clampStrip,
  defaultStrip,
  eqMagnitudeDb,
  eqResponseOf,
  formatProcessingSet,
  formatStripPresetsSet,
  isNeutralStrip,
} from "../app/lib/processing.js";

const settle = () => new Promise((resolve) => setImmediate(resolve));

/** A strip with its compressor on at `thresholdDb`. */
function compressed(thresholdDb, changes = {}) {
  const strip = defaultStrip();
  strip.compressor = { ...strip.compressor, enabled: true, thresholdDb, ...changes };
  return strip;
}

/** A strip whose first peak band boosts by `gainDb`. */
function boosted(gainDb) {
  const strip = defaultStrip();
  strip.eq.bands[1].gainDb = gainDb;
  return strip;
}

// Links CH 3 and CH 4 as a stereo pair through the desk's channel table.
async function linkThreeAndFour(api) {
  const table = await api.channelTable();
  table[2] = { ...table[2], link: true };
  await api.setChannels(table);
}

test("a strip takes the desk's defaults for what it leaves out and holds every number in range", () => {
  const neutral = clampStrip({});
  assert.deepEqual(neutral, defaultStrip());
  assert.ok(isNeutralStrip(neutral));
  assert.deepEqual(
    neutral.eq.bands.map((band) => band.frequencyHz),
    [100, 400, 2500, 8000],
  );

  const wild = clampStrip({
    eq: { highPass: { enabled: true, frequencyHz: 5 }, bands: [{ gainDb: 40 }, { q: "x" }] },
    gate: { thresholdDb: -200, rangeDb: 99 },
    compressor: { enabled: true, ratio: 50, makeupDb: Number.NaN },
  });
  assert.equal(wild.eq.highPass.frequencyHz, 20);
  assert.equal(wild.eq.bands[0].gainDb, 18);
  assert.equal(wild.eq.bands[0].frequencyHz, 100, "the band's own default frequency stays");
  assert.equal(wild.eq.bands[1].q, 1, "not a number takes the default");
  assert.equal(wild.gate.thresholdDb, -80);
  assert.equal(wild.gate.rangeDb, 80);
  assert.equal(wild.compressor.ratio, 20);
  assert.equal(wild.compressor.makeupDb, 0);

  assert.ok(!isNeutralStrip(compressed(-20)));
  assert.ok(isNeutralStrip(compressed(-20, { ratio: 1 })), "a ratio of 1 with no makeup does nothing");
  assert.ok(!isNeutralStrip(boosted(3)));
  const gate = defaultStrip();
  gate.gate = { ...gate.gate, enabled: true, rangeDb: 0 };
  assert.ok(isNeutralStrip(gate), "a gate closing by 0 dB does nothing");
});

test("the equaliser's response is worked out as the desk works it out", () => {
  const flat = eqResponseOf(defaultStrip().eq);
  assert.equal(flat.length, 128);
  assert.equal(flat[0].freqHz, 20);
  assert.ok(Math.abs(flat[127].freqHz - 20000) < 1e-6);
  assert.ok(flat.every((point) => point.db === 0));

  const peak = defaultStrip().eq;
  peak.bands[2] = { enabled: true, frequencyHz: 2000, gainDb: 6, q: 1 };
  assert.ok(Math.abs(eqMagnitudeDb(peak, 2000) - 6) < 0.01);
  assert.ok(Math.abs(eqMagnitudeDb(peak, 20)) < 0.1);
  const points = eqResponseOf(peak, 4);
  assert.ok(Math.abs(points[2].freqHz - 2000) < 1e-6);
  assert.ok(Math.abs(points[2].db - 6) < 0.01);

  const highPass = defaultStrip().eq;
  highPass.highPass = { enabled: true, frequencyHz: 200, q: Math.SQRT1_2 };
  assert.ok(Math.abs(eqMagnitudeDb(highPass, 200) + 3.01) < 0.05, "-3 dB at the corner");
  assert.ok(eqMagnitudeDb(highPass, 20) < -30);

  const shelf = defaultStrip().eq;
  shelf.bands[0].gainDb = 6;
  assert.ok(Math.abs(eqMagnitudeDb(shelf, 20) - 6) < 0.5);
  assert.ok(Math.abs(eqMagnitudeDb(shelf, 20000)) < 0.1);

  assert.equal(eqResponseOf(peak, 1).length, 2);
  assert.equal(eqResponseOf(peak, 5000).length, 1024);
});

test("channels and the desk's answers are said in words", () => {
  assert.equal(channelList([3]), "CH 3");
  assert.equal(channelList([3, 4]), "CH 3 and CH 4");
  assert.equal(channelList([1, 2, 5]), "CH 1, CH 2 and CH 5");
  assert.deepEqual(formatProcessingSet({ outcome: "set", message: "CH 1 is set." }), {
    ok: true,
    message: "CH 1 is set.",
  });
  assert.deepEqual(formatProcessingSet({ outcome: "invalid", message: "Not a channel." }), {
    ok: false,
    message: "Not a channel.",
  });
  assert.equal(formatProcessingSet({ outcome: "notFound" }).ok, false);
  for (const outcome of ["saved", "replaced", "deleted"]) {
    assert.equal(formatStripPresetsSet({ outcome, message: "m" }).ok, true, outcome);
  }
  for (const outcome of ["invalid", "factory", "notFound"]) {
    assert.equal(formatStripPresetsSet({ outcome, message: "m" }).ok, false, outcome);
  }
});

test("the bridge asks the desktop for the processing commands with their argument names", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);
  const strip = compressed(-20);

  await api.processing();
  await api.channelProcessing(2);
  await api.setChannelProcessing(2, strip);
  await api.resetChannelProcessing(2);
  await api.copyChannelProcessing(1, [2, 3]);
  await api.stripPresets();
  await api.saveStripPreset("Mine", strip);
  await api.deleteStripPreset("Mine");
  await api.applyStripPreset("Vocal", 1);
  await api.eqResponse(strip.eq, 64);
  await api.eqResponse(strip.eq);

  assert.deepEqual(fake.calls, [
    { command: "processing", args: undefined },
    { command: "channel_processing", args: { channel: 2 } },
    { command: "set_channel_processing", args: { channel: 2, settings: strip } },
    { command: "reset_channel_processing", args: { channel: 2 } },
    { command: "copy_channel_processing", args: { from: 1, to: [2, 3] } },
    { command: "strip_presets", args: undefined },
    { command: "save_strip_preset", args: { name: "Mine", settings: strip } },
    { command: "delete_strip_preset", args: { name: "Mine" } },
    { command: "apply_strip_preset", args: { name: "Vocal", channel: 1 } },
    { command: "eq_response", args: { settings: strip.eq, points: 64 } },
    { command: "eq_response", args: { settings: strip.eq, points: null } },
  ]);
});

test("the fake bridge changes a strip on the running engine, in range, a linked pair whole", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);

  const all = await api.processing();
  assert.equal(all.length, STRIP_CHANNELS);
  assert.deepEqual(all[0], { channel: 1, settings: defaultStrip(), active: false });

  const set = await api.setChannelProcessing(1, boosted(40));
  assert.equal(set.outcome, "set");
  assert.deepEqual(set.channels, [{ channel: 1, settings: boosted(18), active: true }]);
  assert.equal(set.message, "CH 1 is set to the new processing.");
  assert.deepEqual(fake.savedProcessing(), { 1: boosted(18) }, "kept by channel number");

  await linkThreeAndFour(api);
  const pair = await api.setChannelProcessing(4, compressed(-30));
  assert.deepEqual(
    pair.channels.map((strip) => strip.channel),
    [3, 4],
  );
  assert.equal(pair.message, "CH 3 and CH 4 are set to the new processing.");

  const copied = await api.copyChannelProcessing(1, [3, 1, 8]);
  assert.deepEqual(
    copied.channels.map((strip) => strip.channel),
    [3, 4, 8],
    "onto the pair whole, CH 1 itself left out",
  );
  assert.equal(copied.message, "CH 1's processing is on CH 3, CH 4 and CH 8 now.");
  assert.equal((await api.copyChannelProcessing(1, [1])).outcome, "invalid");

  const reset = await api.resetChannelProcessing(3);
  assert.deepEqual(
    reset.channels.map((strip) => [strip.channel, strip.active]),
    [
      [3, false],
      [4, false],
    ],
  );
  assert.deepEqual(Object.keys(fake.savedProcessing()), ["1", "8"], "a neutral strip is not kept");

  const outside = await api.setChannelProcessing(33, boosted(1));
  assert.equal(outside.outcome, "invalid");
  assert.ok(outside.message.includes("CH 1-32"), outside.message);
  await assert.rejects(api.channelProcessing(0), (error) => String(error).includes("CH 1-32"));

  await api.stop();
  await assert.rejects(
    api.setChannelProcessing(1, boosted(2)),
    (error) => error === "the engine is not running",
  );
  assert.equal(
    (await api.processing())[0].settings.eq.bands[1].gainDb,
    18,
    "with no engine the desk's kept processing is read",
  );
});

test("the fake bridge keeps the engineer's strip presets and never touches the built-in ones", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);

  const listed = await api.stripPresets();
  assert.deepEqual(
    listed.map((preset) => preset.name),
    ["Vocal", "Kick", "Bass DI", "Acoustic guitar", "Gentle compression"],
  );
  assert.ok(listed.every((preset) => preset.factory && !isNeutralStrip(preset.settings)));
  assert.deepEqual(listed, factoryStripPresets());

  const saved = await api.saveStripPreset("  Lead vocal ", boosted(30));
  assert.equal(saved.outcome, "saved");
  assert.deepEqual(saved.preset, { name: "Lead vocal", settings: boosted(18), factory: false });
  assert.equal(saved.presets.length, 6);
  const replaced = await api.saveStripPreset("LEAD VOCAL", compressed(-12));
  assert.equal(replaced.outcome, "replaced");
  assert.deepEqual(fake.savedStripPresets(), [{ name: "LEAD VOCAL", settings: compressed(-12) }]);

  for (const name of ["vocal", " KICK "]) {
    const refused = await api.saveStripPreset(name, boosted(1));
    assert.equal(refused.outcome, "invalid", name);
    assert.ok(refused.message.includes("built-in"), refused.message);
    assert.equal((await api.deleteStripPreset(name)).outcome, "factory", name);
  }
  assert.equal((await api.saveStripPreset(" ", boosted(1))).outcome, "invalid");
  assert.equal((await api.saveStripPreset("x".repeat(41), boosted(1))).outcome, "invalid");

  const applied = await api.applyStripPreset("lead vocal", 2);
  assert.equal(applied.outcome, "set");
  assert.equal(applied.message, '"LEAD VOCAL" is on CH 2 now.');
  assert.deepEqual((await api.channelProcessing(2)).settings, compressed(-12));
  assert.equal((await api.applyStripPreset("Tuba", 2)).outcome, "notFound");

  const deleted = await api.deleteStripPreset("Lead Vocal");
  assert.equal(deleted.outcome, "deleted");
  assert.equal(deleted.presets.length, 5);
  assert.equal((await api.deleteStripPreset("Lead Vocal")).outcome, "notFound");
});

test("the fake bridge works out the equaliser's response and meters the processed inputs", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);
  const eq = boosted(6).eq;

  const curve = await api.eqResponse(eq, 32);
  assert.equal(curve.length, 32);
  assert.deepEqual(curve, eqResponseOf(eq, 32));
  assert.equal((await api.eqResponse(eq)).length, 128);

  const status = sampleStatus();
  const plain = syntheticMeters(status, 1234);
  assert.deepEqual(plain.processed, plain.channels, "no strip: the processed level is the input's");
  assert.ok(plain.gateReductionDb.every((db) => db === 0));
  assert.ok(plain.compressorReductionDb.every((db) => db === 0));

  const squeezed = syntheticMeters(status, 1234, [compressed(-60, { ratio: 4 })]);
  assert.ok(squeezed.compressorReductionDb[0] > 0, `${squeezed.compressorReductionDb[0]}`);
  assert.ok(squeezed.processed[0] < squeezed.channels[0], "turned down");
  assert.deepEqual(squeezed.processed.slice(1), squeezed.channels.slice(1));

  const gate = defaultStrip();
  gate.gate = { ...gate.gate, enabled: true, thresholdDb: 0, rangeDb: 30 };
  const gated = syntheticMeters(status, 1234, [gate]);
  assert.equal(gated.gateReductionDb[0], 30, "closed below its threshold");

  const meters = [];
  await fake.listen("meters", (payload) => meters.push(payload));
  await api.setChannelProcessing(1, compressed(-60, { ratio: 4 }));
  fake.emitMeters();
  assert.ok(meters[0].compressorReductionDb[0] > 0);
});

test("a saved session carries the processing and loading one replaces every strip", async () => {
  const fake = createFakeBridge({ running: true, clock: () => 1_000 });
  const api = createEngineApi(fake);
  await api.setChannelProcessing(2, compressed(-24));
  await api.saveStripPreset("Mine", boosted(2));
  const saved = await api.saveSession("Show");
  await api.resetChannelProcessing(2);
  await api.setChannelProcessing(5, boosted(4));

  await api.loadSession(saved.session.id);

  assert.deepEqual(fake.savedProcessing(), { 2: compressed(-24) }, "CH 5 goes back to none");
  assert.equal(fake.savedStripPresets().length, 1, "the desk's own strip presets stay");

  const empty = createFakeBridge({
    running: true,
    savedProcessing: { 3: compressed(-10) },
    savedSessions: [{ id: "s9", name: "Old show", savedAt: 5, session: { groups: [], channels: [] } }],
  });
  const emptyApi = createEngineApi(empty);
  assert.deepEqual((await emptyApi.channelProcessing(3)).settings, compressed(-10));
  await emptyApi.loadSession("s9");
  assert.deepEqual(empty.savedProcessing(), {}, "a show with no processing leaves every strip neutral");
});

test("the console reads the strips and presets once connected and keeps answers in its store", async () => {
  const fake = createFakeBridge({ running: true, savedProcessing: { 2: compressed(-18) } });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await settle();

  const state = app.store.getState();
  assert.equal(state.processing.length, STRIP_CHANNELS);
  assert.deepEqual(channelStrip(state, 2), {
    channel: 2,
    settings: compressed(-18),
    active: true,
    pending: false,
  });
  assert.equal(state.stripPresets.length, 5);

  assert.deepEqual(await app.resetChannelProcessing(2), {
    ok: true,
    message: "CH 2 is back to no processing.",
  });
  assert.equal(channelStrip(app.store.getState(), 2).active, false);

  assert.deepEqual(await app.applyStripPreset("Kick", 1), {
    ok: true,
    message: '"Kick" is on CH 1 now.',
  });
  assert.deepEqual(channelStrip(app.store.getState(), 1).settings, factoryStripPresets()[1].settings);
  const copied = await app.copyChannelProcessing(1, [6]);
  assert.equal(copied.ok, true, copied.message);
  assert.equal(channelStrip(app.store.getState(), 6).active, true);
  const missing = await app.applyStripPreset("Tuba", 1);
  assert.equal(missing.ok, false);
  assert.ok(missing.message.includes("Tuba"), missing.message);

  assert.deepEqual(await app.saveStripPreset("Mine", boosted(3)), {
    ok: true,
    message: 'Saved the strip preset "Mine".',
  });
  assert.equal(app.store.getState().stripPresets.length, 6);
  const refused = await app.deleteStripPreset("Vocal");
  assert.equal(refused.ok, false);
  assert.ok(refused.message.includes("cannot be deleted"), refused.message);
  assert.equal((await app.deleteStripPreset("Mine")).ok, true);
  assert.equal(app.store.getState().stripPresets.length, 5);

  const curve = await app.loadEqResponse(1, boosted(6).eq, 16);
  assert.equal(curve.ok, true);
  assert.equal(curve.points.length, 16);
  assert.deepEqual(app.store.getState().eqCurves[1], curve.points);

  await fake.invoke("stop_engine");
  const stopped = await app.resetChannelProcessing(1);
  assert.equal(stopped.ok, false);
  assert.equal(stopped.message, "CH 1's processing was not reset: the engine is not running.");
  app.dispose();
});

test("a dragged strip edit shows at once and sends only its latest value after the one in flight", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await settle();
  fake.holdAnswers("set_channel_processing");

  const first = app.editProcessing(1, boosted(1));
  app.editProcessing(1, boosted(2));
  const last = app.editProcessing(1, boosted(3));
  assert.deepEqual(channelStrip(app.store.getState(), 1), {
    channel: 1,
    settings: boosted(3),
    active: true,
    pending: true,
  });
  const sends = () => fake.calls.filter((call) => call.command === "set_channel_processing");
  assert.equal(sends().length, 1, "one in flight");

  fake.releaseAnswers();
  await settle();
  assert.deepEqual(
    sends().map((call) => call.args.settings.eq.bands[1].gainDb),
    [1, 3],
    "the edit in between is never sent",
  );
  assert.equal(channelStrip(app.store.getState(), 1).pending, true, "the latest is still on its way");
  fake.releaseAnswers();
  assert.deepEqual(await last, { ok: true, message: "CH 1 is set to the new processing." });
  assert.deepEqual(await first, await last);
  assert.deepEqual(channelStrip(app.store.getState(), 1), {
    channel: 1,
    settings: boosted(3),
    active: true,
    pending: false,
  });

  const releasing = setInterval(() => fake.releaseAnswers(), 1);
  const outside = await app.editProcessing(40, boosted(1));
  assert.equal(outside.ok, false);
  assert.ok(outside.message.includes("CH 1-32"), outside.message);
  assert.deepEqual(app.store.getState().processingError.channel, 40);
  await fake.invoke("stop_engine");
  const stopped = await app.editProcessing(2, boosted(1));
  clearInterval(releasing);
  assert.equal(stopped.ok, false);
  assert.equal(stopped.message, "CH 2's processing was not changed: the engine is not running.");
  assert.equal(channelStrip(app.store.getState(), 2).pending, false, "a refusal settles the edit");
  assert.equal(app.store.getState().processingError.channel, 2);
  app.dispose();
});

test("a loaded session replaces the strips the console shows and drops the strip edits waiting", async () => {
  const fake = createFakeBridge({ running: true, clock: () => 1_000 });
  const api = createEngineApi(fake);
  await api.setChannelProcessing(3, compressed(-20));
  const saved = await api.saveSession("Show");
  await api.resetChannelProcessing(3);
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await settle();
  assert.equal(channelStrip(app.store.getState(), 3).active, false);

  const loaded = await app.loadSession(saved.session.id);

  assert.equal(loaded.ok, true, loaded.message);
  assert.deepEqual(channelStrip(app.store.getState(), 3).settings, compressed(-20));
  const next = consoleReducer(
    consoleReducer(initialState, actions.processingEdit(4, boosted(1))),
    actions.sessionLoaded({ presets: [] }),
  );
  assert.deepEqual(next.processingPending, {}, "a load drops the strip edits waiting");
  app.dispose();
});
