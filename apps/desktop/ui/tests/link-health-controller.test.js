// The link health panel's wiring on the Network and system screen (link-health-controller.js):
// which musician's panel is open, when it closes, and the follower that reads their history while
// it is open. No DOM: the screen hands the controller the listed musicians and draws its view.
import test from "node:test";
import assert from "node:assert/strict";

import { createLinkHealthController } from "../app/lib/link-health-controller.js";
import { followedMusician } from "../app/lib/link-health-view.js";

const BASS = { address: "192.168.1.12:50000", clientId: "a", name: "Bass", capabilities: [] };
const KEYS = { address: "192.168.1.13:50000", clientId: "b", name: "Keys", capabilities: [] };
const BY_ADDRESS = { address: "192.168.1.20:50000", clientId: null, name: "", capabilities: [] };

function answer(historyId, from, to) {
  const samples = [];
  for (let seq = from; seq <= to; seq += 1) {
    samples.push({ seq, monoMs: seq * 1000, atMs: seq * 1000, gap: false, reconnected: false, health: "good" });
  }
  return { musicianKey: "client:a", historyId, samples, maxSamples: 600, periodMs: 1000 };
}

/** Lets the reads started by the controller answer. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

/** A controller on a fake clock: `reads` lists `[key, sinceSeq]`, `scheduled` the next poll. */
function harness(reply = () => answer(1, 1, 3)) {
  const probe = { reads: [], scheduled: null, changes: 0 };
  probe.controller = createLinkHealthController({
    read: async (key, sinceSeq) => {
      probe.reads.push([key, sinceSeq]);
      return reply(key, sinceSeq);
    },
    onChange: () => {
      probe.changes += 1;
    },
    schedule: (run) => {
      probe.scheduled = run;
      return 1;
    },
    cancel: () => {
      probe.scheduled = null;
    },
  });
  return probe;
}

test("picking a musician opens their panel and follows their history", async () => {
  const probe = harness();
  const listed = [BASS, KEYS];

  assert.deepEqual(probe.controller.view(listed), { selected: null, data: null }, "closed at first");
  probe.controller.select(listed, BASS.address);
  await settle();

  const view = probe.controller.view(listed);
  assert.equal(view.selected, BASS.address);
  assert.equal(view.data.title, "Bass · last 10 minutes");
  assert.equal(view.data.window.samples.length, 3);
  assert.deepEqual(probe.reads, [["client:a", null]]);
  assert.equal(probe.changes, 1, "the screen hears of the history");
  assert.notEqual(probe.scheduled, null, "keeps polling");
});

test("picking the open musician again closes the panel and stops the follower", async () => {
  const probe = harness();
  const listed = [BASS, KEYS];
  probe.controller.select(listed, BASS.address);
  await settle();

  probe.controller.select(listed, BASS.address);

  assert.deepEqual(probe.controller.view(listed), { selected: null, data: null });
  assert.equal(probe.scheduled, null, "no more reads");
});

test("picking another musician follows them instead", async () => {
  const probe = harness();
  const listed = [BASS, KEYS];
  probe.controller.select(listed, BASS.address);
  await settle();

  probe.controller.select(listed, KEYS.address);
  await settle();

  assert.equal(probe.controller.view(listed).selected, KEYS.address);
  assert.deepEqual(probe.reads.at(-1), ["client:b", null], "read whole for the new musician");
});

test("the panel closes and the follower stops when the musician leaves the list", async () => {
  const probe = harness();
  probe.controller.select([BASS, KEYS], BASS.address);
  await settle();

  assert.deepEqual(probe.controller.view([KEYS]), { selected: null, data: null });
  assert.equal(probe.scheduled, null);
  assert.deepEqual(probe.controller.view([BASS, KEYS]), { selected: null, data: null }, "stays closed");
});

test("the panel closes and the follower stops when the engine stops (nobody is listed)", async () => {
  const probe = harness();
  probe.controller.select([BASS], BASS.address);
  await settle();

  assert.equal(probe.controller.view([]).data, null);
  assert.equal(probe.scheduled, null);
});

test("a failed read reaches the panel, and the next good read clears it", async () => {
  let fail = false;
  const probe = harness(() => {
    if (fail) {
      throw "the engine is not running";
    }
    return answer(1, 1, 3);
  });
  probe.controller.select([BASS], BASS.address);
  await settle();
  fail = true;
  await probe.scheduled();

  const failed = probe.controller.view([BASS]).data;
  assert.equal(failed.message, "The link history could not be read: the engine is not running.");
  assert.equal(failed.window.samples.length, 3, "the history held is kept");

  fail = false;
  await probe.scheduled();
  assert.equal(probe.controller.view([BASS]).data.message, null);
});

test("the range and the table are the panel's own; closing hides the table but keeps the range", async () => {
  const probe = harness();
  probe.controller.select([BASS], BASS.address);
  probe.controller.setRange(60_000);
  probe.controller.showTable(true);
  await settle();

  const view = probe.controller.view([BASS]).data;
  assert.equal(view.rangeMs, 60_000);
  assert.ok(Array.isArray(view.table));

  probe.controller.close();
  assert.equal(probe.scheduled, null);
  probe.controller.select([BASS], BASS.address);
  const reopened = probe.controller.view([BASS]).data;
  assert.equal(reopened.rangeMs, 60_000);
  assert.equal(reopened.table, null);
});

test("unmounting stops the follower, and a late answer changes nothing", async () => {
  let release;
  const probe = harness(
    () =>
      new Promise((resolve) => {
        release = () => resolve(answer(1, 1, 3));
      }),
  );
  probe.controller.select([BASS], BASS.address);

  probe.controller.dispose();
  release();
  await settle();

  assert.equal(probe.scheduled, null, "no more reads");
  assert.equal(probe.changes, 0, "the late answer is dropped");
});

test("a phone listed by address that registers keeps its panel open, followed by its new key", async () => {
  const probe = harness();
  probe.controller.select([BY_ADDRESS], BY_ADDRESS.address);
  await settle();
  assert.deepEqual(probe.reads, [["address:192.168.1.20:50000", null]]);

  const registered = { ...BY_ADDRESS, clientId: "p", name: "Drums" };
  const view = probe.controller.view([registered]);

  assert.equal(view.selected, registered.address);
  assert.equal(view.data.title, "Drums · last 10 minutes");
  assert.equal(view.data.window.samples.length, 3, "the history held stays until the new read");
  await settle();
  assert.deepEqual(probe.reads.at(-1), ["client:p", null], "followed by the client id now");

  probe.controller.select([registered], registered.address);
  assert.equal(probe.controller.view([registered]).data, null, "picking them again still closes");
});

test("a row keyed by address is only followed to a registered row at the same address", () => {
  const key = "address:192.168.1.20:50000";
  const elsewhere = { ...BY_ADDRESS, address: "192.168.1.21:50000", clientId: "p" };
  assert.equal(followedMusician([elsewhere], key, BY_ADDRESS.address), null, "another address");
  assert.equal(followedMusician([BY_ADDRESS], key, BY_ADDRESS.address), BY_ADDRESS, "still by address");
  const registered = { ...BY_ADDRESS, clientId: "p" };
  assert.equal(followedMusician([registered], key, BY_ADDRESS.address), registered, "registered");
  assert.equal(
    followedMusician([{ ...BASS, address: BY_ADDRESS.address }], "client:z", BY_ADDRESS.address),
    null,
    "a phone with a client id is never followed to another one at its old address",
  );
  assert.equal(followedMusician([BASS], null, null), null, "nobody open");
});
