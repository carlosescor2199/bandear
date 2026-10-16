import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_AUDIO,
  audioChoice,
  fallbackAudio,
  fallbackNotice,
  normalizeBuffer,
  normalizeHost,
  rememberedDevice,
  restoredAudio,
  startRequest,
} from "../app/lib/engine-config.js";

const VOLT = "INPUT 1/2 (Volt 4)";

test("the default start is the old window's: the system's own driver, 256 frames, nobody added", () => {
  assert.deepEqual(DEFAULT_AUDIO, { host: "auto", device: null, bufferFrames: 256, asioDriver: null });
  assert.deepEqual(startRequest(DEFAULT_AUDIO), {
    deviceFilter: null,
    targets: [],
    controlPort: 50001,
    groups: [],
    host: "auto",
    bufferFrames: 256,
  });
  assert.deepEqual(startRequest(), startRequest(DEFAULT_AUDIO));
});

test("ASIO starts only on the driver the engineer picked, and is never built without one", () => {
  const picked = { host: "asio", bufferFrames: 256, asioDriver: "Volt ASIO" };
  assert.equal(startRequest(picked).host, "asio");
  assert.equal(startRequest(picked).deviceFilter, "Volt ASIO", "a plain start names the driver");
  assert.equal(startRequest(picked, { deviceFilter: "Other ASIO" }).deviceFilter, "Other ASIO");
  assert.equal(startRequest({ host: "asio", bufferFrames: 256 }), null);
  assert.equal(startRequest({ host: "asio", bufferFrames: 256, asioDriver: null }), null);
  assert.equal(
    startRequest({ host: "asio", device: VOLT, bufferFrames: 256, asioDriver: null }),
    null,
    "an input picked for another driver is no ASIO driver",
  );
  assert.equal(normalizeHost("WASAPI"), "wasapi");
  for (const host of [undefined, null, "", "asio driver", "kernel", 3]) {
    assert.equal(normalizeHost(host), "auto", String(host));
  }
});

test("a buffer is a whole number of frames, or the device's own when there is none", () => {
  assert.equal(normalizeBuffer(512), 512);
  assert.equal(normalizeBuffer("128"), 128);
  for (const frames of ["", null, undefined, 0, -5, 12.5, "many"]) {
    assert.equal(normalizeBuffer(frames), null, String(frames));
  }
});

test("a start can ask for an input device by name", () => {
  assert.equal(startRequest(DEFAULT_AUDIO, { deviceFilter: "Volt 4" }).deviceFilter, "Volt 4");
});

test("a plain start reopens the input picked for the driver, and a named one wins", () => {
  const volt = { host: "wasapi", device: VOLT, bufferFrames: 256, asioDriver: "Volt ASIO" };

  assert.equal(startRequest(volt).deviceFilter, VOLT, "not the system's default");
  assert.equal(startRequest(volt).host, "wasapi");
  assert.equal(startRequest({ ...volt, host: "auto" }).deviceFilter, VOLT);
  assert.equal(startRequest(volt, { deviceFilter: "Mic" }).deviceFilter, "Mic");
  assert.equal(rememberedDevice(volt), VOLT);
  assert.equal(rememberedDevice({ ...volt, host: "asio" }), "Volt ASIO", "under ASIO, its driver");
  assert.equal(rememberedDevice(DEFAULT_AUDIO), null, "the system's default");
});

test("changing one audio choice keeps the other", () => {
  assert.deepEqual(audioChoice({ host: "wasapi", bufferFrames: 128 }, { bufferFrames: 512 }), {
    host: "wasapi",
    device: null,
    bufferFrames: 512,
    asioDriver: null,
  });
  assert.deepEqual(audioChoice(DEFAULT_AUDIO, { host: "kernel", bufferFrames: 0 }), {
    host: "auto",
    device: null,
    bufferFrames: null,
    asioDriver: null,
  });
  const asio = { host: "asio", bufferFrames: 256, asioDriver: "Volt ASIO" };
  assert.equal(audioChoice(asio, { bufferFrames: 512 }).asioDriver, "Volt ASIO");
});

test("an input belongs to its driver: a new driver starts on its default unless one is named", () => {
  const volt = { host: "wasapi", device: VOLT, bufferFrames: 256, asioDriver: null };

  assert.equal(audioChoice(volt, { bufferFrames: 512 }).device, VOLT);
  assert.equal(audioChoice(volt, { device: "Mic" }).device, "Mic");
  assert.equal(audioChoice(volt, { host: "auto" }).device, null);
  assert.equal(audioChoice(volt, { host: "auto", device: "Mic" }).device, "Mic");
  assert.equal(audioChoice(volt, { host: "WASAPI" }).device, VOLT, "the same driver");
  assert.equal(audioChoice(volt, { device: "  " }).device, null, "an empty name is none");
});

test("a kept choice is restored as the next start takes it, and ASIO without a driver is no ASIO", () => {
  assert.deepEqual(restoredAudio(null), DEFAULT_AUDIO, "nothing kept: the system's own driver");
  assert.deepEqual(restoredAudio(undefined), DEFAULT_AUDIO);
  assert.deepEqual(
    restoredAudio({ host: "wasapi", device: VOLT, bufferFrames: 512, asioDriver: null }),
    { host: "wasapi", device: VOLT, bufferFrames: 512, asioDriver: null },
  );
  assert.deepEqual(
    restoredAudio({ host: "asio", device: null, bufferFrames: 128, asioDriver: "Volt ASIO" }),
    { host: "asio", device: null, bufferFrames: 128, asioDriver: "Volt ASIO" },
    "an ASIO driver the engineer picked",
  );
  assert.deepEqual(restoredAudio({ host: "wasapi" }), {
    host: "wasapi",
    device: null,
    bufferFrames: 256,
    asioDriver: null,
  });

  for (const odd of [
    { host: "asio" },
    { host: "asio", asioDriver: null, device: VOLT },
    { host: "ASIO", asioDriver: "" },
  ]) {
    const restored = restoredAudio(odd);
    assert.equal(restored.host, "auto", JSON.stringify(odd));
    assert.notEqual(startRequest(restored), null);
    assert.equal(startRequest(restored).host, "auto", "no ASIO driver is ever loaded");
  }
  assert.deepEqual(restoredAudio({ host: "kernel", bufferFrames: "lots" }), {
    ...DEFAULT_AUDIO,
    bufferFrames: null,
  });
});

test("a missing input gives way to the system's default on its driver, never on ASIO", () => {
  const volt = { host: "wasapi", device: VOLT, bufferFrames: 256, asioDriver: null };
  assert.deepEqual(startRequest(fallbackAudio(volt)), { ...startRequest(volt), deviceFilter: null });
  assert.equal(startRequest(fallbackAudio({ ...volt, host: "auto" })).host, "auto");

  const asio = { host: "asio", device: null, bufferFrames: 128, asioDriver: "Volt ASIO" };
  const fallback = startRequest(fallbackAudio(asio));
  assert.equal(fallback.host, "auto", "the system's own driver");
  assert.equal(fallback.deviceFilter, null);
  assert.equal(fallback.bufferFrames, 128);
});

test("the fallback is said with the input that was missing", () => {
  const volt = { host: "wasapi", device: VOLT, bufferFrames: 256, asioDriver: null };
  assert.equal(
    fallbackNotice(volt),
    `The saved input ${VOLT} was not found, so the engine started on the system's default input. ` +
      "The saved input is kept and tried again at the next start.",
  );
  assert.match(
    fallbackNotice({ host: "asio", device: null, bufferFrames: 256, asioDriver: "Volt ASIO" }),
    /^The saved ASIO driver Volt ASIO was not found, so the engine started on the system's own driver/,
  );
});
