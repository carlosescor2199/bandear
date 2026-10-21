// The shared reverb and delay: their settings held in range, the delay's time at a tempo, the
// tempo field, the effects panel's and the master strip's views, a channel's sends (effects.js);
// the bridge's commands; the fake bridge's sends, effects and returns (kept as the desktop keeps
// them, in the session and named sessions); and the console's actions.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { LOCAL_MIX } from "../app/lib/console-state.js";
import {
  channelSends,
  clampDelay,
  clampEffects,
  clampReverb,
  defaultEffects,
  delayTimeMs,
  effectsView,
  formatEffectSet,
  formatLevel,
  formatReturnSet,
  mixReturns,
  readTempo,
  returnsView,
} from "../app/lib/effects.js";
import { createFakeBridge } from "../app/lib/fake-bridge.js";
import { clampStrip, isUntouchedStrip } from "../app/lib/processing.js";

const NOW = Date.UTC(2026, 9, 20, 18, 0, 0);
const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";

test("the effects' settings are held in range, anything unreadable taking the default", () => {
  assert.deepEqual(clampEffects(undefined), defaultEffects());
  assert.deepEqual(clampReverb({ sizePercent: 400, dampingPercent: "x", preDelayMs: -5, widthPercent: 20 }), {
    sizePercent: 100,
    dampingPercent: 40,
    preDelayMs: 0,
    widthPercent: 20,
  });
  const delay = clampDelay({ timeMs: 9000, tempoBpm: 12, note: "whole bar", feedbackPercent: 99, pingPong: 1 });
  assert.deepEqual(delay, {
    timeMs: 2000,
    tempoBpm: 0,
    note: "quarter",
    feedbackPercent: 90,
    highCutHz: 6000,
    pingPong: true,
  });
  assert.equal(clampDelay({ tempoBpm: 500 }).tempoBpm, 300);
});

test("the delay follows a note at a tempo, held to two seconds", () => {
  assert.equal(delayTimeMs({ timeMs: 420 }), 420);
  assert.equal(delayTimeMs({ tempoBpm: 120, note: "quarter" }), 500);
  assert.equal(delayTimeMs({ tempoBpm: 120, note: "dottedEighth" }), 375);
  assert.equal(delayTimeMs({ tempoBpm: 30, note: "half" }), 2000);
  assert.ok(Math.abs(delayTimeMs({ tempoBpm: 90, note: "eighthTriplet" }) - 222.22) < 0.01);
});

test("the tempo field takes a tempo in range or nothing", () => {
  assert.deepEqual(readTempo(""), { ok: true, tempoBpm: 0 });
  assert.deepEqual(readTempo(" 128,5 "), { ok: true, tempoBpm: 128.5 });
  assert.equal(readTempo("12").ok, false);
  assert.equal(readTempo("fast").message, "A tempo is 30 to 300 bpm, or empty for none.");
});

test("a strip carries its sends, and one that only sends is kept", () => {
  assert.deepEqual(clampStrip({}).sends, { reverb: 0, delay: 0 });
  assert.deepEqual(clampStrip({ sends: { reverb: 140, delay: 12.4 } }).sends, { reverb: 100, delay: 12 });
  assert.equal(isUntouchedStrip({}), true);
  assert.equal(isUntouchedStrip({ sends: { reverb: 20 } }), false);
  assert.equal(formatLevel(0), "Off");
  assert.equal(formatLevel(35), "35%");
});

test("the panel and the master strip show what is on its way over what the desk keeps", () => {
  const state = {
    engine: {
      phase: "running",
      status: {
        effects: { reverb: { sizePercent: 70 }, delay: { tempoBpm: 120, note: "eighth" } },
        localFxReturns: { reverb: 10, delay: 0 },
      },
    },
    musicians: [{ address: LEAD, fxReturns: { reverb: 30, delay: 5 } }],
    effectsPending: { reverb: { sizePercent: 90 } },
    returnsPending: { [LEAD]: { delay: 50 } },
    processing: [{ channel: 2, settings: { sends: { reverb: 25, delay: 0 } } }],
    sendsPending: { 2: { delay: 40 } },
  };
  const view = effectsView(state);
  assert.equal(view.running, true);
  assert.equal(view.reverb.size, "90%", "the change on its way");
  assert.equal(view.delay.timeInForce, "250 ms (1/8 at 120 bpm)");
  assert.equal(view.delay.synced, true);
  assert.deepEqual(mixReturns(state, LEAD), { levels: { reverb: 30, delay: 50 }, pending: true });
  assert.deepEqual(mixReturns(state, LOCAL_MIX), { levels: { reverb: 10, delay: 0 }, pending: false });
  assert.deepEqual(
    returnsView(state, LOCAL_MIX).rows.map((row) => [row.label, row.text]),
    [
      ["Reverb return", "10%"],
      ["Delay return", "Off"],
    ],
  );
  assert.deepEqual(channelSends(state, 2), { reverb: 25, delay: 40 });
  assert.deepEqual(channelSends(state, 9), { reverb: 0, delay: 0 });
});

test("the desk's answers are said in words", () => {
  assert.deepEqual(formatEffectSet({ outcome: "set", effect: "delay", message: "The delay is set: 375 ms." }), {
    ok: true,
    message: "The delay is set: 375 ms.",
  });
  assert.equal(formatEffectSet({ outcome: "invalid", message: "Nope." }).ok, false);
  assert.deepEqual(
    formatReturnSet({ outcome: "set", effect: "reverb", returns: { reverb: 40, delay: 0 }, saveError: "disk full" }, "Ana"),
    { ok: true, message: "Ana's mix returns the reverb at 40%. It was not saved for the next start: disk full." },
  );
  assert.equal(
    formatReturnSet({ outcome: "set", effect: "delay", returns: { reverb: 0, delay: 0 } }, "The local mix").message,
    "The local mix returns no delay.",
  );
});

test("the fake keeps sends with the strips, shares them in a pair and keeps them through strip changes", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  const table = await api.channelTable();
  table[2] = { ...table[2], link: true };
  await api.setChannels(table);

  const sent = await api.setChannelSend(4, "reverb", 130);
  assert.equal(sent.outcome, "set");
  assert.equal(sent.message, "CH 3 and CH 4 are sending 100% to the reverb.");
  assert.deepEqual(sent.channels.map((strip) => [strip.channel, strip.settings.sends.reverb]), [
    [3, 100],
    [4, 100],
  ]);
  await api.setChannelProcessing(3, { compressor: { enabled: true, thresholdDb: -20 } });
  await api.applyStripPreset("Vocal", 3);
  const strips = await api.processing();
  assert.equal(strips[2].settings.sends.reverb, 100, "a strip change and a preset keep the send");
  assert.equal(strips[2].active, true);
  assert.deepEqual(fake.savedProcessing()[3].sends, { reverb: 100, delay: 0 });
  assert.equal((await api.setChannelSend(40, "delay", 5)).outcome, "invalid");
});

test("the fake sets the effects and each mix's returns as the desk does, and a named session carries them", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  const before = await api.status();
  assert.deepEqual(before.effects, defaultEffects());
  assert.deepEqual(before.localFxReturns, { reverb: 0, delay: 0 });
  assert.deepEqual(before.musicians[0].fxReturns, { reverb: 0, delay: 0 });

  const reverb = await api.setEffect("reverb", { sizePercent: 80, preDelayMs: 25 });
  assert.equal(reverb.message, "The reverb is set: size 80%, damping 40%, pre-delay 25 ms, width 100%.");
  const delay = await api.setEffect("delay", { tempoBpm: 120, note: "dottedEighth", pingPong: true });
  assert.equal(delay.message, "The delay is set: 375 ms, feedback 35%, ping-pong.");
  assert.equal(delay.effects.reverb.sizePercent, 80, "both effects in the answer");

  const row = before.musicians.find((musician) => musician.address === LEAD);
  const key = row.clientId ? `client:${row.clientId}` : `address:${row.address}`;
  const lead = await api.setMusicianFxReturn(key, "reverb", 45);
  assert.deepEqual(lead.returns, { reverb: 45, delay: 0 });
  assert.equal(lead.message, "This musician's mix returns the reverb at 45%.");
  const local = await api.setLocalFxReturn("delay", 20);
  assert.equal(local.message, "The local mix returns the delay at 20%.");
  const status = await api.status();
  assert.deepEqual(status.musicians.find((musician) => musician.address === LEAD).fxReturns, { reverb: 45, delay: 0 });
  assert.deepEqual(status.localFxReturns, { reverb: 0, delay: 20 });
  assert.equal(status.effects.delay.tempoBpm, 120);

  const saved = await api.saveSession("Wet show");
  await api.setEffect("reverb", { sizePercent: 10 });
  await api.setMusicianFxReturn(key, "reverb", 0);
  await api.loadSession(saved.session.id);
  const loaded = await api.status();
  assert.equal(loaded.effects.reverb.sizePercent, 80, "the show's effects");
  assert.deepEqual(loaded.musicians.find((musician) => musician.address === LEAD).fxReturns, { reverb: 45, delay: 0 });

  await assert.rejects(api.setMusicianFxReturn("Ana", "reverb", 5));
  const stopped = createEngineApi(createFakeBridge({ clock: () => NOW }));
  await assert.rejects(stopped.setEffect("reverb", {}), /not running/);
});

test("the console shows a send, an effect and a return at once and settles them on the desk's answer", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  await app.loadProcessing();
  const state = () => app.store.getState();

  const sending = app.editChannelSend(5, "reverb", 30);
  assert.deepEqual(channelSends(state(), 5), { reverb: 30, delay: 0 }, "shown at once");
  assert.deepEqual(await sending, { ok: true, message: "CH 5 is sending 30% to the reverb." });
  assert.deepEqual(state().sendsPending, {});
  assert.deepEqual(channelSends(state(), 5), { reverb: 30, delay: 0 }, "the desk's strip now");

  const first = app.editEffect("delay", { timeMs: 200 });
  const second = app.editEffect("delay", { tempoBpm: 100, note: "quarter" });
  assert.equal(effectsView(state()).delay.timeInForce, "600 ms (1/4 at 100 bpm)");
  await first;
  assert.equal((await second).ok, true);
  assert.deepEqual(state().effectsPending, {});
  assert.equal(effectsView(state()).delay.tempoBpm, 100, "the latest change stays");

  const returning = app.editMixReturn(BASS, "delay", 55);
  assert.deepEqual(mixReturns(state(), BASS), { levels: { reverb: 0, delay: 55 }, pending: true });
  assert.deepEqual(await returning, { ok: true, message: "Bass's mix returns the delay at 55%." });
  assert.deepEqual(mixReturns(state(), BASS), { levels: { reverb: 0, delay: 55 }, pending: false });
  assert.equal((await app.editMixReturn(LOCAL_MIX, "reverb", 15)).message, "The local mix returns the reverb at 15%.");
  assert.equal(mixReturns(state(), LOCAL_MIX).levels.reverb, 15);

  const gone = await app.editMixReturn("10.0.0.9:50000", "reverb", 5);
  assert.deepEqual(gone, { ok: false, message: "10.0.0.9:50000's reverb return was not set: 10.0.0.9:50000 is not here." });
  app.dispose?.();
});
