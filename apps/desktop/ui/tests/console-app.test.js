import test from "node:test";
import assert from "node:assert/strict";

import {
  addGroup,
  renameChannel,
  renameGroup,
  setChannelKind,
} from "../app/lib/channels-view.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { musicianByAddress, pendingEdits, viewMix } from "../app/lib/console-state.js";
import { createFakeBridge, sampleStatus } from "../app/lib/fake-bridge.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

// Resolves once `done()` holds, polling every few milliseconds for up to two seconds.
async function until(done, limitMs = 2000) {
  const deadline = Date.now() + limitMs;
  while (!done() && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
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

test("a start whose status never comes says so and offers to try again", async () => {
  const fake = createFakeBridge({ running: true, statusOnStart: false });
  const app = createConsoleApp({ bridge: fake, statusRetryMs: 1 });
  await app.ready;
  fake.failNext("engine_status", "the status could not be read", { times: 4 });

  await app.start();
  // Timers are coarse on some systems (about 15 ms on Windows): wait for the outcome, not a time.
  await until(() => app.store.getState().connection.status === "failed");

  const { connection } = app.store.getState();
  assert.equal(connection.status, "failed", "not left on starting with nothing to press");
  assert.match(connection.error, /could not be read/);

  await app.reconnect();
  assert.equal(app.store.getState().connection.status, "ready");
  assert.equal(app.store.getState().engine.phase, "running");
  assert.equal(app.store.getState().engine.run, 2, "the started run, never ended");
  app.dispose();
});

test("two quick reconnects listen to the engine only once", async () => {
  const fake = createFakeBridge({ running: true });
  let refuse = true;
  const active = new Map();
  const bridge = {
    invoke: (command, args) => fake.invoke(command, args),
    async listen(event, handler) {
      if (refuse && event === "meters") {
        throw "the event channel is closed";
      }
      const unlisten = await fake.listen(event, handler);
      active.set(event, (active.get(event) ?? 0) + 1);
      return () => {
        active.set(event, active.get(event) - 1);
        unlisten();
      };
    },
  };
  const app = createConsoleApp({ bridge });
  await app.ready;
  refuse = false;

  await Promise.all([app.reconnect(), app.reconnect()]);

  assert.deepEqual(Object.fromEntries(active), { status: 1, "mix-changed": 1, meters: 1 });
  assert.equal(app.store.getState().connection.status, "ready");
  app.dispose();
});

function startConfigs(fake) {
  return commands(fake, "start_engine").map(({ args }) => args.config);
}

test("devices are listed for the driver asked for, and a failure to list says so", async () => {
  const { fake, app } = await connected();

  await app.loadDevices("wasapi");
  assert.equal(app.store.getState().devices.host, "wasapi");
  assert.equal(app.store.getState().devices.list.length, 3);
  assert.deepEqual(commands(fake, "list_input_devices").at(-1).args, { host: "wasapi" });

  fake.failNext("list_input_devices", "no such backend");
  await app.loadDevices("asio");
  assert.deepEqual(app.store.getState().devices, {
    host: "asio",
    list: [],
    error: "no such backend",
  });
  app.dispose();
});

test("a driver change restarts on it with no device named; a buffer change keeps the device", async () => {
  const { fake, app } = await connected();

  await app.applyAudio({ host: "wasapi" });
  await app.applyAudio({ bufferFrames: 512 }, { keepDevice: true });

  const [driver, buffer] = startConfigs(fake);
  assert.equal(driver.host, "wasapi");
  assert.equal(driver.deviceFilter, null);
  assert.equal(buffer.host, "wasapi");
  assert.equal(buffer.bufferFrames, 512);
  assert.equal(buffer.deviceFilter, "Demo interface");
  app.dispose();
});

test("ASIO starts only on a driver the engineer picked", async () => {
  const { fake, app } = await connected();

  const refused = await app.applyAudio({ host: "asio" });
  assert.equal(refused.ok, false);
  assert.match(refused.error, /Network and system/);
  assert.equal(commands(fake, "start_engine").length, 0, "nothing loaded a driver");

  await app.applyAudio({ host: "asio" }, { deviceFilter: "Volt ASIO" });
  assert.deepEqual(startConfigs(fake).map(({ host, deviceFilter }) => [host, deviceFilter]), [
    ["asio", "Volt ASIO"],
  ]);
  app.dispose();
});

const ASIO = "Demo ASIO driver";

function lastStart(fake) {
  const { host, deviceFilter } = startConfigs(fake).at(-1);
  return [host, deviceFilter];
}

test("after ASIO was applied, a plain start and a start after a stop use the picked driver", async () => {
  const { fake, app } = await connected();
  await app.applyAudio({ host: "asio" }, { deviceFilter: ASIO });

  await app.stop();
  await app.start();

  assert.deepEqual(lastStart(fake), ["asio", ASIO]);
  app.dispose();
});

test("the automatic start after a reconnect uses the picked ASIO driver", async () => {
  const { fake, app } = await connected();
  await app.applyAudio({ host: "asio" }, { deviceFilter: ASIO });
  await fake.invoke("stop_engine");

  await app.reconnect();

  assert.deepEqual(lastStart(fake), ["asio", ASIO]);
  app.dispose();
});

test("ASIO without a picked driver never reaches the engine, and says where to pick one", async () => {
  const { fake, app } = await connected();
  await app.stop();
  app.setAudio({ host: "asio" });

  await app.start();

  assert.equal(commands(fake, "start_engine").length, 0);
  assert.equal(app.store.getState().engine.phase, "error");
  assert.match(app.store.getState().engine.error, /Network and system/);
  app.dispose();
});

test("a failed ASIO start keeps the settings that were applied; a successful one applies it", async () => {
  const { fake, app } = await connected();
  fake.failStart("the driver did not load");

  const failed = await app.applyAudio({ host: "asio" }, { deviceFilter: ASIO });

  assert.deepEqual(failed, { ok: false, error: "the driver did not load" });
  assert.deepEqual(app.store.getState().audio, { host: "auto", bufferFrames: 256, asioDriver: null });
  fake.failStart(null);
  assert.deepEqual(await app.applyAudio({ host: "asio" }, { deviceFilter: ASIO }), { ok: true });
  assert.deepEqual(app.store.getState().audio, { host: "asio", bufferFrames: 256, asioDriver: ASIO });
  app.dispose();
});

test("a device switch is live under WASAPI and a restart on that device otherwise", async () => {
  const { fake, app } = await connected();

  await app.switchDevice("Microphone (USB audio)");
  assert.equal(startConfigs(fake).at(-1).deviceFilter, "Microphone (USB audio)");
  assert.equal(commands(fake, "switch_device").length, 0);

  await app.applyAudio({ host: "wasapi" });
  await app.switchDevice("Demo interface");
  assert.deepEqual(commands(fake, "switch_device").at(-1).args, { device: "Demo interface" });
  app.dispose();
});

test("a refused live switch is said, and the engine keeps its device", async () => {
  const { fake, app } = await connected();
  await app.applyAudio({ host: "wasapi" });
  fake.failNext("switch_device", "the device is busy");

  await app.switchDevice("Microphone (USB audio)");

  const state = app.store.getState();
  assert.deepEqual(state.commandError, { action: "switch", message: "the device is busy" });
  assert.equal(state.engine.phase, "running");
  assert.equal(state.engine.status.deviceName, "Demo interface");
  app.dispose();
});

test("a switch that works clears the refusal of an earlier one", async () => {
  const { fake, app } = await connected();
  await app.applyAudio({ host: "wasapi" });
  fake.failNext("switch_device", "the device is busy");
  await app.switchDevice("Microphone (USB audio)");
  assert.equal(app.store.getState().commandError?.action, "switch");

  await app.switchDevice("Microphone (USB audio)");

  assert.equal(app.store.getState().commandError, null);
  assert.equal(app.store.getState().engine.status.deviceName, "Microphone (USB audio)");
  app.dispose();
});

test("adding, removing and renaming musicians answer with the result, and the list follows", async () => {
  const { app } = await connected();
  const count = sampleStatus().musicians.length;

  assert.deepEqual(await app.addTarget("192.168.1.40"), { ok: true });
  assert.equal(app.store.getState().musicians.length, count + 1);
  const twice = await app.addTarget("192.168.1.40:50001");
  assert.equal(twice.ok, false);
  assert.match(twice.message, /192\.168\.1\.40/);

  assert.deepEqual(await app.rename("192.168.1.40:50000", "Keys 2"), { ok: true });
  const added = app.store.getState().musicians.at(-1);
  assert.equal(added.name, "Keys 2");
  assert.equal(added.nameIsLocal, true);

  assert.deepEqual(await app.removeTarget("192.168.1.40:50000"), { ok: true });
  assert.equal(app.store.getState().musicians.length, count);
  app.dispose();
});

test("a musician's label stays on screen when their phone comes back from a new IP", async () => {
  const { fake, app } = await connected();
  const phone = sampleStatus().musicians.find((musician) => !musician.clientName);

  assert.deepEqual(await app.rename(phone.address, "Drum kit"), { ok: true });
  fake.movePhone(phone.address, "192.168.1.77:50000");

  const { musicians } = app.store.getState();
  assert.equal(musicians.some((musician) => musician.address === phone.address), false);
  const moved = musicians.find((musician) => musician.address === "192.168.1.77:50000");
  assert.equal(moved.clientId, phone.clientId);
  assert.equal(moved.name, "Drum kit");
  assert.equal(moved.nameIsLocal, true);
  app.dispose();
});

test("the window is titled after the input device, and plainly when no engine runs", async () => {
  const { fake, app } = await connected();
  await app.stop();

  assert.deepEqual(fake.titles, ["MixLink Server — Demo interface", "MixLink Server"]);
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

test("channel edits go out one at a time, each built on what the last one left", async () => {
  const { fake, app } = await connected();
  fake.holdAnswers("set_channels");

  const first = app.editChannels((table) => renameChannel(table, 0, "Kick in", 6));
  const second = app.editChannels((table) => setChannelKind(table, 1, "percussion", 6));
  await settle();
  assert.equal(commands(fake, "set_channels").length, 1, "the second waits for the first's answer");
  fake.releaseAnswers();
  await until(() => commands(fake, "set_channels").length === 2);
  fake.releaseAnswers();

  assert.deepEqual(await first, { ok: true, message: "Channel details applied to every phone." });
  assert.deepEqual(await second, { ok: true, message: "Channel details applied to every phone." });
  const [kick, snare] = app.store.getState().engine.status.channels;
  assert.equal(kick.name, "Kick in");
  assert.equal(snare.kind, "percussion", "the second edit kept the first one's name");
  app.dispose();
});

test("a channel edit the desk refuses says why and sends nothing", async () => {
  const { fake, app } = await connected();

  const result = await app.editChannels((table) => renameChannel(table, 0, "x".repeat(40), 6));

  assert.deepEqual(result, {
    ok: false,
    message: "The channel details were not changed: the name of CH 1 is longer than 32 characters.",
  });
  assert.equal(commands(fake, "set_channels").length, 0);
  fake.failNext("set_channels", "the engine is stopping");
  assert.deepEqual(
    await app.editChannels((table) => renameChannel(table, 0, "Kick in", 6)),
    { ok: false, message: "The channel details were not changed: the engine is stopping." },
  );
  app.dispose();
});

test("details of inputs a smaller device lacks survive an edit made while it is in use", async () => {
  const { app } = await connected();
  await app.editChannels((table) => renameChannel(table, 5, "Backing vox 2", 6));

  await app.switchDevice("Microphone (USB audio)");
  await app.editChannels((table) => renameChannel(table, 0, "Kick in", 2));
  await app.switchDevice("Demo interface");

  assert.deepEqual(
    app.store.getState().engine.status.channels.map((channel) => channel.name),
    ["Kick in", "Snare", "Overheads", "Bass", "Lead vox", "Backing vox 2"],
  );
  app.dispose();
});

test("group edits answer in words and the new groups show at once", async () => {
  const { app } = await connected();

  const result = await app.editGroups((groups) => addGroup(groups));

  assert.deepEqual(result, { ok: true, message: "Groups applied to every phone." });
  assert.deepEqual(
    app.store.getState().engine.status.groups.map((group) => group.name),
    ["Drums", "Vocals", "Group 3"],
  );
  assert.deepEqual(await app.editGroups((groups) => renameGroup(groups, "vocals", " ")), {
    ok: false,
    message: "The groups were not changed: a group needs a name.",
  });
  app.dispose();
});

test("with no engine running, an edit is kept for the next start once the desk knows the list", async () => {
  const fake = createFakeBridge();
  const app = createConsoleApp({ bridge: fake, autoStart: false });
  await app.ready;

  assert.deepEqual(await app.editGroups((groups) => addGroup(groups)), {
    ok: false,
    message:
      "The groups were not changed: no engine is running, so this desk does not know them yet; start the engine first.",
  });
  assert.equal(commands(fake, "set_groups").length, 0, "the kept groups are not replaced blindly");

  await app.start();
  await app.editGroups((groups) => renameGroup(groups, "vocals", "Voices"));
  await app.stop();
  assert.deepEqual(await app.editGroups((groups) => addGroup(groups)), {
    ok: true,
    message: "Groups saved; they apply at the next start.",
  });
  assert.deepEqual(
    commands(fake, "set_groups").at(-1).args.groups.map((group) => group.name),
    ["Drums", "Voices", "Group 3"],
  );
  app.dispose();
});
