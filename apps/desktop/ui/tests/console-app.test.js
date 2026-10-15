import test from "node:test";
import assert from "node:assert/strict";

import { createConsoleApp } from "../app/lib/console-app.js";
import { musicianByAddress, pendingEdits, viewMix } from "../app/lib/console-state.js";
import { createFakeBridge, sampleStatus } from "../app/lib/fake-bridge.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
}

async function connected(options = {}) {
  const fake = createFakeBridge({ running: true, ...options });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  return { fake, app };
}

test("connecting to a running engine shows it and starts nothing", async () => {
  const { fake, app } = await connected();

  const state = app.store.getState();
  assert.equal(state.engine.phase, "running");
  assert.equal(state.musicians.length, sampleStatus().musicians.length);
  assert.equal(state.selected, LEAD);
  assert.equal(commands(fake, "start_engine").length, 0);
  app.dispose();
});

test("connecting to a stopped engine starts it as the old window did, never on ASIO", async () => {
  const fake = createFakeBridge();
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.deepEqual(commands(fake, "start_engine").map(({ args }) => args), [
    {
      config: {
        deviceFilter: null,
        targets: [],
        controlPort: 50001,
        groups: [],
        host: "auto",
        bufferFrames: 256,
      },
    },
  ]);
  assert.equal(app.store.getState().engine.phase, "running");
  app.dispose();
});

test("without auto start a stopped engine waits for the engineer", async () => {
  const fake = createFakeBridge();
  const app = createConsoleApp({ bridge: fake, autoStart: false });
  await app.ready;

  assert.equal(app.store.getState().engine.phase, "idle");
  assert.equal(commands(fake, "start_engine").length, 0);

  await app.start();
  assert.equal(app.store.getState().engine.phase, "running");
  app.dispose();
});

test("a failed start shows its error, and a late status of the engine it replaced is ignored", async () => {
  const { fake, app } = await connected();
  const before = await fake.invoke("engine_status");
  fake.failStart("No input device is available.");

  await app.start();
  fake.emit("status", before);

  const { engine } = app.store.getState();
  assert.equal(engine.phase, "error");
  assert.equal(engine.error, "No input device is available.");
  assert.equal(app.store.getState().musicians.length, 0);
  app.dispose();
});

test("after a stop, late events of the stopped engine are ignored", async () => {
  const { fake, app } = await connected();
  const before = await fake.invoke("engine_status");
  const meters = [];
  app.onMeters((payload) => meters.push(payload));

  await app.stop();
  fake.emit("status", before);
  fake.emit("mix-changed", { run: before.run, address: LEAD, origin: "phone", mix: {} });
  fake.emit("meters", { run: before.run, channels: [50], musicians: {} });

  assert.equal(app.store.getState().engine.phase, "idle");
  assert.deepEqual(app.store.getState().musicians, []);
  assert.deepEqual(meters, []);
  app.dispose();
});

test("meters of the running engine reach every listener until it unsubscribes", async () => {
  const { fake, app } = await connected();
  const seen = [];
  const unsubscribe = app.onMeters((payload) => seen.push(payload.channels.length));

  fake.emitMeters();
  unsubscribe();
  fake.emitMeters();

  assert.deepEqual(seen, [sampleStatus().sourceChannels]);
  app.dispose();
});

test("fader moves reach set_mix one at a time, and the latest value always goes out", async () => {
  const { fake, app } = await connected();
  fake.holdAnswers("set_mix");

  app.edit(LEAD, { "channelGains.0": 10 });
  app.edit(LEAD, { "channelGains.0": 20 });
  app.edit(LEAD, { "channelGains.0": 30 });
  app.edit(LEAD, { "channelGains.1": 40 });
  assert.equal(commands(fake, "set_mix").length, 1, "one request in flight per musician");

  fake.releaseAnswers();
  await settle();
  const sent = commands(fake, "set_mix").map(({ args }) => args.mix);
  assert.deepEqual(sent, [{ channelGains: [10] }, { channelGains: [30, 40] }]);

  fake.releaseAnswers();
  await settle();
  const state = app.store.getState();
  assert.deepEqual(pendingEdits(state, LEAD), {});
  assert.deepEqual(viewMix(state, LEAD).channelGains.slice(0, 2), [30, 40]);
  assert.deepEqual(musicianByAddress(state, LEAD).mix.channelGains.slice(0, 2), [30, 40]);
  app.dispose();
});

test("when the answer comes before its event, the fader never shows an older value", async () => {
  const { fake, app } = await connected({ answersFirst: true });
  const shown = [];
  app.store.subscribe((state) => shown.push(viewMix(state, LEAD).channelGains[1]));

  app.edit(LEAD, { "channelGains.1": 40 });
  await new Promise((resolve) => setTimeout(resolve, 20));

  assert.ok(shown.length >= 2, "the edit, the answer and the event all reached the store");
  assert.ok(
    shown.every((value) => value === 40),
    `shown: ${shown.join(", ")}`,
  );
  assert.deepEqual(pendingEdits(app.store.getState(), LEAD), {});
  assert.equal(musicianByAddress(app.store.getState(), LEAD).mix.channelGains[1], 40);
  assert.equal(commands(fake, "set_mix").length, 1);
  app.dispose();
});

test("a phone change between the desk's request and its answer stays on screen", async () => {
  const { fake, app } = await connected();
  fake.holdAnswers("set_mix");
  const gains = sampleStatus().musicians[0].channelGains;

  app.edit(LEAD, { "channelGains.1": 40 });
  fake.changeFromPhone(LEAD, { channelGains: [gains[0], 90] });
  fake.releaseAnswers();
  await settle();

  assert.equal(viewMix(app.store.getState(), LEAD).channelGains[1], 90);
  app.dispose();
});

test("a refused change says so and the strip falls back to the engine's value", async () => {
  const { fake, app } = await connected();
  await fake.invoke("stop_engine");

  app.edit(BASS, { volumePercent: 10 });
  await settle();

  const state = app.store.getState();
  assert.deepEqual(state.mixError, { address: BASS, message: "the engine is not running" });
  assert.equal(viewMix(state, BASS).volumePercent, 100);
  app.dispose();
});

test("a failed stop keeps the engine's run, shows the error, and its events still count", async () => {
  const { fake, app } = await connected();
  fake.failNext("stop_engine", "could not stop the capture");

  await app.stop();
  fake.changeFromPhone(LEAD, { volumePercent: 5 });

  const state = app.store.getState();
  assert.equal(state.engine.phase, "running", "engine_status says it still runs");
  assert.equal(state.musicians.length, sampleStatus().musicians.length);
  assert.deepEqual(state.commandError, { action: "stop", message: "could not stop the capture" });
  assert.equal(viewMix(state, LEAD).volumePercent, 5);
  app.dispose();
});

test("a stop that reports a failure but did stop the engine ends the run", async () => {
  const { fake, app } = await connected();
  fake.failNext("stop_engine", "stopped, but late", { afterEffect: true });

  await app.stop();

  assert.equal(app.store.getState().engine.phase, "idle");
  app.dispose();
});

test("when a start works but reading its status fails, the new run stays live", async () => {
  const { fake, app } = await connected();
  fake.failNext("engine_status", "the status could not be read");

  await app.start();
  fake.changeFromPhone(LEAD, { volumePercent: 7 });

  const state = app.store.getState();
  assert.equal(state.engine.phase, "running");
  assert.equal(state.engine.error, null);
  assert.equal(viewMix(state, LEAD).volumePercent, 7, "the new run's events still count");
  app.dispose();
});

test("only an engine that is not running is started; any other failure is shown", async () => {
  const fake = createFakeBridge();
  fake.failNext("engine_status", "engine state lock was poisoned");
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.equal(commands(fake, "start_engine").length, 0);
  assert.deepEqual(app.store.getState().connection, {
    status: "failed",
    error: "engine state lock was poisoned",
  });

  await app.reconnect();
  assert.equal(commands(fake, "start_engine").length, 1, "now it is just not running");
  assert.equal(app.store.getState().connection.status, "ready");
  assert.equal(app.store.getState().engine.phase, "running");
  app.dispose();
});

test("a failed event registration is shown, and connecting again listens once", async () => {
  const fake = createFakeBridge({ running: true });
  let refuse = true;
  const bridge = {
    invoke: (command, args) => fake.invoke(command, args),
    listen: (event, handler) =>
      refuse && event === "meters"
        ? Promise.reject("the event channel is closed")
        : fake.listen(event, handler),
  };
  const app = createConsoleApp({ bridge });
  await app.ready;

  const { connection } = app.store.getState();
  assert.equal(connection.status, "failed");
  assert.match(connection.error, /the event channel is closed/);
  const before = app.store.getState();
  fake.changeFromPhone(LEAD, { volumePercent: 3 });
  assert.equal(app.store.getState(), before, "no half-connected listeners are left behind");

  refuse = false;
  await app.reconnect();
  let updates = 0;
  app.store.subscribe(() => {
    updates += 1;
  });
  fake.changeFromPhone(LEAD, { volumePercent: 4 });

  assert.equal(app.store.getState().connection.status, "ready");
  assert.equal(viewMix(app.store.getState(), LEAD).volumePercent, 4);
  assert.equal(updates, 1, "each event is heard once");
  app.dispose();
});

test("picking a musician opens their mix", async () => {
  const { app } = await connected();

  app.select(BASS);

  assert.equal(app.store.getState().selected, BASS);
  app.dispose();
});

test("once disposed, the console hears no more events", async () => {
  const { fake, app } = await connected();
  app.dispose();
  const state = app.store.getState();

  fake.changeFromPhone(LEAD, { volumePercent: 5 });

  assert.equal(app.store.getState(), state);
});
