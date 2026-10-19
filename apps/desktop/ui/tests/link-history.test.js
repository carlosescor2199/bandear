// Link histories: the bridge's `linkHistory` call as the desktop takes it, and the fake bridge's
// generated history (syntheticLinkHistory), which the Network and system screen's chart is built
// against: its shape, bounds, gaps and reconnects, the `sinceSeq` cursor, the history id and the refusals, and the
// round trip and newest second every status row carries.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import {
  LINK_HISTORY_SAMPLES,
  LINK_SAMPLE_PERIOD_MS,
  createFakeBridge,
  sampleStatus,
  syntheticLinkHistory,
} from "../app/lib/fake-bridge.js";

const NOW = Date.UTC(2026, 9, 18, 19, 4, 5, 250);
const LEAD = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const BASS = "client:a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64";
const KEYS = "address:192.168.1.17:50000";
const DRUMS = "client:e4b7c1d9-2a6f-4d3e-9b8c-5f0a7e2d6c18";

const SAMPLE_FIELDS = [
  "seq",
  "monoMs",
  "atMs",
  "gap",
  "reconnected",
  "health",
  "intervalMs",
  "packetsReceived",
  "packetsLost",
  "lossPercent",
  "underruns",
  "receiveTimeouts",
  "rttMs",
  "jitterMs",
  "cushionMs",
  "backlogMs",
  "recovered",
];

function running() {
  return createEngineApi(createFakeBridge({ running: true, clock: () => NOW }));
}

test("linkHistory asks the desktop for a musician's history, all of it or since a sample", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async () => () => {},
  });

  await api.linkHistory(LEAD);
  await api.linkHistory(LEAD, 41);

  assert.deepEqual(calls, [
    ["link_history", { musicianKey: LEAD, sinceSeq: null }],
    ["link_history", { musicianKey: LEAD, sinceSeq: 41 }],
  ]);
});

test("a musician's history is ten minutes of seconds, the oldest first, in the desktop's shape", async () => {
  const history = await running().linkHistory(LEAD);

  assert.equal(history.musicianKey, LEAD);
  assert.equal(history.maxSamples, LINK_HISTORY_SAMPLES);
  assert.equal(history.periodMs, LINK_SAMPLE_PERIOD_MS);
  assert.equal(typeof history.historyId, "number");
  assert.equal(history.samples.length, 600);
  const newest = history.samples.at(-1);
  assert.equal(newest.atMs, Math.floor(NOW / 1000) * 1000, "the newest second is now");
  for (const [index, sample] of history.samples.entries()) {
    assert.deepEqual(Object.keys(sample), SAMPLE_FIELDS);
    if (index > 0) {
      assert.equal(sample.atMs - history.samples[index - 1].atMs, 1000, "a second apart");
      assert.equal(sample.monoMs - history.samples[index - 1].monoMs, 1000, "on the monotonic clock");
      assert.equal(sample.seq, history.samples[index - 1].seq + 1, "numbered in order");
    }
  }
  assert.ok(history.samples.every((sample) => !sample.gap && sample.health !== "unknown"));
  assert.ok(history.samples.every((sample) => sample.cushionMs === 40), "stats.v2 extras");
  assert.ok(history.samples.every((sample) => sample.recovered === null), "no redundancy");
  assert.ok(history.samples.every((sample) => sample.rttMs >= 3 && sample.rttMs <= 6));
});

test("the generated history is the same for the same second and moves on with the clock", () => {
  const row = sampleStatus().musicians[1];
  row.capabilities = ["stats.v2"];

  const first = syntheticLinkHistory(row, NOW);
  const again = syntheticLinkHistory(row, NOW + 500);
  const later = syntheticLinkHistory(row, NOW + 1_000);

  assert.deepEqual(again, first, "the same second");
  assert.deepEqual(later.slice(0, -1), first.slice(1), "one second on");
  assert.deepEqual(syntheticLinkHistory(row, NOW, 5), first.slice(-5), "the newest few");
});

test("a link that needs attention reconnected once and a lost one ends in gaps", async () => {
  const api = running();
  const bass = (await api.linkHistory(BASS)).samples;
  const drums = (await api.linkHistory(DRUMS)).samples;

  assert.equal(bass.filter((sample) => sample.reconnected).length, 1);
  assert.ok(bass.some((sample) => sample.health === "warning"));
  const gaps = drums.slice(-20);
  assert.ok(gaps.every((sample) => sample.gap && sample.health === "unknown"));
  assert.ok(gaps.every((sample) => sample.lossPercent === null && sample.rttMs !== null));
  assert.equal(drums.at(-21).gap, false, "the reports stopped 20 seconds ago");
  assert.ok(
    drums.every((sample) => sample.jitterMs === null),
    "a phone that does not announce stats.v2 reports no jitter",
  );
});

test("a phone that never reported has no history, and sinceSeq answers only what is newer", async () => {
  const api = running();

  assert.deepEqual((await api.linkHistory(KEYS)).samples, []);
  const newest = Math.floor(NOW / 1000) * 1000;
  const whole = (await api.linkHistory(LEAD)).samples;
  const cursor = whole.at(-4).seq;
  const newer = (await api.linkHistory(LEAD, cursor)).samples;
  assert.deepEqual(
    newer.map((sample) => sample.atMs),
    [newest - 2_000, newest - 1_000, newest],
  );
  assert.deepEqual((await api.linkHistory(LEAD, whole.at(-1).seq)).samples, []);
});

test("each musician's history has its own id, and an engine restart starts new ones", async () => {
  const bridge = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(bridge);
  const lead = (await api.linkHistory(LEAD)).historyId;
  const bass = (await api.linkHistory(BASS)).historyId;

  assert.notEqual(lead, bass);
  assert.equal((await api.linkHistory(LEAD)).historyId, lead, "the same history");
  await api.stop();
  await api.start();
  assert.notEqual((await api.linkHistory(LEAD)).historyId, lead, "a new engine, a new history");
});

test("a key of another form, a key that names nobody and a stopped engine are refused", async () => {
  await assert.rejects(running().linkHistory("Lead vocal"), (error) =>
    String(error).includes("names no phone or address"),
  );
  await assert.rejects(running().linkHistory("client:nobody"), (error) =>
    String(error).includes("names no musician"),
  );
  const stopped = createEngineApi(createFakeBridge({ clock: () => NOW }));
  await assert.rejects(stopped.linkHistory(LEAD), (error) => error === "the engine is not running");
});

test("every status row carries its capabilities, round trip and newest second, never the history", async () => {
  const status = await running().status();
  const [lead, bass, keys, drums] = status.musicians;

  assert.deepEqual(lead.capabilities, ["stats.v2", "playback.buffer"]);
  assert.deepEqual(bass.capabilities, ["stats.v2", "redundancy.recovered", "redundancy.hold"]);
  assert.deepEqual(keys.capabilities, []);
  for (const row of status.musicians) {
    assert.equal(typeof row.rttMs, "number");
    assert.equal(typeof row.rttLastMs, "number");
    assert.equal(row.linkHistory, undefined);
  }
  assert.deepEqual(lead.linkLatest, syntheticLinkHistory(lead, NOW).at(-1));
  assert.equal(keys.linkLatest, null, "never reported");
  assert.equal(drums.linkLatest.gap, true, "its reports stopped");
});
