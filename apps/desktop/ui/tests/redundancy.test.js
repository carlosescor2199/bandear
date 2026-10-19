// Packet redundancy set from the desk: the Redundancy cell's view, the Copy choices with their
// latency cost and the outcome in words (redundancy.js), the table rows that carry it
// (network-view.js), the bridge's `setMusicianRedundancy`, the fake bridge's
// `set_musician_redundancy` (which keeps it per musician as the desktop does, refuses a delayed copy
// for a phone that cannot hold packets back for it, and makes copies save most losses) and the
// console's command.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { createFakeBridge, syntheticLinkHistory, sampleStatus } from "../app/lib/fake-bridge.js";
import { musicianRows } from "../app/lib/network-view.js";
import {
  DEFAULT_BLOCK_MS,
  DEFAULT_DELAY_BLOCKS,
  NEEDS_NEWER_APP,
  NOT_REPORTED,
  blockMsOf,
  delayLabel,
  formatRedundancySet,
  pendingRedundancyChoice,
  redundancyView,
} from "../app/lib/redundancy.js";

const NOW = Date.UTC(2026, 9, 19, 19, 4, 5, 250);
const LEAD = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const BASS = "client:a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64";
const KEYS = "client:c81e4f2a-9b3d-4a7e-8c6f-0d2b5e9a1f37";

function row(changes = {}) {
  return {
    address: "192.168.1.12:50000",
    clientId: "a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64",
    name: "Bass",
    redundancy: { on: false },
    duplicatesSent: 0,
    recovered: 0,
    redundancySupported: true,
    redundancyHoldSupported: true,
    ...changes,
  };
}

test("one engine block lasts its buffer at the device's rate, else the default block", () => {
  assert.ok(Math.abs(DEFAULT_BLOCK_MS - 5.333) < 0.001);
  assert.equal(blockMsOf({ bufferSize: "Fixed(256)", sampleRate: 48000 }), DEFAULT_BLOCK_MS);
  assert.equal(blockMsOf({ bufferSize: "Fixed(512)", sampleRate: 96000 }), DEFAULT_BLOCK_MS, "decimated to 48 kHz");
  assert.ok(Math.abs(blockMsOf({ bufferSize: "Fixed(128)", sampleRate: 48000 }) - 2.667) < 0.001);
  assert.equal(blockMsOf({ bufferSize: "device default", sampleRate: 48000 }), DEFAULT_BLOCK_MS);
  assert.equal(blockMsOf(undefined), DEFAULT_BLOCK_MS);
  assert.equal(delayLabel(0), "immediate");
  assert.equal(delayLabel(1), "1 block later (+5.3 ms)");
  assert.equal(delayLabel(2, 2.5), "2 blocks later (+5.0 ms)");
});

test("the Redundancy cell shows whether it is on, its copy, what it costs and what it recovered", () => {
  const delayed = redundancyView(
    row({ redundancy: { on: true, delayBlocks: 2 }, duplicatesSent: 1200, recovered: 7 }),
  );
  assert.equal(delayed.key, BASS);
  assert.equal(delayed.on, true);
  assert.equal(delayed.delayBlocks, 2);
  assert.equal(delayed.holdSupported, true);
  assert.equal(delayed.holdNote, null);
  assert.equal(delayed.bandwidth, "2× bandwidth, +10.7 ms latency on this phone");
  assert.equal(delayed.recovered, "7 packets recovered");
  assert.equal(delayed.duplicates, "1,200 copies sent");
  assert.deepEqual(delayed.delays, [
    { value: 0, label: "Copy: immediate", disabled: false },
    { value: 1, label: "Copy: 1 block later (+5.3 ms)", disabled: false },
    { value: 2, label: "Copy: 2 blocks later (+10.7 ms)", disabled: false },
    { value: 3, label: "Copy: 3 blocks later (+16.0 ms)", disabled: false },
    { value: 4, label: "Copy: 4 blocks later (+21.3 ms)", disabled: false },
  ]);

  const off = redundancyView(row());
  assert.equal(off.on, false);
  assert.equal(off.delayBlocks, null);
  assert.equal(off.bandwidth, "Sends once; on doubles this musician's bandwidth");
  assert.equal(off.recovered, null);
  assert.equal(off.duplicates, null);

  const immediate = redundancyView(row({ redundancy: { on: true, delayBlocks: 0 }, recovered: 0 }));
  assert.equal(immediate.bandwidth, "2× bandwidth, no added latency");
  assert.equal(immediate.recovered, "Recovered packets are counted for delayed copies only");
  assert.equal(
    redundancyView(row({ redundancy: { on: true } })).delayBlocks,
    DEFAULT_DELAY_BLOCKS,
    "a delay not said is the default, an immediate copy",
  );
  assert.equal(DEFAULT_DELAY_BLOCKS, 0);
  const one = redundancyView(row({ redundancy: { on: true, delayBlocks: 1 }, recovered: 1 }), 2.5);
  assert.equal(one.bandwidth, "2× bandwidth, +2.5 ms latency on this phone", "the block the engine sends");
  assert.equal(one.recovered, "1 packet recovered");

  // An older phone takes immediate copies only and reports nothing it recovered.
  const older = redundancyView(
    row({
      clientId: null,
      redundancySupported: false,
      redundancyHoldSupported: false,
      redundancy: { on: true, delayBlocks: 0 },
    }),
  );
  assert.equal(older.key, "address:192.168.1.12:50000");
  assert.equal(older.recovered, `Recovered packets ${NOT_REPORTED}`);
  assert.equal(older.holdNote, `Delayed copies: ${NEEDS_NEWER_APP}`);
  assert.deepEqual(older.delays[0], { value: 0, label: "Copy: immediate", disabled: false });
  assert.deepEqual(older.delays[2], {
    value: 2,
    label: `Copy: 2 blocks later (+10.7 ms), ${NEEDS_NEWER_APP}`,
    disabled: true,
  });
  assert.ok(older.delays.slice(1).every((delay) => delay.disabled));
  // A row from before redundancy reads as off.
  assert.equal(redundancyView({ address: "1.2.3.4:5", clientId: null }).on, false);
});

test("the outcome of a change is said with what it does to the bandwidth and the latency", () => {
  const on = {
    outcome: "set",
    redundancy: { on: true, delayBlocks: 0 },
    bandwidthFactor: 2,
    saved: true,
    saveError: null,
    message: "Each packet is sent twice ...",
  };
  assert.deepEqual(formatRedundancySet(on, "Bass"), {
    ok: true,
    message:
      "Bass's audio is now sent twice, each copy right after its original: their bandwidth doubles (×2), with no added latency.",
  });
  assert.deepEqual(formatRedundancySet({ ...on, redundancy: { on: true, delayBlocks: 2 } }, "Bass"), {
    ok: true,
    message:
      "Bass's audio is now sent twice, each copy 2 blocks later: their bandwidth doubles (×2) and their phone plays 10.7 ms later.",
  });
  assert.deepEqual(
    formatRedundancySet({ ...on, redundancy: { on: false }, bandwidthFactor: 1 }, "Bass"),
    { ok: true, message: "Bass's audio is sent once again: their bandwidth is back to normal (×1)." },
  );
  assert.deepEqual(
    formatRedundancySet(
      { ...on, redundancy: { on: true, delayBlocks: 1 }, saved: false, saveError: "disk full" },
      "Bass",
      2.5,
    ),
    {
      ok: true,
      message:
        "Bass's audio is now sent twice, each copy 1 block later: their bandwidth doubles (×2) and their phone plays 2.5 ms later. It was not saved for the next start: disk full.",
    },
  );
  assert.deepEqual(
    formatRedundancySet({ outcome: "unsupported", redundancy: null, bandwidthFactor: 1, message: "Not set: ..." }, "Keys"),
    {
      ok: false,
      message: "Keys's phone needs a newer app for a delayed copy; an immediate copy works with any app.",
    },
  );
  assert.deepEqual(
    formatRedundancySet({ outcome: "invalid", redundancy: null, message: "Not set: a copy must be from 0 to 4 blocks late, got 9." }, "Bass"),
    { ok: false, message: "Not set: a copy must be from 0 to 4 blocks late, got 9." },
  );
});

test("a pending Redundancy choice settles once the row reports it, its delay too, or a new answer came back", () => {
  const off = redundancyView(row());
  const immediate = redundancyView(row({ redundancy: { on: true, delayBlocks: 0 } }));
  const two = redundancyView(row({ redundancy: { on: true, delayBlocks: 2 } }));
  assert.equal(pendingRedundancyChoice({ on: true, delayBlocks: null }, immediate, null, null), null);
  assert.deepEqual(pendingRedundancyChoice({ on: true, delayBlocks: null }, off, null, null), {
    on: true,
    delayBlocks: null,
  });
  assert.deepEqual(
    pendingRedundancyChoice({ on: true, delayBlocks: 2 }, immediate, null, null),
    { on: true, delayBlocks: 2 },
    "on, but not with the delay asked yet",
  );
  assert.equal(pendingRedundancyChoice({ on: true, delayBlocks: 2 }, two, null, null), null);
  assert.equal(pendingRedundancyChoice({ on: false, delayBlocks: null }, off, null, null), null);
  const said = { address: "192.168.1.12:50000", ok: false, message: "refused" };
  assert.equal(pendingRedundancyChoice({ on: true }, off, said, null), null, "a new answer");
  assert.deepEqual(pendingRedundancyChoice({ on: true }, off, said, said), { on: true }, "an old one");
  assert.equal(pendingRedundancyChoice(null, off, null, null), null);
});

test("every table row carries its Redundancy cell, priced in the engine's block", () => {
  const rows = musicianRows({
    engine: { status: { captureFormat: { channels: 2, sampleRate: 48000, sampleFormat: "I24", bufferSize: "Fixed(128)" } } },
    musicians: [
      {
        ...row({ redundancy: { on: true, delayBlocks: 3 }, recovered: 2 }),
        mix: { volumePercent: 80, muted: false },
        controlConnected: true,
        linkHealth: "good",
      },
    ],
  });
  assert.equal(rows[0].redundancy.on, true);
  assert.equal(rows[0].redundancy.delayBlocks, 3);
  assert.equal(rows[0].redundancy.key, BASS);
  assert.equal(rows[0].redundancy.delays[2].label, "Copy: 2 blocks later (+5.3 ms)");
  assert.equal(rows[0].redundancy.bandwidth, "2× bandwidth, +8.0 ms latency on this phone");
});

test("setMusicianRedundancy asks the desktop for the musician's redundancy", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async () => () => {},
  });

  await api.setMusicianRedundancy(BASS, true, 3);
  await api.setMusicianRedundancy(BASS, false);

  assert.deepEqual(calls, [
    ["set_musician_redundancy", { musicianKey: BASS, on: true, delayBlocks: 3 }],
    ["set_musician_redundancy", { musicianKey: BASS, on: false, delayBlocks: null }],
  ]);
});

test("the fake keeps a musician's redundancy across starts, and only a delayed copy is recovered", async () => {
  let now = NOW;
  const fake = createFakeBridge({ running: true, clock: () => now });
  const api = createEngineApi(fake);
  const bass = async () => (await api.status()).musicians[1];
  assert.deepEqual((await bass()).redundancy, { on: false });
  assert.equal((await bass()).redundancySupported, true, "Bass's phone reports recoveries");
  assert.equal((await bass()).redundancyHoldSupported, true, "and holds packets back for copies");
  assert.equal((await api.status()).musicians[0].redundancyHoldSupported, false, "Lead vocal's does not");
  assert.equal((await api.status()).musicians[2].redundancySupported, false, "Keys' does not");
  assert.equal((await bass()).recovered, 0, "nothing to recover with redundancy off");

  const set = await api.setMusicianRedundancy(BASS, true);
  assert.equal(set.outcome, "set");
  assert.deepEqual(set.redundancy, { on: true, delayBlocks: 0 }, "an immediate copy by default");
  assert.equal(set.bandwidthFactor, 2);
  assert.equal(set.saved, true);
  assert.equal(set.defaultDelayBlocks, 0);
  assert.equal(set.minDelayBlocks, 0);
  now += 60_000;
  const immediate = await bass();
  assert.ok(immediate.duplicatesSent > 0, "copies are sent");
  assert.equal(immediate.recovered, 0, "an immediate copy cannot be told from its original");

  const delayed = await api.setMusicianRedundancy(BASS, true, 2);
  assert.deepEqual(delayed.redundancy, { on: true, delayBlocks: 2 });
  const since = now;
  now += 120_000;
  const on = await bass();
  assert.deepEqual(on.redundancy, { on: true, delayBlocks: 2 });
  assert.ok(on.recovered > 0, "a lossy link recovers packets");
  const history = await api.linkHistory(BASS);
  const after = history.samples.filter((sample) => sample.atMs > since + 1000);
  assert.ok(after.some((sample) => sample.recovered > 0));
  assert.ok(
    history.samples.filter((sample) => sample.atMs < since - 1000).every((sample) => !sample.recovered),
    "nothing recovered before the copies came late",
  );

  await api.stop();
  await api.start();
  assert.deepEqual((await bass()).redundancy, { on: true, delayBlocks: 2 }, "kept for the next start");

  const off = await api.setMusicianRedundancy(BASS, false);
  assert.equal(off.bandwidthFactor, 1);
  assert.deepEqual((await bass()).redundancy, { on: false });
  assert.equal((await bass()).duplicatesSent, 0);

  // Every phone takes immediate copies; a delayed one needs a phone that holds packets back.
  const keysLate = await api.setMusicianRedundancy(KEYS, true, 1);
  assert.equal(keysLate.outcome, "unsupported");
  assert.equal(keysLate.redundancy, null);
  assert.ok(keysLate.message.includes(NEEDS_NEWER_APP), keysLate.message);
  assert.deepEqual((await api.status()).musicians[2].redundancy, { on: false }, "nothing changed");
  const keys = await api.setMusicianRedundancy(KEYS, true, 0);
  assert.equal(keys.outcome, "set");
  assert.deepEqual((await api.status()).musicians[2].redundancy, { on: true, delayBlocks: 0 });

  const invalid = await api.setMusicianRedundancy(BASS, true, 5);
  assert.equal(invalid.outcome, "invalid");
  assert.equal(invalid.redundancy, null);
  assert.ok(invalid.message.includes("from 0 to 4 blocks"), invalid.message);
  await assert.rejects(api.setMusicianRedundancy("Bass", true), (error) =>
    String(error).includes("names no phone or address"),
  );
  await assert.rejects(api.setMusicianRedundancy("client:nobody", true), (error) =>
    String(error).includes("is not here"),
  );
  const stopped = createEngineApi(createFakeBridge({ clock: () => NOW }));
  await assert.rejects(stopped.setMusicianRedundancy(BASS, true), (error) => error === "the engine is not running");
});

test("a named session keeps the redundancy, and loading one replaces it", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  await api.setMusicianRedundancy(BASS, true, 3);
  const saved = await api.saveSession("with copies");
  await api.setMusicianRedundancy(BASS, false);
  await api.setMusicianRedundancy(LEAD, true);

  await api.loadSession(saved.session.id);

  const status = await api.status();
  assert.deepEqual(status.musicians[1].redundancy, { on: true, delayBlocks: 3 });
  assert.deepEqual(status.musicians[0].redundancy, { on: false }, "not kept by the loaded session");
});

test("in the generated history copies save most losses, and only delayed ones count as recovered", () => {
  const bass = sampleStatus().musicians[1];
  bass.capabilities = ["stats.v2", "redundancy.recovered", "redundancy.hold"];
  const second = Math.floor(NOW / 1000);
  const before = syntheticLinkHistory(bass, NOW, 600);
  const delayed = syntheticLinkHistory(bass, NOW, 600, null, second - 600, 2);
  for (const [index, sample] of delayed.entries()) {
    const was = before[index];
    assert.equal(was.recovered, 0);
    assert.equal(sample.packetsLost + sample.recovered, was.packetsLost);
    assert.equal(sample.packetsReceived + sample.packetsLost, was.packetsReceived + was.packetsLost);
    assert.ok(Math.abs(sample.cushionMs - was.cushionMs - 2 * (256 / 48)) < 0.01, "the hold is in the cushion");
  }
  assert.ok(delayed.some((sample) => sample.recovered > 0));
  // Immediate copies save as many for any phone, none of them counted, and add no latency.
  const immediate = syntheticLinkHistory(bass, NOW, 600, null, second - 600, 0);
  for (const [index, sample] of immediate.entries()) {
    assert.equal(sample.packetsLost, delayed[index].packetsLost);
    assert.equal(sample.recovered, 0);
    assert.equal(sample.cushionMs, before[index].cushionMs);
  }
  const keys = { ...bass, capabilities: [] };
  assert.deepEqual(
    syntheticLinkHistory(keys, NOW, 600, null, second - 600, 0).map((sample) => sample.packetsLost),
    immediate.map((sample) => sample.packetsLost),
  );
  // A phone that does not hold packets back drops delayed copies: its losses stay.
  assert.deepEqual(syntheticLinkHistory(keys, NOW, 600, null, second - 600, 2), syntheticLinkHistory(keys, NOW, 600));
});

test("the console sets redundancy, reads the status after it and says the outcome", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.deepEqual(await app.setMusicianRedundancy(BASS, true), {
    ok: true,
    message:
      "Bass's audio is now sent twice, each copy right after its original: their bandwidth doubles (×2), with no added latency.",
  });
  assert.deepEqual(app.store.getState().musicians[1].redundancy, { on: true, delayBlocks: 0 });
  assert.deepEqual(await app.setMusicianRedundancy(BASS, true, 2), {
    ok: true,
    message:
      "Bass's audio is now sent twice, each copy 2 blocks later: their bandwidth doubles (×2) and their phone plays 10.7 ms later.",
  });
  const keys = await app.setMusicianRedundancy(KEYS, true, 2);
  assert.equal(keys.ok, false);
  assert.ok(keys.message.includes(NEEDS_NEWER_APP), keys.message);
  const refused = await app.setMusicianRedundancy("client:nobody", true);
  assert.equal(refused.ok, false);
  assert.ok(refused.message.startsWith("Redundancy was not set: "), refused.message);
  app.dispose?.();
});
