// Named sessions through the console (console-app.js) and the fake bridge, which keeps the
// desktop's rules: saving keeps the desk session and every musician's mix by their key, loading
// replaces the desk session, applies it live and pushes each saved mix to the musician here.
import test from "node:test";
import assert from "node:assert/strict";

import { addGroup, renameChannel, setChannelKind } from "../app/lib/channels-view.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { musicianByAddress, pendingEdits } from "../app/lib/console-state.js";
import { startRequest } from "../app/lib/engine-config.js";
import { createFakeBridge, sampleStatus } from "../app/lib/fake-bridge.js";
import { sessionPanelView } from "../app/lib/sessions.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const KEYS = "192.168.1.17:50000";
const LEAD_KEY = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const KEYS_KEY = "client:c81e4f2a-9b3d-4a7e-8c6f-0d2b5e9a1f37";

function commands(fake, name) {
  return fake.calls.filter(({ command }) => command === name);
}

async function connected(options = {}) {
  let now = 1000;
  const fake = createFakeBridge({ running: true, clock: () => (now += 1000), ...options });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  return { fake, app };
}

// A saved session the way the fake keeps one: the desk session and the mixes by key.
function savedSession(id, changes = {}) {
  return {
    id,
    name: "Sunday",
    savedAt: 500,
    session: {
      labels: [[LEAD_KEY, "Ana"]],
      groups: [{ id: "kit", name: "Kit", channels: [1, 2] }],
      channels: [{ name: "Kick in", kind: "drums", hidden: false, link: false }],
      engineer: [],
      mirrors: [],
      presets: [{ id: "p9", name: "Loud", mix: { channels: [], groups: {} } }],
      audio: null,
      ...changes.session,
    },
    mixes: {
      [LEAD_KEY]: {
        channels: [{ gain: 11, pan: 50, muted: false, solo: false }],
        groups: { kit: 22 },
        volumePercent: 33,
        muted: false,
      },
      ...changes.mixes,
    },
  };
}

test("the console reads the saved sessions and what the desk said about its session when it connects", async () => {
  const { app } = await connected({
    savedSessions: [savedSession("s9")],
    sessionNotice: "imported 2 labels from musicians.json",
  });

  const view = sessionPanelView(app.store.getState());

  assert.equal(view.notice, "When the console opened its saved session: imported 2 labels from musicians.json.");
  assert.deepEqual(view.rows.map((row) => row.name), ["Sunday"]);
  app.dispose();
});

test("a saved session keeps the desk session and every musician's mix by their key", async () => {
  const { fake, app } = await connected();

  const saved = await app.saveSession("  Show ");

  assert.deepEqual(saved, { ok: true, message: 'Saved the session as "Show", with 4 musicians\' mixes.' });
  const listed = app.store.getState().sessions.list;
  assert.deepEqual(listed.map(({ name, mixes }) => ({ name, mixes })), [{ name: "Show", mixes: 4 }]);
  const refused = await app.saveSession("SHOW");
  assert.equal(refused.ok, false);
  assert.match(refused.message, /^The session was not saved: a session named "SHOW" is already saved\.$/);

  // The mixes are the engine's at save time: a later change does not reach the saved session.
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [1] } });
  const loaded = await app.loadSession(listed[0].id);
  assert.equal(loaded.ok, true, loaded.message);
  assert.equal(musicianByAddress(app.store.getState(), LEAD).mix.channelGains[0], 62);
  app.dispose();
});

test("a load pushes each saved mix to the musician here and names those it did not reach", async () => {
  const status = sampleStatus();
  // Keys' phone is listed but its control channel is down.
  status.musicians[2].controlConnected = false;
  const { fake, app } = await connected({
    status,
    savedSessions: [
      savedSession("s9", {
        mixes: {
          [KEYS_KEY]: { channels: [], groups: {}, volumePercent: 10, muted: true },
          "client:gone": { channels: [], groups: {}, volumePercent: 10, muted: true },
        },
        session: { labels: [[LEAD_KEY, "Ana"], ["client:gone", "Gone"]] },
      }),
    ],
  });
  const result = await app.loadSession("s9");

  assert.equal(
    result.message,
    'Loaded "Sunday": its channels, groups, presets, labels, limits, locks and mirrors apply now. ' +
      "Pushed the saved mix to Lead vocal. " +
      "Not pushed to Keys: not connected, and a phone's own mix wins when it connects. " +
      "Not pushed to Gone: not here.",
  );
  const state = app.store.getState();
  const lead = musicianByAddress(state, LEAD).mix;
  assert.equal(lead.channelGains[0], 11);
  assert.equal(lead.groupLevels[0], 22, "the level of the group it was saved for, by id");
  assert.equal(lead.volumePercent, 33);
  assert.equal(lead.maxLevelPercent, 90, "the phone's own cap is never pushed");
  const keys = musicianByAddress(state, KEYS).mix;
  assert.deepEqual([keys.volumePercent, keys.muted], [100, false], "a phone away keeps its own mix");
  assert.deepEqual(
    state.engine.status.groups.map(({ id, name }) => ({ id, name })),
    [{ id: "kit", name: "Kit" }],
  );
  assert.equal(state.engine.status.channels[0].name, "Kick in");
  assert.deepEqual(state.presets.map((preset) => preset.name), ["Loud"]);
  assert.equal(commands(fake, "start_engine").length, 0, "the engine is never restarted");
  app.dispose();
});

test("after a load the next group edit builds on the loaded groups, not the last answer", async () => {
  const { fake, app } = await connected({ savedSessions: [savedSession("s9")] });
  const added = await app.editGroups(addGroup);
  assert.equal(added.ok, true, added.message);

  await app.loadSession("s9");
  const after = await app.editGroups(addGroup);

  assert.equal(after.ok, true, after.message);
  const sent = commands(fake, "set_groups").at(-1).args.groups;
  assert.deepEqual(
    sent.map(({ id, name, channels }) => ({ id, name, channels })).slice(0, 1),
    [{ id: "kit", name: "Kit", channels: [1, 2] }],
    "the loaded group, kept with its id",
  );
  assert.equal(sent.length, 2, "the loaded group and the new one, not the groups before the load");
  app.dispose();
});

test("a load drops the edits still waiting once it loaded, and none goes out over it", async () => {
  const { fake, app } = await connected({ savedSessions: [savedSession("s9")] });
  fake.holdAnswers("set_mix");
  fake.holdAnswers("load_session");
  app.edit(BASS, { "channelGains.0": 70 });
  app.edit(BASS, { "channelGains.1": 60 });
  assert.equal(app.unsavedEdits(), 2);

  const loading = app.loadSession("s9");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(app.unsavedEdits(), 2, "kept while the desk has not answered");
  fake.releaseAnswers();
  const result = await loading;
  await new Promise((resolve) => setImmediate(resolve));

  assert.equal(result.ok, true, result.message);
  assert.equal(app.unsavedEdits(), 0);
  assert.deepEqual(pendingEdits(app.store.getState(), BASS), {});
  assert.equal(commands(fake, "set_mix").length, 1, "the waiting edit never went out over the load");
  app.dispose();
});

test("a refused load keeps the edits still waiting, and they go out", async () => {
  const { fake, app } = await connected({ savedSessions: [savedSession("s9")] });
  fake.holdAnswers("set_mix");
  app.edit(BASS, { "channelGains.0": 70 });
  app.edit(BASS, { "channelGains.1": 60 });
  fake.failNext("load_session", "the disk is gone");

  const result = await app.loadSession("s9");

  assert.deepEqual(result, { ok: false, message: '"Sunday" was not loaded: the disk is gone.' });
  assert.equal(app.unsavedEdits(), 2, "the engineer's edits are not lost for nothing");
  fake.releaseAnswers();
  await new Promise((resolve) => setImmediate(resolve));
  fake.releaseAnswers();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(commands(fake, "set_mix").length, 2, "the waiting edit went out after the refusal");
  assert.equal(app.unsavedEdits(), 0);
  const bass = musicianByAddress(app.store.getState(), BASS).mix;
  assert.deepEqual([bass.channelGains[0], bass.channelGains[1]], [70, 60]);
  app.dispose();
});

test("a channel or group edit on its way counts, and the load waits for it", async () => {
  const { fake, app } = await connected({ savedSessions: [savedSession("s9")] });
  fake.holdAnswers("set_groups");
  const editing = app.editGroups(addGroup);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(app.unsavedEdits(), 1);

  const loading = app.loadSession("s9");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(commands(fake, "load_session").length, 0, "the load waits for the edit's answer");
  fake.releaseAnswers();
  await editing;
  await loading;

  assert.equal(commands(fake, "load_session").length, 1);
  assert.deepEqual(
    app.store.getState().engine.status.groups.map((group) => group.id),
    ["kit"],
    "the loaded groups, not the edit's",
  );
  app.dispose();
});

test("a loaded audio input is opened by the next start, never ASIO without its picked driver", async () => {
  const asio = { host: "asio", device: null, bufferFrames: 128, asioDriver: null };
  const { fake, app } = await connected({
    savedAudio: { host: "wasapi", device: "Demo interface", bufferFrames: 256, asioDriver: null },
    savedSessions: [
      savedSession("bare", { session: { audio: asio } }),
      savedSession("picked", { session: { audio: { ...asio, asioDriver: "Demo ASIO driver" } } }),
    ],
  });
  const before = app.store.getState().audio;

  const bare = await app.loadSession("bare");

  assert.match(bare.message, /Its audio input was not taken: ASIO is kept only with the ASIO driver/);
  assert.deepEqual(app.store.getState().audio, before, "the desk keeps its own");
  assert.equal(fake.savedAudio().host, "wasapi");

  const picked = await app.loadSession("picked");

  assert.match(picked.message, /Its audio input is used from the next start\.$/);
  const audio = app.store.getState().audio;
  assert.equal(audio.host, "asio");
  assert.equal(audio.asioDriver, "Demo ASIO driver");
  assert.equal(startRequest(audio).deviceFilter, "Demo ASIO driver", "the next start opens it");
  assert.equal(commands(fake, "start_engine").length, 0, "nothing restarted");
  await app.stop();
  await app.start();
  assert.deepEqual(commands(fake, "start_engine").at(-1).args.config.host, "asio");
  app.dispose();
});

test("with the engine stopped a load applies at the next start and pushes no mix", async () => {
  const fake = createFakeBridge({ savedSessions: [savedSession("s9")] });
  const app = createConsoleApp({ bridge: fake, autoStart: false });
  await app.ready;

  const result = await app.loadSession("s9");

  assert.equal(
    result.message,
    'Loaded "Sunday": it applies at the next start. The saved mix was not pushed: the engine is not running.',
  );
  await app.start();
  assert.deepEqual(
    app.store.getState().engine.status.groups.map((group) => group.id),
    ["kit"],
    "the next start takes the loaded groups",
  );
  app.dispose();
});

test("a kind a newer build wrote survives an edit of another input, and an explicit Other replaces it", async () => {
  const { fake, app } = await connected({
    savedSessions: [
      savedSession("s9", {
        session: {
          channels: [
            { name: "Kick", kind: "drums", hidden: false, link: false },
            { name: "Theremin", kind: "theremin", hidden: false, link: false },
          ],
        },
      }),
    ],
  });
  await app.loadSession("s9");
  assert.equal(app.store.getState().engine.status.channels[1].kind, "other", "it applies as other");

  const renamed = await app.editChannels((table, count = 6) => renameChannel(table, 0, "Kick in", count));

  assert.equal(renamed.ok, true, renamed.message);
  assert.equal((await fake.invoke("channel_table"))[1].kind, "theremin");
  const other = await app.editChannels((table) => setChannelKind(table, 1, "other", 6));
  assert.equal(other.ok, true, other.message);
  assert.equal((await fake.invoke("channel_table"))[1].kind, "other");
  app.dispose();
});

test("saved sessions are renamed and deleted, and one that cannot be read is said so", async () => {
  const broken = { id: "broken", name: "broken", savedAt: null, problem: "the file is not valid JSON" };
  const { app } = await connected({ savedSessions: [savedSession("s9"), broken] });

  assert.deepEqual(await app.renameSession("s9", "Late show"), {
    ok: true,
    message: 'Renamed the saved session "Sunday" to "Late show".',
  });
  const refused = await app.loadSession("broken");
  assert.equal(refused.ok, false);
  assert.match(refused.message, /^"broken" was not loaded: the saved session `broken` cannot be loaded/);
  assert.equal((await app.renameSession("broken", "Fixed")).ok, false);
  assert.deepEqual(await app.deleteSession("broken"), {
    ok: true,
    message: 'Deleted the saved session "broken".',
  });
  assert.deepEqual(
    app.store.getState().sessions.list.map((saved) => saved.name),
    ["Late show"],
  );
  app.dispose();
});
