import test from "node:test";
import assert from "node:assert/strict";

import {
  MAX_CHANNELS,
  MAX_GROUPS,
  applyEdits,
  buildMixRequest,
  defaultMix,
  editKey,
  mergeMixUpdate,
  mixFromDto,
  parseEditKey,
  settleEdits,
} from "../app/lib/mix.js";

const MIX_FIELDS = [
  "channelGains",
  "channelMuted",
  "channelSolo",
  "groupLevels",
  "maxLevelPercent",
  "muted",
  "pans",
  "revision",
  "volumePercent",
];

test("the default mix mirrors the engine: full tables, unity levels, even channels left, odd right", () => {
  const mix = defaultMix();

  assert.equal(mix.channelGains.length, MAX_CHANNELS);
  assert.equal(mix.pans.length, MAX_CHANNELS);
  assert.equal(mix.channelMuted.length, MAX_CHANNELS);
  assert.equal(mix.channelSolo.length, MAX_CHANNELS);
  assert.equal(mix.groupLevels.length, MAX_GROUPS);
  assert.ok(mix.channelGains.every((gain) => gain === 100));
  assert.deepEqual(mix.pans.slice(0, 4), [0, 100, 0, 100]);
  assert.equal(mix.volumePercent, 100);
  assert.equal(mix.maxLevelPercent, 100);
  assert.equal(mix.muted, false);
});

test("a status row's flat mix fields become a mix, and nothing else does", () => {
  const row = {
    address: "192.168.1.3:50000",
    linkHealth: "good",
    ...defaultMix(),
    volumePercent: 42,
  };

  const mix = mixFromDto(row);

  assert.deepEqual(Object.keys(mix).sort(), MIX_FIELDS);
  assert.equal(mix.volumePercent, 42);
});

test("a mix keeps the engine's revision, and a payload without one reads as the oldest", () => {
  assert.equal(defaultMix().revision, 0);
  assert.equal(mixFromDto({ ...defaultMix(), revision: 41 }).revision, 41);
  const { revision: _revision, ...withoutRevision } = defaultMix();
  assert.equal(mixFromDto(withoutRevision).revision, 0);
});

test("the revision is the engine's, never part of an edit or a request", () => {
  assert.throws(() => parseEditKey("revision"), /mix edit/);
  const mix = { ...defaultMix(), revision: 7 };
  assert.deepEqual(buildMixRequest(mix, { volumePercent: 80 }), { volumePercent: 80 });
  assert.equal(applyEdits(mix, { volumePercent: 80 }).revision, 7);
});

test("a mix read from a payload owns its tables", () => {
  const payload = defaultMix();

  const mix = mixFromDto(payload);
  payload.channelGains[0] = 7;

  assert.equal(mix.channelGains[0], 100);
});

test("edit keys name a whole-mix field or one entry of a table", () => {
  assert.equal(editKey("volumePercent"), "volumePercent");
  assert.equal(editKey("channelGains", 3), "channelGains.3");
  assert.deepEqual(parseEditKey("channelGains.3"), { field: "channelGains", index: 3 });
  assert.deepEqual(parseEditKey("muted"), { field: "muted", index: null });
  assert.deepEqual(parseEditKey("groupLevels.15"), { field: "groupLevels", index: 15 });
});

test("an edit the engine would not accept fails loudly", () => {
  for (const key of [
    "volume",
    "channelGains",
    "channelGains.32",
    "groupLevels.16",
    "muted.0",
    "pans.-1",
    "pans.x",
  ]) {
    assert.throws(() => parseEditKey(key), /mix edit/, key);
  }
  assert.throws(() => editKey("channelGains"), /mix edit/);
});

test("edits show on top of a mix without changing it", () => {
  const mix = defaultMix();

  const view = applyEdits(mix, { "channelGains.2": 40, muted: true, "pans.0": 50 });

  assert.equal(view.channelGains[2], 40);
  assert.equal(view.pans[0], 50);
  assert.equal(view.muted, true);
  assert.equal(view.channelGains[1], 100);
  assert.equal(mix.channelGains[2], 100);
  assert.equal(mix.pans[0], 0);
  assert.equal(mix.muted, false);
});

test("with no edits there is nothing to send", () => {
  assert.equal(buildMixRequest(defaultMix(), {}), null);
  assert.equal(buildMixRequest(defaultMix(), undefined), null);
});

test("a whole-mix edit sends only that field", () => {
  assert.deepEqual(buildMixRequest(defaultMix(), { volumePercent: 80 }), { volumePercent: 80 });
});

test("one channel's change sends its table from the first channel up to it, the others as they are", () => {
  const mix = defaultMix();
  mix.channelGains[0] = 70;
  mix.channelGains[1] = 60;
  mix.channelGains[2] = 50;

  assert.deepEqual(buildMixRequest(mix, { "channelGains.3": 40 }), {
    channelGains: [70, 60, 50, 40],
  });
});

test("several edits in one table send it up to the furthest one", () => {
  assert.deepEqual(buildMixRequest(defaultMix(), { "pans.4": 50, "pans.1": 30 }), {
    pans: [0, 30, 0, 100, 50],
  });
});

test("edits across tables and whole-mix fields travel together", () => {
  const request = buildMixRequest(defaultMix(), {
    "channelMuted.1": true,
    "channelSolo.0": true,
    "groupLevels.0": 55,
    maxLevelPercent: 90,
    muted: true,
  });

  assert.deepEqual(request, {
    channelMuted: [false, true],
    channelSolo: [true],
    groupLevels: [55],
    maxLevelPercent: 90,
    muted: true,
  });
});

test("levels go out as whole numbers from 0 to 100, as the engine's integer fields need", () => {
  const request = buildMixRequest(defaultMix(), {
    "channelGains.0": 62.6,
    volumePercent: 140,
    "pans.0": -5,
  });

  assert.deepEqual(request, { channelGains: [63], volumePercent: 100, pans: [0] });
});

test("the engine's merge of a built request gives the mix the edits show", () => {
  const mix = defaultMix();
  mix.channelGains[5] = 12;
  mix.groupLevels[1] = 33;
  const edits = {
    "channelGains.3": 40,
    "pans.2": 50,
    "channelSolo.6": true,
    "groupLevels.0": 70,
    volumePercent: 64,
    muted: true,
  };

  assert.deepEqual(mergeMixUpdate(mix, buildMixRequest(mix, edits)), applyEdits(mix, edits));
});

test("the engine's merge replaces a table from the first entry and keeps the rest", () => {
  const merged = mergeMixUpdate(defaultMix(), { channelGains: [10, 20] });

  assert.deepEqual(merged.channelGains.slice(0, 3), [10, 20, 100]);
  assert.equal(merged.channelGains.length, MAX_CHANNELS);
});

test("the engine's merge clamps levels and keeps absent fields", () => {
  const mix = { ...defaultMix(), volumePercent: 42 };

  const merged = mergeMixUpdate(mix, { pans: [150, -3], maxLevelPercent: 120 });

  assert.deepEqual(merged.pans.slice(0, 3), [100, 0, 0]);
  assert.equal(merged.maxLevelPercent, 100);
  assert.equal(merged.volumePercent, 42);
  assert.equal(mix.pans[0], 0, "the merge returns a new mix");
});

test("the engine's merge rejects an unknown field and a table longer than the mix", () => {
  assert.throws(() => mergeMixUpdate(defaultMix(), { volume: 80 }), /volume/);
  assert.throws(
    () => mergeMixUpdate(defaultMix(), { groupLevels: new Array(MAX_GROUPS + 1).fill(1) }),
    /groupLevels/,
  );
});

test("an answer settles the edits it carried and keeps those changed since", () => {
  const sent = { "channelGains.0": 40, "channelGains.1": 50, volumePercent: 80 };
  const now = { "channelGains.0": 40, "channelGains.1": 55, "pans.0": 50 };

  assert.deepEqual(settleEdits(now, sent), { "channelGains.1": 55, "pans.0": 50 });
});

test("when every edit was answered nothing stays pending", () => {
  assert.deepEqual(settleEdits({ muted: true }, { muted: true }), {});
  assert.deepEqual(settleEdits(undefined, { muted: true }), {});
});
