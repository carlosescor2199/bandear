// The channel processing editor's wiring (processingEditor, processing-editor.js), without a DOM:
// which strip it opens, edits through the console, A/B, the debounced equaliser curve, presets and
// copies, against the fake bridge.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { channelStrip } from "../app/lib/console-state.js";
import { createFakeBridge, factoryStripPresets } from "../app/lib/fake-bridge.js";
import { processingEditor } from "../app/lib/processing-editor.js";
import { defaultStrip } from "../app/lib/processing.js";

const settle = () => new Promise((resolve) => setImmediate(resolve));

// Timers the test runs by hand.
function manualTimers() {
  const waiting = new Map();
  let next = 1;
  return {
    setTimeout(run) {
      waiting.set(next, run);
      return next++;
    },
    clearTimeout(id) {
      waiting.delete(id);
    },
    pending: () => waiting.size,
    runAll() {
      const runs = [...waiting.values()];
      waiting.clear();
      runs.forEach((run) => run());
    },
  };
}

async function editor({ channel = null, link = false } = {}) {
  const fake = createFakeBridge({ running: true });
  if (link) {
    const api = createEngineApi(fake);
    const table = await api.channelTable();
    table[2] = { ...table[2], link: true };
    await api.setChannels(table);
  }
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await settle();
  const timers = manualTimers();
  let changes = 0;
  const commands = processingEditor({
    app,
    channel,
    timers,
    changed: () => {
      changes += 1;
    },
  });
  return { fake, app, timers, commands, changes: () => changes };
}

const sent = (fake, command) => fake.calls.filter((call) => call.command === command);

test("the editor opens the channel asked for, a pair on its first side, and steps between strips", async () => {
  const { app, commands } = await editor({ channel: 4, link: true });
  let view = commands.view();
  assert.equal(view.known, true);
  assert.equal(view.channel, 3, "CH 4 is the second side of the pair CH 3-4");
  assert.equal(view.choice.label.startsWith("CH 3–4"), true, view.choice.label);
  assert.deepEqual(view.sides, [3, 4]);
  assert.equal(commands.step(1), 5);
  assert.equal(commands.step(-1), 3);
  assert.equal(commands.step(-1), 2);
  assert.equal(commands.open(99), 2, "a channel the device does not have keeps the one open");
  view = commands.view();
  assert.deepEqual(view.settings, defaultStrip());
  assert.equal(view.active, false);
  commands.dispose();
  app.dispose();
});

test("a slider's change goes to the desk as the whole strip, held in range, and is shown at once", async () => {
  const { fake, app, commands, changes } = await editor({ channel: 1 });
  const editing = commands.edit("compressor.enabled", true);
  assert.equal(commands.view().settings.compressor.enabled, true, "shown before the answer");
  assert.equal(commands.view().pending, true);
  const outcome = await editing;
  assert.equal(outcome.ok, true, outcome.message);
  await commands.edit("compressor.thresholdDb", -500);
  const strip = channelStrip(app.store.getState(), 1);
  assert.equal(strip.settings.compressor.thresholdDb, -60, "held to the compressor's range");
  assert.equal(strip.active, true);
  assert.equal(sent(fake, "set_channel_processing").at(-1).args.channel, 1);
  assert.equal(changes(), 0, "a sent edit is drawn from the store, never said");
  commands.dispose();
  app.dispose();
});

test("A/B keeps the strip, sends a neutral one and puts the kept one back", async () => {
  const { app, commands } = await editor({ channel: 2 });
  await commands.edit("gate.enabled", true);
  const working = structuredClone(commands.view().settings);
  assert.equal(commands.view().canBypass, true);

  assert.equal((await commands.toggleBypass()).ok, true);
  let view = commands.view();
  assert.equal(view.bypassed, true);
  assert.deepEqual(view.settings, working, "the kept strip is shown, to put back");
  assert.equal(channelStrip(app.store.getState(), 2).active, false, "the desk plays CH 2 untouched");
  assert.equal((await commands.edit("gate.thresholdDb", -10)).ok, false, "no edit while bypassed");

  assert.equal((await commands.toggleBypass()).ok, true);
  view = commands.view();
  assert.equal(view.bypassed, false);
  assert.deepEqual(channelStrip(app.store.getState(), 2).settings, working);

  await commands.toggleBypass();
  await commands.dispose();
  assert.deepEqual(
    channelStrip(app.store.getState(), 2).settings,
    working,
    "leaving the editor ends the bypass",
  );
  app.dispose();
});

test("a strip changed elsewhere while bypassed ends the bypass and is not overwritten", async () => {
  const { app, commands } = await editor({ channel: 2 });
  await commands.edit("gate.enabled", true);
  await commands.toggleBypass();
  assert.equal((await app.applyStripPreset("Kick", 2)).ok, true);
  const view = commands.view();
  assert.equal(view.bypassed, false);
  assert.deepEqual(view.settings, factoryStripPresets()[1].settings);
  await commands.dispose();
  assert.deepEqual(channelStrip(app.store.getState(), 2).settings, factoryStripPresets()[1].settings);
  app.dispose();
});

test("the curve is drawn at once and asked of the desk once the edits pause", async () => {
  const { fake, app, timers, commands } = await editor({ channel: 1 });
  assert.equal(commands.view().curve.points.length, 128);
  commands.requestCurve();
  await commands.edit("eq.bands.1.gainDb", 6);
  commands.requestCurve();
  commands.requestCurve();
  assert.equal(timers.pending(), 1, "one ask waiting");
  assert.equal(commands.view().curve.fromDesk, false);
  timers.runAll();
  await settle();
  assert.equal(sent(fake, "eq_response").length, 1);
  assert.equal(sent(fake, "eq_response")[0].args.settings.bands[1].gainDb, 6);
  assert.equal(commands.view().curve.fromDesk, true);
  const peak = Math.max(...commands.view().curve.points.map((point) => point.db));
  assert.ok(peak > 5.9 && peak < 6.1, String(peak));
  await commands.edit("eq.bands.1.gainDb", -3);
  assert.equal(commands.view().curve.fromDesk, false, "the desk's curve is for the old strip");
  commands.dispose();
  app.dispose();
});

test("reset, copy and presets say what they did; a built-in preset cannot be deleted", async () => {
  const { app, commands } = await editor({ channel: 1 });
  let outcome = await commands.applyPreset("Vocal");
  assert.equal(outcome.ok, true, outcome.message);
  assert.deepEqual(commands.view().settings, factoryStripPresets()[0].settings);
  assert.equal(commands.view().outcome, outcome);

  outcome = await commands.copyTo([5, 6]);
  assert.equal(outcome.ok, true, outcome.message);
  assert.deepEqual(channelStrip(app.store.getState(), 6).settings, factoryStripPresets()[0].settings);
  assert.equal((await commands.copyTo([])).ok, false, "no channel picked");

  outcome = await commands.savePreset("  ");
  assert.equal(outcome.ok, false);
  outcome = await commands.savePreset("Mine");
  assert.equal(outcome.ok, true, outcome.message);
  const mine = commands.view().presets.find((preset) => preset.name === "Mine");
  assert.deepEqual(mine, { name: "Mine", label: "Mine", factory: false });
  assert.equal(commands.view().presets[0].label, "Vocal (built-in)");
  assert.equal((await commands.deletePreset("Vocal")).ok, false);
  assert.equal((await commands.deletePreset("Mine")).ok, true);

  outcome = await commands.reset();
  assert.equal(outcome.ok, true, outcome.message);
  assert.equal(commands.view().active, false);
  commands.dispose();
  app.dispose();
});

test("strips not read yet say so, and a read can be asked for again", async () => {
  const fake = createFakeBridge({ running: true });
  fake.failNext("processing", "the desk is busy");
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await settle();
  const commands = processingEditor({ app, timers: manualTimers() });
  assert.equal(commands.view().unread, true);
  assert.equal(commands.view().channel, 1);
  assert.equal(await commands.readAgain(), null);
  assert.equal(commands.view().unread, false);

  await app.stop();
  await settle();
  assert.equal(commands.view().known, false, "no engine, no inputs to open");
  commands.dispose();
  app.dispose();
});
