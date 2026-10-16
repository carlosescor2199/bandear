import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_AUDIO,
  audioChoice,
  normalizeBuffer,
  normalizeHost,
  startRequest,
} from "../app/lib/engine-config.js";

test("the default start is the old window's: the system's own driver, 256 frames, nobody added", () => {
  assert.deepEqual(DEFAULT_AUDIO, { host: "auto", bufferFrames: 256, asioDriver: null });
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

test("changing one audio choice keeps the other", () => {
  assert.deepEqual(audioChoice({ host: "wasapi", bufferFrames: 128 }, { bufferFrames: 512 }), {
    host: "wasapi",
    bufferFrames: 512,
    asioDriver: null,
  });
  assert.deepEqual(audioChoice(DEFAULT_AUDIO, { host: "kernel", bufferFrames: 0 }), {
    host: "auto",
    bufferFrames: null,
    asioDriver: null,
  });
  const asio = { host: "asio", bufferFrames: 256, asioDriver: "Volt ASIO" };
  assert.equal(audioChoice(asio, { bufferFrames: 512 }).asioDriver, "Volt ASIO");
});
