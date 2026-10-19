// The phone's playback buffer set from the desk: the Buffer cell's view and the engineer's request
// (playback-buffer.js), the table rows that carry it (network-view.js), the bridge's
// `setMusicianBuffer`, the fake bridge's `set_musician_buffer` (which keeps it per musician as the
// desktop does and makes the phone's reported cushion follow it) and the console's command.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { createFakeBridge } from "../app/lib/fake-bridge.js";
import { musicianRows } from "../app/lib/network-view.js";
import {
  DEFAULT_FIXED_MS,
  MAX_BUFFER_MS,
  MIN_BUFFER_MS,
  NEEDS_NEWER_APP,
  bufferRequest,
  bufferView,
  formatBufferSet,
  nameForKey,
} from "../app/lib/playback-buffer.js";

const NOW = Date.UTC(2026, 9, 19, 19, 4, 5, 250);
const LEAD = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const BASS = "client:a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64";

function row(changes = {}) {
  return {
    address: "192.168.1.3:50000",
    clientId: "5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90",
    name: "Lead vocal",
    buffer: { mode: "auto" },
    bufferSupported: true,
    linkLatest: null,
    ...changes,
  };
}

test("the Buffer cell shows the mode, a fixed length and the cushion the phone plays", () => {
  assert.deepEqual(bufferView(row({ buffer: { mode: "fixed", ms: 40 }, linkLatest: { cushionMs: 42.6667 } })), {
    key: LEAD,
    supported: true,
    mode: "fixed",
    ms: 40,
    playing: "plays 42.7 ms",
    note: null,
  });
  assert.deepEqual(bufferView(row()), {
    key: LEAD,
    supported: true,
    mode: "auto",
    ms: null,
    playing: null,
    note: null,
  });
  const older = bufferView(row({ clientId: null, bufferSupported: false, buffer: undefined }));
  assert.equal(older.key, "address:192.168.1.3:50000");
  assert.equal(older.supported, false);
  assert.equal(older.note, NEEDS_NEWER_APP);
  assert.equal(older.mode, "auto");
});

test("a fixed buffer is whole milliseconds within its range, Auto needs none", () => {
  assert.equal(MIN_BUFFER_MS, 5);
  assert.equal(MAX_BUFFER_MS, 500);
  assert.equal(DEFAULT_FIXED_MS, 40);
  assert.deepEqual(bufferRequest("auto", "garbage"), { ok: true, mode: "auto", ms: null });
  assert.deepEqual(bufferRequest("fixed", " 40 "), { ok: true, mode: "fixed", ms: 40 });
  assert.deepEqual(bufferRequest("fixed", 5), { ok: true, mode: "fixed", ms: 5 });
  assert.deepEqual(bufferRequest("fixed", "500"), { ok: true, mode: "fixed", ms: 500 });
  for (const [text, says] of [
    ["", "whole milliseconds"],
    ["12.5", "whole milliseconds"],
    ["-3", "whole milliseconds"],
    ["4", "not 4 ms"],
    ["501", "not 501 ms"],
  ]) {
    const refused = bufferRequest("fixed", text);
    assert.equal(refused.ok, false, text);
    assert.ok(refused.message.includes(says), refused.message);
  }
  assert.equal(bufferRequest("elastic", "40").ok, false);
});

test("the desk's answer is said for the musician, a failed save after it", () => {
  const fixed = { mode: "fixed", ms: 40 };
  assert.deepEqual(formatBufferSet({ outcome: "sent", buffer: fixed }, "Ana"), {
    ok: true,
    message: "Sent Ana's phone a fixed 40 ms buffer.",
  });
  assert.deepEqual(formatBufferSet({ outcome: "sent", buffer: { mode: "auto" } }, "Ana"), {
    ok: true,
    message: "Ana's phone is back on Auto.",
  });
  assert.deepEqual(formatBufferSet({ outcome: "notConnected", buffer: fixed }, "Ana"), {
    ok: true,
    message: "Kept for Ana: their phone gets a fixed 40 ms buffer when it is connected again.",
  });
  assert.deepEqual(formatBufferSet({ outcome: "unsupported", buffer: null }, "Bass"), {
    ok: false,
    message: "Bass's phone needs a newer app to take its buffer from the desk.",
  });
  assert.deepEqual(
    formatBufferSet({ outcome: "invalid", message: "Not set: too long." }, "Ana"),
    { ok: false, message: "Not set: too long." },
  );
  assert.equal(
    formatBufferSet({ outcome: "sent", buffer: fixed, saveError: "disk full" }, "Ana").message,
    "Sent Ana's phone a fixed 40 ms buffer. It was not saved for the next start: disk full.",
  );
  assert.equal(nameForKey([row()], LEAD), "Lead vocal");
  assert.equal(nameForKey([row()], BASS), BASS);
});

test("each table row carries its Buffer cell", () => {
  const rows = musicianRows({
    musicians: [
      {
        ...row({ buffer: { mode: "fixed", ms: 80 } }),
        mix: { volumePercent: 80, muted: false },
        controlConnected: true,
        linkHealth: "good",
      },
    ],
  });
  assert.equal(rows[0].buffer.mode, "fixed");
  assert.equal(rows[0].buffer.ms, 80);
  assert.equal(rows[0].buffer.key, LEAD);
});

test("setMusicianBuffer asks the desktop for the musician's buffer", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async () => () => {},
  });

  await api.setMusicianBuffer(LEAD, "fixed", 40);
  await api.setMusicianBuffer(LEAD, "auto");

  assert.deepEqual(calls, [
    ["set_musician_buffer", { musicianKey: LEAD, mode: "fixed", ms: 40 }],
    ["set_musician_buffer", { musicianKey: LEAD, mode: "auto", ms: null }],
  ]);
});

test("the fake keeps a phone's buffer, its cushion follows it, and an older phone takes none", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  const lead = async () => (await api.status()).musicians[0];
  assert.deepEqual((await lead()).buffer, { mode: "auto" });
  assert.equal((await lead()).bufferSupported, true);
  assert.equal((await lead()).linkLatest.cushionMs, 40, "the phone's own cushion");

  const set = await api.setMusicianBuffer(LEAD, "fixed", 40);
  assert.equal(set.outcome, "sent");
  assert.deepEqual(set.buffer, { mode: "fixed", ms: 40 });
  assert.equal(set.saved, true);
  assert.deepEqual((await lead()).buffer, { mode: "fixed", ms: 40 });
  // Eight 256-frame packets at 48 kHz: 40 ms rounded up to whole packets.
  assert.equal((await lead()).linkLatest.cushionMs, 42.67);

  await api.stop();
  await api.start();
  assert.deepEqual((await lead()).buffer, { mode: "fixed", ms: 40 }, "kept for the next start");

  assert.equal((await api.setMusicianBuffer(LEAD, "auto")).outcome, "sent");
  assert.deepEqual((await lead()).buffer, { mode: "auto" });
  assert.equal((await lead()).linkLatest.cushionMs, 40);

  const bass = await api.setMusicianBuffer(BASS, "fixed", 40);
  assert.equal(bass.outcome, "unsupported");
  assert.equal(bass.buffer, null);
  const status = await api.status();
  assert.equal(status.musicians[1].bufferSupported, false);
  assert.deepEqual(status.musicians[1].buffer, { mode: "auto" });

  const invalid = await api.setMusicianBuffer(LEAD, "fixed", 501);
  assert.equal(invalid.outcome, "invalid");
  assert.ok(invalid.message.includes("between 5 and 500 ms"), invalid.message);
  await assert.rejects(api.setMusicianBuffer("Lead", "auto"), (error) =>
    String(error).includes("names no phone or address"),
  );
  await assert.rejects(api.setMusicianBuffer("client:nobody", "auto"), (error) =>
    String(error).includes("is not here"),
  );
  const stopped = createEngineApi(createFakeBridge({ clock: () => NOW }));
  await assert.rejects(
    stopped.setMusicianBuffer(LEAD, "auto"),
    (error) => error === "the engine is not running",
  );
});

test("the console sets a buffer, reads the status after it and says the outcome", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.deepEqual(await app.setMusicianBuffer(LEAD, "fixed", 40), {
    ok: true,
    message: "Sent Lead vocal's phone a fixed 40 ms buffer.",
  });
  assert.deepEqual(app.store.getState().musicians[0].buffer, { mode: "fixed", ms: 40 });

  assert.deepEqual(await app.setMusicianBuffer(BASS, "fixed", 40), {
    ok: false,
    message: "Bass's phone needs a newer app to take its buffer from the desk.",
  });
  const refused = await app.setMusicianBuffer("client:nobody", "auto");
  assert.equal(refused.ok, false);
  assert.ok(refused.message.startsWith("The buffer was not set: "), refused.message);
  app.dispose?.();
});
