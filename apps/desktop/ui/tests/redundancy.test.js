// Packet redundancy set from the desk: the Redundancy cell's view and the outcome in words
// (redundancy.js), the table rows that carry it (network-view.js), the bridge's
// `setMusicianRedundancy`, the fake bridge's `set_musician_redundancy` (which keeps it per musician
// as the desktop does and makes a phone that reports recoveries recover its losses) and the
// console's command.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { createFakeBridge, syntheticLinkHistory, sampleStatus } from "../app/lib/fake-bridge.js";
import { musicianRows } from "../app/lib/network-view.js";
import {
  DEFAULT_DELAY_BLOCKS,
  NOT_REPORTED,
  formatRedundancySet,
  pendingRedundancyChoice,
  redundancyView,
} from "../app/lib/redundancy.js";

const NOW = Date.UTC(2026, 9, 19, 19, 4, 5, 250);
const LEAD = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const BASS = "client:a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64";

function row(changes = {}) {
  return {
    address: "192.168.1.12:50000",
    clientId: "a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64",
    name: "Bass",
    redundancy: { on: false },
    duplicatesSent: 0,
    recovered: 0,
    redundancySupported: true,
    ...changes,
  };
}

test("the Redundancy cell shows whether it is on, what it costs and what it recovered", () => {
  assert.deepEqual(
    redundancyView(row({ redundancy: { on: true, delayBlocks: 2 }, duplicatesSent: 1200, recovered: 7 })),
    {
      key: BASS,
      on: true,
      delayBlocks: 2,
      bandwidth: "2× bandwidth, each copy 2 blocks later",
      recovered: "7 packets recovered",
      duplicates: "1,200 copies sent",
    },
  );
  assert.deepEqual(redundancyView(row()), {
    key: BASS,
    on: false,
    delayBlocks: null,
    bandwidth: "Sends once; on doubles this musician's bandwidth",
    recovered: null,
    duplicates: null,
  });
  const one = redundancyView(row({ redundancy: { on: true, delayBlocks: 1 }, recovered: 1 }));
  assert.equal(one.bandwidth, "2× bandwidth, each copy 1 block later");
  assert.equal(one.recovered, "1 packet recovered");
  // An older phone drops the copies and reports nothing it recovered.
  const older = redundancyView(
    row({ clientId: null, redundancySupported: false, redundancy: { on: true, delayBlocks: 2 } }),
  );
  assert.equal(older.key, "address:192.168.1.12:50000");
  assert.equal(older.recovered, `Recovered packets ${NOT_REPORTED}`);
  // A row from before redundancy reads as off.
  assert.equal(redundancyView({ address: "1.2.3.4:5", clientId: null }).on, false);
  assert.equal(
    redundancyView(row({ redundancy: { on: true } })).delayBlocks,
    DEFAULT_DELAY_BLOCKS,
    "a delay not said is the default",
  );
});

test("the outcome of a change is said with what it does to the bandwidth", () => {
  const on = {
    outcome: "set",
    redundancy: { on: true, delayBlocks: 2 },
    bandwidthFactor: 2,
    saved: true,
    saveError: null,
    message: "Each packet is sent twice ...",
  };
  assert.deepEqual(formatRedundancySet(on, "Bass"), {
    ok: true,
    message: "Bass's audio is now sent twice, each copy 2 blocks later: their bandwidth doubles (×2).",
  });
  assert.deepEqual(
    formatRedundancySet({ ...on, redundancy: { on: false }, bandwidthFactor: 1 }, "Bass"),
    { ok: true, message: "Bass's audio is sent once again: their bandwidth is back to normal (×1)." },
  );
  assert.deepEqual(
    formatRedundancySet(
      { ...on, redundancy: { on: true, delayBlocks: 1 }, saved: false, saveError: "disk full" },
      "Bass",
    ),
    {
      ok: true,
      message:
        "Bass's audio is now sent twice, each copy 1 block later: their bandwidth doubles (×2). It was not saved for the next start: disk full.",
    },
  );
  assert.deepEqual(
    formatRedundancySet({ outcome: "invalid", redundancy: null, message: "Not set: a copy must be from 1 to 4 blocks late, got 9." }, "Bass"),
    { ok: false, message: "Not set: a copy must be from 1 to 4 blocks late, got 9." },
  );
});

test("a pending Redundancy choice settles once the row reports it or a new answer came back", () => {
  const off = redundancyView(row());
  const on = redundancyView(row({ redundancy: { on: true, delayBlocks: 2 } }));
  assert.equal(pendingRedundancyChoice({ on: true }, on, null, null), null);
  assert.deepEqual(pendingRedundancyChoice({ on: true }, off, null, null), { on: true });
  const said = { address: "192.168.1.12:50000", ok: false, message: "refused" };
  assert.equal(pendingRedundancyChoice({ on: true }, off, said, null), null, "a new answer");
  assert.deepEqual(pendingRedundancyChoice({ on: true }, off, said, said), { on: true }, "an old one");
  assert.equal(pendingRedundancyChoice(null, off, null, null), null);
});

test("every table row carries its Redundancy cell", () => {
  const rows = musicianRows({
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

test("the fake keeps a musician's redundancy across starts and a phone that reports recoveries recovers", async () => {
  let now = NOW;
  const fake = createFakeBridge({ running: true, clock: () => now });
  const api = createEngineApi(fake);
  const bass = async () => (await api.status()).musicians[1];
  assert.deepEqual((await bass()).redundancy, { on: false });
  assert.equal((await bass()).redundancySupported, true, "Bass's phone reports recoveries");
  assert.equal((await api.status()).musicians[2].redundancySupported, false, "Keys' does not");
  assert.equal((await bass()).recovered, 0, "nothing to recover with redundancy off");

  const set = await api.setMusicianRedundancy(BASS, true);
  assert.equal(set.outcome, "set");
  assert.deepEqual(set.redundancy, { on: true, delayBlocks: 2 });
  assert.equal(set.bandwidthFactor, 2);
  assert.equal(set.saved, true);
  now += 120_000;
  const on = await bass();
  assert.deepEqual(on.redundancy, { on: true, delayBlocks: 2 });
  assert.ok(on.duplicatesSent > 0, "copies are sent");
  assert.ok(on.recovered > 0, "a lossy link recovers packets");
  const history = await api.linkHistory(BASS);
  const after = history.samples.filter((sample) => sample.atMs > NOW + 1000);
  assert.ok(after.some((sample) => sample.recovered > 0));
  assert.ok(
    history.samples.filter((sample) => sample.atMs < NOW - 1000).every((sample) => sample.recovered === 0),
    "nothing recovered before it was on",
  );

  await api.stop();
  await api.start();
  assert.deepEqual((await bass()).redundancy, { on: true, delayBlocks: 2 }, "kept for the next start");

  const off = await api.setMusicianRedundancy(BASS, false);
  assert.equal(off.bandwidthFactor, 1);
  assert.deepEqual((await bass()).redundancy, { on: false });
  assert.equal((await bass()).duplicatesSent, 0);

  // Every phone takes it, an older one too.
  const keys = await api.setMusicianRedundancy("client:c81e4f2a-9b3d-4a7e-8c6f-0d2b5e9a1f37", true, 1);
  assert.equal(keys.outcome, "set");
  assert.deepEqual((await api.status()).musicians[2].redundancy, { on: true, delayBlocks: 1 });

  const invalid = await api.setMusicianRedundancy(BASS, true, 5);
  assert.equal(invalid.outcome, "invalid");
  assert.equal(invalid.redundancy, null);
  assert.ok(invalid.message.includes("from 1 to 4 blocks"), invalid.message);
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

test("a recovered packet counts as received, not lost, in the generated history", () => {
  const bass = sampleStatus().musicians[1];
  bass.capabilities = ["stats.v2", "redundancy.recovered"];
  const second = Math.floor(NOW / 1000);
  const before = syntheticLinkHistory(bass, NOW, 600);
  const after = syntheticLinkHistory(bass, NOW, 600, null, second - 600);
  for (const [index, sample] of after.entries()) {
    const was = before[index];
    assert.equal(was.recovered, 0);
    assert.equal(sample.packetsLost + sample.recovered, was.packetsLost);
    assert.equal(sample.packetsReceived + sample.packetsLost, was.packetsReceived + was.packetsLost);
  }
  assert.ok(after.some((sample) => sample.recovered > 0));
  // A phone that does not report recoveries drops the copies: its losses stay.
  const keys = { ...bass, capabilities: [] };
  assert.deepEqual(syntheticLinkHistory(keys, NOW, 600, null, second - 600), syntheticLinkHistory(keys, NOW, 600));
});

test("the console sets redundancy, reads the status after it and says the outcome", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.deepEqual(await app.setMusicianRedundancy(BASS, true), {
    ok: true,
    message: "Bass's audio is now sent twice, each copy 2 blocks later: their bandwidth doubles (×2).",
  });
  assert.deepEqual(app.store.getState().musicians[1].redundancy, { on: true, delayBlocks: 2 });
  const refused = await app.setMusicianRedundancy("client:nobody", true);
  assert.equal(refused.ok, false);
  assert.ok(refused.message.startsWith("Redundancy was not set: "), refused.message);
  app.dispose?.();
});
