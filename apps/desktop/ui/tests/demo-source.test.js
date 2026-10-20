// The demo source: its driver and device in the audio setup (never chosen on the window's own),
// the top bar's badge and the window's title while it plays (demo-source.js, network-view.js,
// record-controller.js), the start request and the kept choice (engine-config.js), and the fake
// bridge, which mirrors the desktop: its one device, an engine with no interface whose meters
// move, a switch to anything else and a soundcheck refused, the choice kept for the next start.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import {
  DEMO_CHANNELS,
  DEMO_DEVICE,
  DEMO_HOST,
  DEMO_VOICES,
  demoChip,
  demoDevices,
  isDemoRunning,
} from "../app/lib/demo-source.js";
import {
  DEFAULT_AUDIO,
  HOSTS,
  normalizeHost,
  restoredAudio,
  startRequest,
} from "../app/lib/engine-config.js";
import { createFakeBridge, sampleStatus, syntheticMeters } from "../app/lib/fake-bridge.js";
import { HOST_OPTIONS, audioSetupView, deviceOptions, windowTitle } from "../app/lib/network-view.js";
import { mountRecChip } from "../app/lib/record-controller.js";
import { createStore } from "../app/lib/store.js";

function withStatus(status) {
  return consoleReducer(initialState, actions.status(status));
}

const demoStatus = () => ({
  ...sampleStatus(),
  deviceName: DEMO_DEVICE,
  captureFormat: { channels: 8, sampleRate: 48000, sampleFormat: "F32", bufferSize: "Fixed(256)" },
  sourceChannels: 8,
  demo: true,
});

test("the demo is a driver of its own with one device, offered last and never the default", () => {
  assert.equal(DEMO_HOST, "demo");
  assert.ok(HOSTS.includes("demo"));
  assert.equal(normalizeHost(" Demo "), "demo");
  assert.equal(DEFAULT_AUDIO.host, "auto", "the window starts on the system's own driver");
  assert.equal(normalizeHost(undefined), "auto");
  assert.equal(HOST_OPTIONS[0].value, "auto");
  assert.deepEqual(HOST_OPTIONS.at(-1), { value: "demo", label: "Demo (no interface)" });

  assert.deepEqual(demoDevices(), [{ name: "Demo (no interface)", channels: 8 }]);
  assert.deepEqual(deviceOptions(demoDevices()), [
    { value: "Demo (no interface)", label: "Demo (no interface) — 8 channels" },
  ]);
  assert.equal(DEMO_VOICES.length, DEMO_CHANNELS);
});

test("a start on the demo asks for its driver, and a kept demo is restored as chosen", () => {
  const request = startRequest({ ...DEFAULT_AUDIO, host: "demo", device: DEMO_DEVICE });
  assert.equal(request.host, "demo");
  assert.equal(request.deviceFilter, DEMO_DEVICE);
  assert.equal(startRequest(DEFAULT_AUDIO).host, "auto");

  assert.deepEqual(restoredAudio({ host: "demo", device: DEMO_DEVICE, bufferFrames: 256 }), {
    host: "demo",
    device: DEMO_DEVICE,
    bufferFrames: 256,
    asioDriver: null,
  });
  assert.equal(restoredAudio(null).host, "auto", "nothing kept is never the demo");
});

test("the badge and the title say Demo source only while the demo plays", () => {
  assert.equal(demoChip(initialState), null, "no engine");
  const live = withStatus(sampleStatus());
  assert.equal(isDemoRunning(live), false);
  assert.equal(demoChip(live), null, "an interface");
  assert.equal(windowTitle(live), "MixLink Server — Demo interface");

  const demo = withStatus(demoStatus());
  assert.equal(isDemoRunning(demo), true);
  assert.deepEqual(demoChip(demo), {
    kind: "demo",
    word: "Demo source",
    time: "",
    detail: "— no interface in use",
    label:
      "Demo source — no interface in use: the engine plays a generated band. Open Network and " +
      "system to choose an interface",
    href: "#/network",
  });
  assert.equal(windowTitle(demo), "MixLink Server — Demo source");

  const stopped = withStatus({ ...demoStatus(), stopped: true });
  assert.equal(demoChip(stopped), null, "a stopped engine plays nothing");
});

test("the top bar shows the demo badge beside the REC chip, and the strip only while one shows", () => {
  const app = { store: createStore(consoleReducer, initialState) };
  const rec = { data: undefined };
  const check = { data: undefined };
  const demo = { data: undefined };
  const chips = { '[data-chip="rec"]': rec, '[data-chip="soundcheck"]': check, '[data-chip="demo"]': demo };
  const strip = { hidden: false, querySelector: (selector) => chips[selector] ?? null };

  const unsubscribe = mountRecChip(strip, app);
  assert.equal(strip.hidden, true, "nothing to say");
  assert.equal(demo.data, null);

  app.store.dispatch(actions.status(demoStatus()));
  assert.equal(strip.hidden, false);
  assert.equal(demo.data.kind, "demo");
  assert.equal(rec.data, null, "no take records");

  app.store.dispatch(actions.status(sampleStatus()));
  assert.equal(strip.hidden, true, "an interface again");
  unsubscribe();
});

test("the audio setup lists the demo's one device under its driver", () => {
  const state = consoleReducer(
    consoleReducer(withStatus(demoStatus()), actions.audio({ host: "demo", device: DEMO_DEVICE })),
    actions.devices("demo", demoDevices()),
  );
  const view = audioSetupView(state, { draftHost: null, draftDevice: null, pendingSwitch: null });
  assert.equal(view.host, "demo");
  assert.equal(view.device, DEMO_DEVICE);
  assert.deepEqual(view.devices, [
    { value: "Demo (no interface)", label: "Demo (no interface) — 8 channels" },
  ]);
  assert.equal(view.readouts.format.startsWith("8 ch"), true, view.readouts.format);
});

test("the fake runs the demo with no interface, as the desktop does", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);
  assert.deepEqual(await api.listInputDevices("demo"), demoDevices());
  assert.equal((await api.status()).demo, false, "the sample engine runs on an interface");

  const started = await api.start(startRequest({ ...DEFAULT_AUDIO, host: "demo" }));
  assert.equal(started.deviceName, DEMO_DEVICE);
  const status = await api.status();
  assert.equal(status.demo, true);
  assert.equal(status.sourceChannels, 8);
  assert.deepEqual(status.captureFormat, {
    channels: 8,
    sampleRate: 48000,
    sampleFormat: "F32",
    bufferSize: "Fixed(256)",
  });
  const meters = syntheticMeters(status, 1_500);
  assert.equal(meters.channels.length, 8);
  assert.ok(meters.channels.some((level) => level > 0), "the meters move");

  await assert.rejects(api.switchDevice("Demo interface"), /not the demo source's device/);
  await assert.doesNotReject(api.switchDevice(DEMO_DEVICE));
  const kept = await api.setAudioSettings({ host: "demo", device: DEMO_DEVICE, bufferFrames: 256 });
  assert.equal(kept.host, "demo");

  // Back on an interface: no demo.
  await api.start(startRequest(DEFAULT_AUDIO));
  const back = await api.status();
  assert.equal(back.demo, false);
  assert.equal(back.deviceName, "Demo interface");
  assert.deepEqual(back.captureFormat, {
    channels: 6,
    sampleRate: 48000,
    sampleFormat: "I24",
    bufferSize: "Fixed(256)",
  });
});

test("back on an interface after the demo, the fake reports none of the demo's format", async () => {
  const fake = createFakeBridge({ running: true });
  const api = createEngineApi(fake);
  const interfaceFormat = (await api.status()).captureFormat;
  await api.start(startRequest({ ...DEFAULT_AUDIO, host: "demo", bufferFrames: 128 }));
  assert.equal((await api.status()).captureFormat.bufferSize, "Fixed(128)");

  // An input whose channel count is unknown keeps the interface's count, not the demo's 8.
  await api.start(startRequest(DEFAULT_AUDIO, { deviceFilter: "Digital input" }));
  const back = await api.status();
  assert.equal(back.demo, false);
  assert.equal(back.deviceName, "Digital input");
  assert.deepEqual(back.captureFormat, interfaceFormat);
  assert.equal(back.sourceChannels, interfaceFormat.channels);

  // And a known one takes its own count on the interface's format.
  await api.start(startRequest({ ...DEFAULT_AUDIO, host: "demo", bufferFrames: 128 }));
  await api.start(startRequest(DEFAULT_AUDIO, { deviceFilter: "Microphone" }));
  assert.deepEqual((await api.status()).captureFormat, { ...interfaceFormat, channels: 2 });
});

test("the console starts the demo by its driver and keeps the choice", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  const result = await app.applyAudio({ host: "demo" });
  assert.equal(result.ok, true);
  const state = app.store.getState();
  assert.equal(state.audio.host, "demo");
  assert.equal(isDemoRunning(state), true);
  assert.equal(windowTitle(state), "MixLink Server — Demo source");
  app.dispose();
});
