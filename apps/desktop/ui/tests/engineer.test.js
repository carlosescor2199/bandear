// The engineer's limit and locks: the mix model, the strips, the console app and the fake engine.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { keepLocked, keepPairsEqual, lockPairs } from "../app/lib/channels.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { actions, consoleReducer, initialState, pendingEdits, viewMix } from "../app/lib/console-state.js";
import { createFakeBridge, sampleStatus, syntheticMeters } from "../app/lib/fake-bridge.js";
import { formatEngineerControlsSet } from "../app/lib/format.js";
import {
  ENGINEER_FIELDS,
  MAX_CHANNELS,
  buildEngineerRequest,
  buildMixRequest,
  defaultMix,
  editKey,
  mergeEngineerUpdate,
  mergeMixUpdate,
  mixFromDto,
  splitEdits,
} from "../app/lib/mix.js";
import { channelEdits, channelStrips, masterStrip, mixMessages } from "../app/lib/mix-view.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const LEAD_PHONE = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";

function locks(...channels) {
  const table = new Array(MAX_CHANNELS).fill(false);
  for (const channel of channels) {
    table[channel] = true;
  }
  return table;
}

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
}

async function until(done, limitMs = 2000) {
  const deadline = Date.now() + limitMs;
  while (!done() && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

// --- The mix model -------------------------------------------------------------------------------

test("every mix carries the engineer's limit and locks, open by default and on an older payload", () => {
  const mix = defaultMix();
  assert.deepEqual(ENGINEER_FIELDS, ["engineerLimitPercent", "lockedChannels"]);
  assert.equal(mix.engineerLimitPercent, 100);
  assert.deepEqual(mix.lockedChannels, locks());

  const older = mixFromDto({ ...defaultMix(), engineerLimitPercent: undefined, lockedChannels: undefined });
  assert.equal(older.engineerLimitPercent, 100, "a desk that predates them reads as no limit");
  assert.deepEqual(older.lockedChannels, locks(), "and nothing locked");

  const row = mixFromDto({ ...defaultMix(), engineerLimitPercent: 60, lockedChannels: locks(2) });
  assert.equal(row.engineerLimitPercent, 60);
  assert.deepEqual(row.lockedChannels, locks(2));
});

test("the engineer's edits are kept apart from the mix's, and go out as their own request", () => {
  const edits = {
    "channelGains.1": 40,
    [editKey("engineerLimitPercent")]: 60,
    [editKey("lockedChannels", 2)]: true,
  };

  const { mix, engineer } = splitEdits(edits);

  assert.deepEqual(mix, { "channelGains.1": 40 });
  assert.deepEqual(engineer, { engineerLimitPercent: 60, "lockedChannels.2": true });
  const base = { ...defaultMix(), lockedChannels: locks(0) };
  assert.deepEqual(buildEngineerRequest(base, engineer), {
    limitPercent: 60,
    lockedChannels: [true, false, true],
  });
  assert.deepEqual(buildEngineerRequest(base, { "lockedChannels.1": true }), {
    lockedChannels: [true, true],
  });
  assert.equal(buildEngineerRequest(base, {}), null);
  assert.deepEqual(buildMixRequest(base, mix), { channelGains: [100, 40] });
});

test("set_mix cannot carry the engineer's controls", () => {
  assert.throws(() => mergeMixUpdate(defaultMix(), { engineerLimitPercent: 100 }), /engineerLimitPercent/);
  assert.throws(() => mergeMixUpdate(defaultMix(), { lockedChannels: [false] }), /lockedChannels/);
});

test("the engineer's change merges as the engine merges it: a clamped limit, locks from CH 1 on", () => {
  const mix = { ...defaultMix(), engineerLimitPercent: 70, lockedChannels: locks(3) };

  assert.deepEqual(mergeEngineerUpdate(mix, { lockedChannels: [true] }), {
    engineerLimitPercent: 70,
    lockedChannels: locks(0, 3),
  });
  assert.equal(mergeEngineerUpdate(mix, { limitPercent: 150 }).engineerLimitPercent, 100);
  assert.equal(mergeEngineerUpdate(mix, { limitPercent: -4 }).engineerLimitPercent, 0);
  assert.throws(() => mergeEngineerUpdate(mix, { lockedChannels: new Array(33).fill(true) }), /locked/);
  assert.throws(() => mergeEngineerUpdate(mix, { limit: 60 }), /limit/);
});

// --- The engine's rules, for the fake ------------------------------------------------------------

test("a phone's change to a locked channel is put back, and the rest of it stands", () => {
  const before = {
    ...defaultMix(),
    channelGains: [40, 50, ...new Array(30).fill(100)],
    lockedChannels: locks(0),
  };
  const merged = mergeMixUpdate(before, {
    channelGains: [90, 80],
    pans: [50, 50],
    channelMuted: [true, true],
    volumePercent: 70,
  });

  const kept = keepLocked(before, merged);

  assert.equal(kept.channelGains[0], 40);
  assert.equal(kept.pans[0], 0);
  assert.equal(kept.channelMuted[0], false);
  assert.equal(kept.channelGains[1], 80);
  assert.equal(kept.channelMuted[1], true);
  assert.equal(kept.volumePercent, 70);
  assert.deepEqual(before.channelGains.slice(0, 2), [40, 50], "the mix before is left as it was");
});

test("a pair locks as one: either side's lock covers both, and a change of one side moves both", () => {
  const channels = [
    { link: false },
    { link: true },
    { link: false },
    { link: false },
  ];

  assert.deepEqual(lockPairs(locks(2), channels), locks(1, 2));
  assert.deepEqual(lockPairs(locks(3), channels), locks(3));
  const before = { lockedChannels: locks() };
  assert.deepEqual(
    keepPairsEqual(before, { lockedChannels: locks(2) }, channels, ["lockedChannels"]).lockedChannels,
    locks(1, 2),
  );
});

// --- What the Mix screen shows -------------------------------------------------------------------

function withControls(row, limit, locked) {
  return { ...row, engineerLimitPercent: limit, lockedChannels: locked };
}

function stateWithControls(limit, locked, change = (status) => status) {
  const status = sampleStatus();
  status.musicians[0] = withControls(status.musicians[0], limit, locked);
  return reduce([actions.status(change(status))]);
}

test("a channel strip says whether it is locked, in words", () => {
  const strips = channelStrips(stateWithControls(100, locks(1)), LEAD);

  assert.equal(strips[0].locked, false);
  assert.equal(strips[0].lockText, "Unlocked");
  assert.equal(strips[1].locked, true);
  assert.equal(strips[1].lockText, "Locked");
});

test("a pair shows locked when either side is, and its lock goes to both inputs", () => {
  const state = stateWithControls(100, locks(2), (status) => {
    status.channels[1] = { ...status.channels[1], link: true };
    return status;
  });
  const pair = channelStrips(state, LEAD)[1];

  assert.deepEqual(pair.sides, [1, 2]);
  assert.equal(pair.locked, true);
  assert.deepEqual(channelEdits(pair, "lockedChannels", false), {
    "lockedChannels.1": false,
    "lockedChannels.2": false,
  });
});

test("the master shows the engineer's limit apart from the phone's cap and says the lower wins", () => {
  const limited = masterStrip(stateWithControls(60, locks()), LEAD);

  assert.equal(limited.cap, 90);
  assert.equal(limited.limit, 60);
  assert.equal(limited.readout, "85% · cap 90% · limit 60%");
  assert.equal(limited.ceilingNote, "The lower of the phone's cap and your limit wins: at most 60%.");

  const capped = masterStrip(stateWithControls(95, locks()), LEAD);
  assert.equal(capped.ceilingNote, "The lower of the phone's cap and your limit wins: at most 90%.");
});

test("the engineer's pending edits show at once, as a fader's do", () => {
  const state = reduce(
    [actions.mixEdit(LEAD, { engineerLimitPercent: 50, "lockedChannels.0": true })],
    stateWithControls(100, locks()),
  );

  assert.equal(masterStrip(state, LEAD).limit, 50);
  assert.equal(channelStrips(state, LEAD)[0].locked, true);
  assert.equal(viewMix(state, LEAD).engineerLimitPercent, 50);
});

test("a lock waiting while an input joins a pair is dropped, as its level would be", () => {
  const before = reduce(
    [actions.mixEdit(LEAD, { "lockedChannels.1": true, engineerLimitPercent: 40 })],
    stateWithControls(100, locks()),
  );
  const paired = sampleStatus();
  paired.musicians[0] = withControls(paired.musicians[0], 100, locks());
  paired.channels[1] = { ...paired.channels[1], link: true };

  const after = reduce([actions.status(paired)], before);

  assert.deepEqual(pendingEdits(after, LEAD), { engineerLimitPercent: 40 });
});

test("an answer that took the controls but could not keep them says so above the mix", () => {
  const sent = { engineerLimitPercent: 60 };
  const outcome = {
    address: LEAD,
    applied: true,
    saved: false,
    saveError: "the disk is full.",
    mix: null,
  };

  const state = reduce(
    [actions.mixEdit(LEAD, sent), actions.engineerApplied(LEAD, sent, outcome)],
    stateWithControls(100, locks()),
  );

  assert.deepEqual(pendingEdits(state, LEAD), {});
  assert.equal(
    mixMessages(state).controls,
    "Lead vocal's limit and locks apply now, but were not saved: the disk is full.",
  );
  const cleared = reduce(
    [actions.engineerApplied(LEAD, {}, { ...outcome, saved: true, saveError: null })],
    state,
  );
  assert.equal(mixMessages(cleared).controls, null);
});

test("what a change of the engineer's controls did, in words", () => {
  assert.equal(
    formatEngineerControlsSet({ applied: true, saved: true, saveError: null }),
    "Limit and locks apply now and are saved.",
  );
  assert.equal(
    formatEngineerControlsSet({ applied: false, saved: true, saveError: null }),
    "Limit and locks saved; they apply at the next start.",
  );
  assert.equal(
    formatEngineerControlsSet({ applied: true, saved: false, saveError: null }),
    "Limit and locks apply now, but were not saved.",
  );
});

// --- The console app ------------------------------------------------------------------------------

async function connected(options = {}) {
  const fake = createFakeBridge({ running: true, ...options });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  return { fake, app };
}

test("the engine API sends the engineer's controls by the names the desktop command takes", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async () => () => {},
  });

  await api.setEngineerControls(LEAD, { limitPercent: 60 }, "phone-1");
  await api.setEngineerControls(LEAD, { limitPercent: 50 });

  assert.deepEqual(calls, [
    ["set_engineer_controls", { address: LEAD, controls: { limitPercent: 60 }, clientId: "phone-1" }],
    ["set_engineer_controls", { address: LEAD, controls: { limitPercent: 50 }, clientId: null }],
  ]);
});

test("the engineer's edits go to set_engineer_controls one at a time, never to set_mix", async () => {
  const { fake, app } = await connected();
  fake.holdAnswers("set_engineer_controls");

  app.edit(LEAD, { engineerLimitPercent: 70 });
  app.edit(LEAD, { engineerLimitPercent: 60, "lockedChannels.1": true });
  assert.equal(commands(fake, "set_engineer_controls").length, 1, "one request in flight");
  fake.releaseAnswers();
  await until(() => commands(fake, "set_engineer_controls").length === 2);
  fake.releaseAnswers();
  await until(() => Object.keys(pendingEdits(app.store.getState(), LEAD)).length === 0);

  const clientId = LEAD_PHONE.slice("client:".length);
  assert.deepEqual(
    commands(fake, "set_engineer_controls").map(({ args }) => args),
    [
      { address: LEAD, controls: { limitPercent: 70 }, clientId },
      { address: LEAD, controls: { limitPercent: 60, lockedChannels: [false, true] }, clientId },
    ],
    "each carries the row's client id, so a desk with no engine keeps it for the phone",
  );
  assert.equal(commands(fake, "set_mix").length, 0);
  const mix = viewMix(app.store.getState(), LEAD);
  assert.equal(mix.engineerLimitPercent, 60);
  assert.equal(mix.lockedChannels[1], true);
  app.dispose();
});

test("a fader move and a lock at once go out on their own commands", async () => {
  const { fake, app } = await connected();

  app.edit(LEAD, { "channelGains.0": 30, "lockedChannels.0": true });
  await until(() => Object.keys(pendingEdits(app.store.getState(), LEAD)).length === 0);

  assert.deepEqual(commands(fake, "set_mix").map(({ args }) => args.mix), [{ channelGains: [30] }]);
  assert.deepEqual(
    commands(fake, "set_engineer_controls").map(({ args }) => args.controls),
    [{ lockedChannels: [true] }],
  );
  app.dispose();
});

test("a refused change of the engineer's controls is said above the mix and the edit goes", async () => {
  const { fake, app } = await connected();
  fake.failNext("set_engineer_controls", "the engine is stopping");

  app.edit(LEAD, { engineerLimitPercent: 20 });
  await until(() => app.store.getState().mixError !== null);

  assert.equal(
    mixMessages(app.store.getState()).error,
    "A change to Lead vocal's mix was not applied: the engine is stopping.",
  );
  assert.deepEqual(pendingEdits(app.store.getState(), LEAD), {});
  app.dispose();
});

// --- The fake engine ------------------------------------------------------------------------------

test("the fake engine takes the engineer's controls, announces the mix and answers as the desktop", async () => {
  const fake = createFakeBridge({ running: true });
  const changes = [];
  await fake.listen("mix-changed", (change) => changes.push(change));

  const outcome = await fake.invoke("set_engineer_controls", {
    address: LEAD,
    controls: { limitPercent: 60, lockedChannels: [true] },
  });

  assert.equal(outcome.address, LEAD);
  assert.equal(outcome.limitPercent, 60);
  assert.deepEqual(outcome.lockedChannels, locks(0));
  assert.equal(outcome.keptFor, "phone");
  assert.equal(outcome.applied, true);
  assert.equal(outcome.saved, true);
  assert.equal(outcome.mix.engineerLimitPercent, 60);
  assert.equal(changes.length, 1);
  assert.equal(changes[0].origin, "desktop");
  assert.deepEqual(changes[0].mix.lockedChannels, locks(0));
  await assert.rejects(
    fake.invoke("set_engineer_controls", { address: LEAD, controls: { limit: 60 } }),
    /limit/,
  );
  await assert.rejects(
    fake.invoke("set_mix", { address: LEAD, mix: { engineerLimitPercent: 100 } }),
    /engineerLimitPercent/,
  );
});

test("on the fake engine a phone cannot change a locked channel, and the desk still can", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: LEAD, controls: { lockedChannels: [true] } });
  const changes = [];
  await fake.listen("mix-changed", (change) => changes.push(change));

  fake.changeFromPhone(LEAD, { channelGains: [10, 20], maxLevelPercent: 50 });
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [33] } });

  const [fromPhone, fromDesk] = changes.map(({ mix }) => mix);
  assert.deepEqual(fromPhone.channelGains.slice(0, 2), [62, 20], "CH 1 is kept");
  assert.equal(fromPhone.maxLevelPercent, 50, "the phone's cap is its own");
  assert.equal(fromDesk.channelGains[0], 33);
});

test("a pair on the fake engine locks as one, also when it is linked after the lock", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", {
    address: LEAD,
    controls: { lockedChannels: [false, false, true] },
  });
  const table = await fake.invoke("channel_table");
  table[1] = { ...table[1], link: true };

  await fake.invoke("set_channels", { channels: table });

  const status = await fake.invoke("engine_status");
  assert.deepEqual(status.musicians[0].lockedChannels.slice(0, 4), [false, true, true, false]);
  const unlocked = await fake.invoke("set_engineer_controls", {
    address: LEAD,
    controls: { lockedChannels: [false, false] },
  });
  assert.deepEqual(unlocked.lockedChannels.slice(0, 4), [false, false, false, false]);
});

test("the fake engine's limit caps a musician's meter as the lower of it and the phone's cap", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: LEAD, controls: { limitPercent: 5 } });
  const status = await fake.invoke("engine_status");

  for (let time = 0; time < 5000; time += 250) {
    const { left, right } = syntheticMeters(status, time).musicians[LEAD];
    assert.ok(left <= 5 && right <= 5, `${left} ${right} at ${time} ms`);
  }
});

test("controls set while the fake engine is stopped are kept for the address and apply at the next start", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: LEAD, controls: { limitPercent: 45 } });
  await fake.invoke("stop_engine");

  const idle = await fake.invoke("set_engineer_controls", {
    address: BASS,
    controls: { lockedChannels: [true] },
  });
  await fake.invoke("start_engine", { config: {} });

  assert.equal(idle.applied, false);
  assert.equal(idle.keptFor, "address");
  assert.equal(idle.mix, null);
  const status = await fake.invoke("engine_status");
  const lead = status.musicians.find((row) => row.address === LEAD);
  const bass = status.musicians.find((row) => row.address === BASS);
  assert.equal(lead.engineerLimitPercent, 45, "kept for the phone, given back at the start");
  assert.equal(bass.lockedChannels[0], true);
  assert.ok(fake.engineerKeys().includes(LEAD_PHONE));
});

test("an idle edit for a known phone is kept for the phone, and that is what the next start gives", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: LEAD, controls: { limitPercent: 40 } });
  await fake.invoke("stop_engine");
  await fake.invoke("set_engineer_controls", {
    address: LEAD,
    controls: { limitPercent: 70 },
  });

  const idle = await fake.invoke("set_engineer_controls", {
    address: LEAD,
    controls: { lockedChannels: [true] },
    clientId: LEAD_PHONE.slice("client:".length),
  });
  await fake.invoke("start_engine", { config: {} });

  assert.equal(idle.keptFor, "phone");
  assert.equal(idle.limitPercent, 40, "built on the phone's own controls");
  assert.deepEqual(fake.engineerKeys(), [LEAD_PHONE], "those kept for the address are gone");
  const lead = (await fake.invoke("engine_status")).musicians.find((row) => row.address === LEAD);
  assert.equal(lead.engineerLimitPercent, 40);
  assert.equal(lead.lockedChannels[0], true, "the start gives exactly the idle edit");
  await assert.rejects(
    fake.invoke("set_engineer_controls", { address: "somewhere", controls: {} }),
    /somewhere/,
  );
});

test("a phone back from a new IP keeps the controls the desk kept for it", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_engineer_controls", { address: LEAD, controls: { limitPercent: 30 } });

  fake.movePhone(LEAD, "192.168.1.33:50000");

  const status = await fake.invoke("engine_status");
  assert.equal(
    status.musicians.find((row) => row.address === "192.168.1.33:50000").engineerLimitPercent,
    30,
  );
});
