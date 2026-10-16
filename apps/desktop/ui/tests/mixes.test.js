import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";
import { defaultMix, mergeMixUpdate } from "../app/lib/mix.js";
import {
  MAX_PRESET_NAME_LENGTH,
  checkPresetName,
  closesCircle,
  formatCopied,
  formatMirrorSet,
  formatPresetApplied,
  joinNames,
  mirrorChoice,
  mixTargets,
  presetOf,
  sameShared,
  withPreset,
  withShared,
} from "../app/lib/mixes.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const KEYS = "192.168.1.17:50000";
const DRUMS = "192.168.1.20:50000";

// The sample engine with `mirrors` on its rows: `{ [follower]: [leaderAddress, name] }`.
function withMirrors(mirrors) {
  const status = sampleStatus();
  for (const row of status.musicians) {
    const mirror = mirrors[row.address];
    row.mirror = mirror ? { leader: `address:${mirror[0]}`, leaderAddress: mirror[0], name: mirror[1] } : null;
  }
  return consoleReducer(initialState, actions.status(status));
}

function mixWith(changes) {
  return mergeMixUpdate(defaultMix(), changes);
}

test("a preset name is trimmed, never empty, never too long and never another preset's", () => {
  const presets = [
    { id: "p1", name: "Verse" },
    { id: "p2", name: "Chorus" },
  ];

  assert.equal(checkPresetName("  Bridge ", presets), "Bridge");
  assert.equal(checkPresetName("verse", presets, "p1"), "verse", "renaming one keeps its own name");
  assert.throws(() => checkPresetName("   ", presets), /needs a name/);
  assert.throws(() => checkPresetName("x".repeat(MAX_PRESET_NAME_LENGTH + 1), presets), /at most 40/);
  assert.throws(() => checkPresetName("CHORUS", presets), /already exists/);
  assert.throws(() => checkPresetName("chorus", presets, "p1"), /already exists/);
});

test("a preset keeps each input of the device and the group levels by group id", () => {
  const status = sampleStatus();
  const mix = mixWith({
    channelGains: [10, 20, 30, 40, 50, 60, 70],
    pans: [0, 100],
    channelMuted: [false, true],
    channelSolo: [true],
    groupLevels: [80, 40],
    volumePercent: 33,
  });

  const preset = presetOf(mix, status);

  assert.equal(preset.channels.length, 6, "one entry per input of the device");
  assert.deepEqual(preset.channels[0], { gain: 10, pan: 0, muted: false, solo: true });
  assert.deepEqual(preset.channels[1], { gain: 20, pan: 100, muted: true, solo: false });
  assert.deepEqual(preset.groups, { drums: 80, vocals: 40 });
  assert.equal("volumePercent" in preset, false, "never the master");
});

test("a preset lands on the live groups by id, and keeps the master, the cap and the engineer's controls", () => {
  const preset = {
    channels: [
      { gain: 40, pan: 0, muted: true, solo: false },
      { gain: 70, pan: 100, muted: false, solo: false },
    ],
    groups: { drums: 10, gone: 55, vocals: 20 },
  };
  const live = [{ id: "vocals" }, { id: "keys" }, { id: "drums" }];
  const mix = {
    ...mixWith({ groupLevels: [90, 80, 70], volumePercent: 60, maxLevelPercent: 80 }),
    engineerLimitPercent: 50,
  };
  mix.lockedChannels[0] = true;

  const applied = withPreset(mix, preset, live, [{ link: true }, {}, {}]);

  assert.deepEqual(applied.groupLevels.slice(0, 3), [20, 80, 10], "by id; Keys keeps its own");
  assert.deepEqual(applied.channelGains.slice(0, 3), [40, 40, 100], "the pair's first side wins");
  assert.deepEqual(applied.channelMuted.slice(0, 2), [true, true]);
  assert.deepEqual(applied.pans.slice(0, 2), [0, 100], "the balance is the preset's");
  assert.equal(applied.volumePercent, 60);
  assert.equal(applied.maxLevelPercent, 80);
  assert.equal(applied.engineerLimitPercent, 50);
  assert.equal(applied.lockedChannels[0], true);
  assert.deepEqual(mix.groupLevels.slice(0, 3), [90, 80, 70], "the mix given is left alone");
});

test("a copy carries the channels and group levels and nothing of the musician's own", () => {
  const leader = mixWith({ channelGains: [5, 6], pans: [30], channelSolo: [true], groupLevels: [44] });
  const mix = { ...mixWith({ volumePercent: 61, maxLevelPercent: 71 }), engineerLimitPercent: 40 };

  const copied = withShared(mix, leader);

  assert.equal(sameShared(copied, leader), true);
  assert.equal(sameShared(mix, leader), false);
  assert.deepEqual(copied.channelGains.slice(0, 2), [5, 6]);
  assert.equal(copied.groupLevels[0], 44);
  assert.equal(copied.volumePercent, 61);
  assert.equal(copied.maxLevelPercent, 71);
  assert.equal(copied.engineerLimitPercent, 40);
});

test("a mirror that would go round in a circle is found, directly or through others", () => {
  const { musicians } = withMirrors({ [BASS]: [LEAD, "Lead vocal"], [KEYS]: [BASS, "Bass"] });

  assert.equal(closesCircle(musicians, LEAD, BASS), true, "Bass follows Lead");
  assert.equal(closesCircle(musicians, LEAD, KEYS), true, "Keys follows Bass, who follows Lead");
  assert.equal(closesCircle(musicians, LEAD, LEAD), true, "nobody follows their own mix");
  assert.equal(closesCircle(musicians, KEYS, DRUMS), false);
  assert.equal(closesCircle(musicians, DRUMS, KEYS), false);
});

test("the Mirror control offers every other musician but never a circle, and says whom the mix follows", () => {
  const state = withMirrors({ [BASS]: [LEAD, "Lead vocal"] });

  const lead = mirrorChoice(state, LEAD);
  const bass = mirrorChoice(state, BASS);

  assert.deepEqual(
    lead.options.map((option) => option.label),
    ["Off", "Keys", "Drums"],
    "Bass follows Lead, so Lead cannot follow Bass",
  );
  assert.equal(lead.value, "");
  assert.equal(lead.note, null);
  assert.equal(bass.value, LEAD);
  assert.deepEqual(
    bass.options.map((option) => option.value),
    ["", LEAD, KEYS, DRUMS],
  );
  assert.match(bass.note, /^Bass follows Lead vocal's mix: its channels and groups move with it/);
});

test("a follower whose leader is away keeps the last mix, and the Mirror control says so", () => {
  const status = sampleStatus();
  status.musicians[1].mirror = { leader: "client:gone", leaderAddress: null, name: "Ana" };
  const state = consoleReducer(initialState, actions.status(status));

  const choice = mirrorChoice(state, BASS);

  assert.equal(choice.value, "away");
  assert.deepEqual(choice.options.at(-1), { value: "away", label: "Ana (not here)" });
  assert.match(choice.note, /Ana is not here, so Bass's mix stays as Ana left it/);
});

test("a copy or a preset reaches every other musician, never one whose mix follows another's", () => {
  const state = withMirrors({ [KEYS]: [BASS, "Bass"] });

  assert.deepEqual(mixTargets(state, LEAD), [
    { address: BASS, label: "Bass", disabled: false, note: null },
    { address: KEYS, label: "Keys", disabled: true, note: "follows Bass" },
    { address: DRUMS, label: "Drums", disabled: false, note: null },
  ]);
  const all = mixTargets(state, KEYS, { includeSelf: true });
  assert.deepEqual(all[0], {
    address: KEYS,
    label: "Keys",
    disabled: true,
    note: "this mix, follows Bass",
  });
  assert.equal(all.length, 4);
});

test("the desk's answers to presets, copies and mirrors read as sentences", () => {
  assert.equal(joinNames(["Bass"]), "Bass");
  assert.equal(joinNames(["Bass", "Keys", "Drums"]), "Bass, Keys and Drums");
  assert.equal(
    formatCopied("Lead vocal", ["Bass", "Keys"]),
    "Copied Lead vocal's mix to Bass and Keys: channels, pans, mutes, solos and group levels.",
  );
  assert.equal(formatPresetApplied("Verse", ["Bass"]), 'Applied "Verse" to Bass.');
  const followed = { leader: LEAD, applied: true, saved: true, saveError: null };
  assert.equal(formatMirrorSet(followed, "Bass", "Lead vocal"), "Bass follows Lead vocal's mix now.");
  assert.equal(
    formatMirrorSet({ ...followed, applied: false }, "Bass", "Lead vocal"),
    "Bass will follow Lead vocal's mix from the next start.",
  );
  assert.equal(
    formatMirrorSet({ ...followed, leader: null }, "Bass", null),
    "Bass no longer follows another mix; their mix stays as it is.",
  );
  assert.equal(
    formatMirrorSet({ ...followed, saved: false, saveError: "the disk is full." }, "Bass", "Lead vocal"),
    "Bass follows Lead vocal's mix now, but this was not saved: the disk is full.",
  );
});
