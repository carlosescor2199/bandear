import test from "node:test";
import assert from "node:assert/strict";

import { createConsoleApp } from "../app/lib/console-app.js";
import { musicianByAddress } from "../app/lib/console-state.js";
import { createFakeBridge, isSocketAddress, sampleStatus } from "../app/lib/fake-bridge.js";
import { channelStrips, groupStrips } from "../app/lib/mix-view.js";
import { createMixTools, mixToolCommands } from "../app/lib/mix-tools.js";
import { mixToolsView } from "../app/lib/mixes.js";

const [LEAD, BASS, KEYS, DRUMS] = sampleStatus().musicians.map((musician) => musician.address);
const BASS_PHONE = sampleStatus().musicians[1].clientId;

async function rowAt(fake, address) {
  const status = await fake.invoke("engine_status");
  return status.musicians.find((musician) => musician.address === address);
}

const SHARED = ["channelGains", "pans", "channelMuted", "channelSolo", "groupLevels"];

function shared(row) {
  return Object.fromEntries(SHARED.map((field) => [field, row[field]]));
}

async function changesHeard(fake) {
  const heard = [];
  await fake.listen("mix-changed", (payload) => heard.push([payload.address, payload.origin]));
  return heard;
}

async function refusal(promise) {
  try {
    await promise;
  } catch (error) {
    return String(error);
  }
  assert.fail("the call should have been refused");
}

test("a preset is saved from a running musician's mix, renamed and deleted, its name checked", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mix", { address: LEAD, mix: { groupLevels: [40, 70] } });

  const saved = await fake.invoke("save_preset", { address: LEAD, name: "  Verse " });

  assert.equal(saved.preset.name, "Verse");
  assert.equal(saved.preset.mix.channels.length, 6, "one entry per input");
  assert.deepEqual(saved.preset.mix.channels[0], { gain: 62, pan: 50, muted: false, solo: false });
  assert.deepEqual(saved.preset.mix.groups, { drums: 40, vocals: 70 });
  assert.deepEqual(await fake.invoke("presets"), saved.presets);
  assert.match(await refusal(fake.invoke("save_preset", { address: LEAD, name: "VERSE" })), /already/);
  const renamed = await fake.invoke("rename_preset", { id: saved.preset.id, name: "Chorus" });
  assert.equal(renamed.preset.name, "Chorus");
  assert.match(await refusal(fake.invoke("rename_preset", { id: "p-gone", name: "x" })), /no preset/);
  assert.deepEqual(await fake.invoke("delete_preset", { id: saved.preset.id }), {
    preset: null,
    presets: [],
  });
  await fake.invoke("stop_engine");
  assert.match(
    await refusal(fake.invoke("save_preset", { address: LEAD, name: "Bridge" })),
    /not running/,
  );
});

test("a preset lands on another musician by group id after the groups moved, never on a follower", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [11, 22], groupLevels: [40, 70] } });
  const { preset } = await fake.invoke("save_preset", { address: LEAD, name: "Verse" });
  await fake.invoke("set_groups", {
    groups: [
      { id: "vocals", name: "Vocals", channels: [5, 6] },
      { id: "drums", name: "Drums", channels: [1, 2, 3] },
    ],
  });
  const heard = await changesHeard(fake);

  const applied = await fake.invoke("apply_preset", { id: preset.id, musicians: [BASS] });

  const bass = await rowAt(fake, BASS);
  assert.deepEqual(bass.groupLevels.slice(0, 2), [70, 40], "Vocals and Drums by id");
  assert.deepEqual(bass.channelGains.slice(0, 2), [11, 22]);
  assert.deepEqual(applied.mixes.map(({ address }) => address), [BASS]);
  assert.deepEqual(heard, [[BASS, "desktop"]]);
  await fake.invoke("set_mirror", { address: KEYS, leader: { address: BASS, name: "Bass" } });
  assert.match(
    await refusal(fake.invoke("apply_preset", { id: preset.id, musicians: [DRUMS, KEYS] })),
    /follows Bass's mix/,
  );
  assert.equal((await rowAt(fake, DRUMS)).channelGains[0], 100, "nobody's mix changed");
});

test("a copy carries channels, pans, mutes, solos and group levels, never a master, cap or the engineer's controls", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: BASS, controls: { limitPercent: 50 } });
  await fake.invoke("set_mix", { address: BASS, mix: { volumePercent: 40, maxLevelPercent: 70 } });
  const lead = await rowAt(fake, LEAD);

  await fake.invoke("copy_mix", { from: LEAD, to: [BASS, KEYS] });

  for (const address of [BASS, KEYS]) {
    assert.deepEqual(shared(await rowAt(fake, address)), shared(lead), address);
  }
  const bass = await rowAt(fake, BASS);
  assert.equal(bass.volumePercent, 40);
  assert.equal(bass.maxLevelPercent, 70);
  assert.equal(bass.engineerLimitPercent, 50);
  assert.match(await refusal(fake.invoke("copy_mix", { from: LEAD, to: [LEAD] })), /onto itself/);
});

test("a follower takes its leader's mix at once and every later change, from the desk or the phone, and on down a chain", async () => {
  const fake = createFakeBridge({ running: true });
  const heard = await changesHeard(fake);

  const started = await fake.invoke("set_mirror", {
    address: BASS,
    clientId: BASS_PHONE,
    leader: { address: LEAD, name: "Lead vocal" },
  });
  await fake.invoke("set_mirror", { address: KEYS, leader: { address: BASS, name: "Bass" } });

  assert.equal(started.applied, true);
  assert.equal(started.keptFor, "phone");
  assert.deepEqual(shared(await rowAt(fake, BASS)), shared(await rowAt(fake, LEAD)));
  assert.deepEqual((await rowAt(fake, BASS)).mirror, {
    leader: `client:${sampleStatus().musicians[0].clientId}`,
    leaderAddress: LEAD,
    name: "Lead vocal",
  });
  heard.length = 0;
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [5] } });
  assert.deepEqual(heard, [
    [LEAD, "desktop"],
    [BASS, "desktop"],
    [KEYS, "desktop"],
  ]);
  fake.changeFromPhone(LEAD, { channelGains: [6], volumePercent: 99 });
  for (const address of [BASS, KEYS]) {
    const follower = await rowAt(fake, address);
    assert.equal(follower.channelGains[0], 6, address);
    assert.notEqual(follower.volumePercent, 99, "the master stays the follower's");
  }
  fake.changeFromPhone(BASS, { channelGains: [90], groupLevels: [10], volumePercent: 33 });
  const bass = await rowAt(fake, BASS);
  assert.equal(bass.channelGains[0], 6, "its phone cannot move a channel");
  assert.equal(bass.groupLevels[0], 100, "nor a group");
  assert.equal(bass.volumePercent, 33, "its master is its own");
});

test("a mirror never goes round in a circle, and stopping one leaves the mix as it is", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mirror", { address: BASS, leader: { address: LEAD, name: "Lead" } });
  await fake.invoke("set_mirror", { address: KEYS, leader: { address: BASS, name: "Bass" } });

  for (const [follower, leader] of [
    [LEAD, BASS],
    [LEAD, KEYS],
    [LEAD, LEAD],
  ]) {
    assert.match(
      await refusal(fake.invoke("set_mirror", { address: follower, leader: { address: leader } })),
      /circle|own mix/,
      `${follower} following ${leader}`,
    );
  }
  const before = shared(await rowAt(fake, BASS));
  const stopped = await fake.invoke("set_mirror", { address: BASS, leader: null });
  assert.equal(stopped.leader, null);
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [3] } });
  assert.deepEqual(shared(await rowAt(fake, BASS)), before, "it no longer follows");
  assert.equal((await rowAt(fake, BASS)).mirror, null);
});

test("a mirror set with no engine is kept for the phone and in force at the next start", async () => {
  const fake = createFakeBridge();
  await fake.invoke("start_engine", { config: {} });
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [44] } });
  await fake.invoke("stop_engine");

  const kept = await fake.invoke("set_mirror", {
    address: BASS,
    clientId: BASS_PHONE,
    leader: { address: LEAD, name: "Lead vocal" },
  });

  assert.deepEqual(
    { applied: kept.applied, saved: kept.saved, keptFor: kept.keptFor, mix: kept.mix },
    { applied: false, saved: true, keptFor: "phone", mix: null },
  );
  assert.deepEqual(fake.mirrors(), [
    [`client:${BASS_PHONE}`, { leader: `address:${LEAD}`, name: "Lead vocal" }],
  ]);
  assert.match(
    await refusal(fake.invoke("set_mirror", { address: "lead.local:50000", leader: null })),
    /invalid musician address/,
  );
  await fake.invoke("start_engine", { config: {} });
  assert.equal((await rowAt(fake, BASS)).channelGains[0], 44, "it took the leader's mix at start");
});

test("a musician address reads as the desktop reads it: an IP and a port, never a host name", () => {
  for (const address of ["192.168.1.3:50000", "[fe80::1]:50000", "[::1%3]:1"]) {
    assert.equal(isSocketAddress(address), true, address);
  }
  for (const address of ["studio.local:50000", "192.168.1.3", "192.168.1.300:50000", "01.2.3.4:5", ":50000", "", null]) {
    assert.equal(isSocketAddress(address), false, String(address));
  }
});

test("the Mix tools answer in words, and a follower's strips say whose mix they follow", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;

  assert.deepEqual(app.store.getState().presets, [], "read when the console connects");
  const saved = await app.savePreset(LEAD, "Verse");
  assert.deepEqual(saved, { ok: true, message: 'Saved Lead vocal\'s mix as "Verse".' });
  const [verse] = app.store.getState().presets;
  assert.deepEqual(await app.applyPreset(verse.id, [BASS, KEYS]), {
    ok: true,
    message: 'Applied "Verse" to Bass and Keys.',
  });
  assert.deepEqual(await app.renamePreset(verse.id, "Chorus"), {
    ok: true,
    message: 'Renamed "Verse" to "Chorus".',
  });
  assert.deepEqual(await app.copyMix(LEAD, [DRUMS]), {
    ok: true,
    message: "Copied Lead vocal's mix to Drums: channels, pans, mutes, solos and group levels.",
  });
  assert.deepEqual(await app.setMirror(BASS, LEAD), {
    ok: true,
    message: "Bass follows Lead vocal's mix now.",
  });

  const state = app.store.getState();
  assert.equal(musicianByAddress(state, BASS).mirror.name, "Lead vocal");
  assert.equal(channelStrips(state, BASS)[0].following, "Lead vocal");
  assert.equal(groupStrips(state, BASS)[0].following, "Lead vocal");
  assert.equal(channelStrips(state, LEAD)[0].following, null);
  const circle = await app.setMirror(LEAD, BASS);
  assert.equal(circle.ok, false);
  assert.match(circle.message, /^The mirror was not changed: .*circle\.$/);
  assert.deepEqual(await app.deletePreset(verse.id), {
    ok: true,
    message: 'Deleted the preset "Chorus".',
  });
  assert.deepEqual(app.store.getState().presets, []);
  app.dispose();
});

test("the Mix screen's wiring hands the console's mirror answers to the tools, done or refused", async () => {
  const fake = createFakeBridge({ running: true });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  app.select(LEAD);
  const said = [];
  const handled = [];
  let commands = null;
  const tools = createMixTools({
    emit: (type, detail) => handled.push(commands.handle(type, detail)),
  });
  const show = () => tools.setData({ ...mixToolsView(app.store.getState(), LEAD), address: LEAD });
  // What the Mirror control shows the moment each answer reaches the tools.
  const shownOnAnswer = [];
  const answered = tools.answered;
  tools.answered = (kind, ok) => {
    const cleared = answered(kind, ok);
    shownOnAnswer.push(tools.mirrorValue());
    return cleared;
  };
  // As the screen does: the outcome is said, and the tools take the store's data at once.
  commands = mixToolCommands({
    app,
    tools,
    said(outcome) {
      said.push(outcome);
      show();
    },
  });

  show();
  tools.chooseMirror(KEYS);
  assert.equal((await handled[0]).ok, true);
  assert.deepEqual(shownOnAnswer, [KEYS], "the new mirror at once, never the old one in between");

  // Bass starts following Lead by another route while the tools still offer Bass to Lead.
  await fake.invoke("set_mirror", { address: BASS, leader: { address: LEAD, name: "Lead" } });
  tools.chooseMirror(BASS);
  assert.equal(tools.mirrorValue(), BASS, "on its way");
  const refused = await handled[1];
  assert.equal(refused.ok, false);
  assert.match(refused.message, /circle/);
  assert.equal(tools.mirrorValue(), KEYS, "the refusal reached the tools: the desk's mirror shows");
  assert.deepEqual(
    said.map((outcome) => outcome.ok),
    [true, false],
  );
  app.dispose();
});

test("a refused preset says why, and presets that could not be read are read again on asking", async () => {
  const fake = createFakeBridge({
    running: true,
    presets: [{ id: "p9", name: "Verse", mix: { channels: [], groups: {} } }],
  });
  fake.failNext("presets", "the session is busy");
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  assert.equal(app.store.getState().presets, null, "not known yet");

  assert.equal(await app.loadPresets(), null);
  assert.deepEqual(
    app.store.getState().presets.map((preset) => preset.name),
    ["Verse"],
  );
  assert.deepEqual(await app.savePreset(LEAD, "verse"), {
    ok: false,
    message: 'The preset was not saved: a preset named "verse" already exists.',
  });
  app.dispose();
});
