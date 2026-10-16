import test from "node:test";
import assert from "node:assert/strict";

import { EVENTS, createEngineApi, createTauriBridge } from "../app/lib/bridge.js";
import { createFakeBridge, sampleStatus, syntheticMeters } from "../app/lib/fake-bridge.js";
import { formatChannelsSet, formatGroupsSet } from "../app/lib/format.js";
import { applyEdits, buildMixRequest, mixFromDto } from "../app/lib/mix.js";

function recordingBridge() {
  const calls = [];
  const listens = [];
  return {
    calls,
    listens,
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async (event) => {
      listens.push(event);
      return () => {};
    },
  };
}

test("the Tauri bridge passes commands through to invoke", async () => {
  const seen = [];
  const tauri = {
    core: {
      invoke: async (command, args) => {
        seen.push([command, args]);
        return "answer";
      },
    },
    event: { listen: async () => () => {} },
  };

  const answer = await createTauriBridge(tauri).invoke("set_mix", { address: "a", mix: {} });

  assert.equal(answer, "answer");
  assert.deepEqual(seen, [["set_mix", { address: "a", mix: {} }]]);
});

test("the Tauri bridge hands listeners the payload and returns the unlisten function", async () => {
  let registered;
  const unlisten = () => {};
  const tauri = {
    core: { invoke: async () => null },
    event: {
      listen: async (event, handler) => {
        registered = { event, handler };
        return unlisten;
      },
    },
  };
  const payloads = [];

  const result = await createTauriBridge(tauri).listen("meters", (payload) => payloads.push(payload));
  registered.handler({ event: "meters", id: 1, payload: { channels: [5] } });

  assert.equal(registered.event, "meters");
  assert.deepEqual(payloads, [{ channels: [5] }]);
  assert.equal(result, unlisten);
});

test("the Tauri bridge titles the app's window", async () => {
  const titles = [];
  const tauri = {
    core: { invoke: async () => null },
    event: { listen: async () => () => {} },
    window: { getCurrentWindow: () => ({ setTitle: async (title) => titles.push(title) }) },
  };

  await createTauriBridge(tauri).setTitle("MixLink Server — Volt 4");

  assert.deepEqual(titles, ["MixLink Server — Volt 4"]);
});

test("without the Tauri globals the bridge says so", () => {
  assert.throws(() => createTauriBridge(undefined), /Tauri/);
  assert.throws(() => createTauriBridge({ core: {} }), /Tauri/);
});

test("the engine API calls each command with the argument names the desktop commands take", async () => {
  const bridge = recordingBridge();
  const api = createEngineApi(bridge);

  await api.status();
  await api.start({ host: "wasapi" });
  await api.start();
  await api.stop();
  await api.listInputDevices("wasapi");
  await api.listInputDevices();
  await api.switchDevice("Volt 4");
  await api.addTarget("192.168.1.9");
  await api.removeTarget("192.168.1.9:50000");
  await api.setMusicianName("192.168.1.9:50000", "Keys");
  await api.setMix("192.168.1.9:50000", { volumePercent: 80 });
  await api.setGroups([
    { id: "kit", name: "Drums", channels: [1, 2] },
    { name: "Vocals", channels: [3] },
  ]);
  await api.setChannels([{ name: "Kick", kind: "drums", hidden: false, link: false }]);

  assert.deepEqual(bridge.calls, [
    ["engine_status", undefined],
    ["start_engine", { config: { host: "wasapi" } }],
    ["start_engine", { config: {} }],
    ["stop_engine", undefined],
    ["list_input_devices", { host: "wasapi" }],
    ["list_input_devices", { host: null }],
    ["switch_device", { device: "Volt 4" }],
    ["add_target", { address: "192.168.1.9" }],
    ["remove_target", { address: "192.168.1.9:50000" }],
    ["set_musician_name", { address: "192.168.1.9:50000", name: "Keys" }],
    ["set_mix", { address: "192.168.1.9:50000", mix: { volumePercent: 80 } }],
    [
      "set_groups",
      {
        groups: [
          { id: "kit", name: "Drums", channels: [1, 2] },
          { name: "Vocals", channels: [3] },
        ],
      },
    ],
    ["set_channels", { channels: [{ name: "Kick", kind: "drums", hidden: false, link: false }] }],
  ]);
});

test("the engine API reads and keeps the audio input with the argument names the desktop takes", async () => {
  const bridge = recordingBridge();
  const api = createEngineApi(bridge);
  const volt = { host: "wasapi", device: "Volt 4", bufferFrames: 256, asioDriver: null };

  await api.audioSettings();
  await api.setAudioSettings(volt);

  assert.deepEqual(bridge.calls, [
    ["audio_settings", undefined],
    ["set_audio_settings", { settings: volt }],
  ]);
});

test("the fake keeps the audio input as the desktop's session does", async () => {
  const volt = { host: "wasapi", device: "Volt 4", bufferFrames: 256, asioDriver: null };
  const fake = createFakeBridge();
  assert.equal(await fake.invoke("audio_settings"), null, "nothing kept yet");

  await assert.rejects(fake.invoke("set_audio_settings", { settings: volt }), /not running/);
  await fake.invoke("start_engine", { config: {} });
  await assert.rejects(
    fake.invoke("set_audio_settings", { settings: { ...volt, host: "asio", asioDriver: " " } }),
    /ASIO/,
  );
  await assert.rejects(
    fake.invoke("set_audio_settings", { settings: { ...volt, host: "kernel" } }),
    /kernel/,
  );
  assert.equal(fake.savedAudio(), null, "nothing refused is kept");
  assert.deepEqual(
    await fake.invoke("set_audio_settings", { settings: { ...volt, host: "WASAPI", device: " Volt 4 " } }),
    volt,
  );
  assert.deepEqual(await fake.invoke("audio_settings"), volt);
  assert.deepEqual(fake.savedAudio(), volt);

  const seeded = createFakeBridge({ savedAudio: volt });
  assert.deepEqual(await seeded.invoke("audio_settings"), volt, "a session kept before");
});

test("the fake refuses a start on an input it does not have, as the engine does", async () => {
  const fake = createFakeBridge();

  await assert.rejects(
    fake.invoke("start_engine", { config: { host: "wasapi", deviceFilter: "INPUT 1/2 (Volt 4)" } }),
    /no input device name contains "input 1\/2 \(volt 4\)"/,
  );
  const summary = await fake.invoke("start_engine", {
    config: { host: "wasapi", deviceFilter: "usb AUDIO" },
  });
  assert.equal(summary.deviceName, "Microphone (USB audio)", "a part of the name, in any case");
});

test("the engine API listens for the three engine events", async () => {
  const bridge = recordingBridge();
  const api = createEngineApi(bridge);

  await api.onMeters(() => {});
  await api.onStatus(() => {});
  await api.onMixChanged(() => {});

  assert.deepEqual(bridge.listens, ["meters", "status", "mix-changed"]);
  assert.deepEqual(EVENTS, { meters: "meters", status: "status", mixChanged: "mix-changed" });
});

test("the fake engine can fail a command once or a few times, before or after it takes effect", async () => {
  const fake = createFakeBridge({ running: true });

  fake.failNext("stop_engine", "could not stop the capture");
  await assert.rejects(fake.invoke("stop_engine"), /could not stop the capture/);
  assert.equal((await fake.invoke("engine_status")).state, "running", "the engine still runs");

  fake.failNext("engine_status", "busy", { times: 2 });
  await assert.rejects(fake.invoke("engine_status"), /busy/);
  await assert.rejects(fake.invoke("engine_status"), /busy/);
  assert.equal((await fake.invoke("engine_status")).state, "running");

  fake.failNext("stop_engine", "stopped, but late", { afterEffect: true });
  await assert.rejects(fake.invoke("stop_engine"), /stopped, but late/);
  assert.equal((await fake.invoke("engine_status")).state, "notRunning", "this time it stopped");
});

test("the fake engine says whether it runs, with its status when it does, as the desktop does", async () => {
  const fake = createFakeBridge();

  assert.deepEqual(await fake.invoke("engine_status"), { state: "notRunning", run: 0 });
  await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");

  assert.equal(status.state, "running");
  assert.equal(status.musicians.length, sampleStatus().musicians.length);
  assert.equal(status.stopped, false);
  assert.deepEqual(
    fake.calls.map(({ command }) => command),
    ["engine_status", "start_engine", "engine_status"],
  );
});

test("starting the fake engine announces a status and stopping it ends engine_status", async () => {
  const fake = createFakeBridge();
  const statuses = [];
  await fake.listen("status", (payload) => statuses.push(payload));

  const summary = await fake.invoke("start_engine", { config: {} });
  await fake.invoke("stop_engine");

  assert.equal(summary.deviceName, sampleStatus().deviceName);
  assert.equal(statuses.length, 1);
  assert.deepEqual(await fake.invoke("engine_status"), { state: "notRunning", run: 1 });
});

test("a fake engine can start without announcing a status, as a window without a pump", async () => {
  const fake = createFakeBridge({ statusOnStart: false });
  const statuses = [];
  await fake.listen("status", (payload) => statuses.push(payload));

  await fake.invoke("start_engine", { config: {} });

  assert.deepEqual(statuses, []);
});

test("the fake engine names its run on every payload, and each start begins the next", async () => {
  const fake = createFakeBridge();
  const seen = [];
  for (const event of ["status", "meters", "mix-changed"]) {
    await fake.listen(event, (payload) => seen.push([event, payload.run]));
  }

  const first = await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");
  fake.changeFromPhone(status.musicians[0].address, { muted: true });
  fake.emitMeters();
  const second = await fake.invoke("start_engine", { config: {} });

  assert.equal(first.run, 1);
  assert.equal(status.run, 1);
  assert.equal(second.run, 2);
  assert.equal((await fake.invoke("engine_status")).run, 2);
  assert.deepEqual(seen, [
    ["status", 1],
    ["mix-changed", 1],
    ["meters", 1],
    ["status", 2],
  ]);
});

test("a failed start of the fake engine still ends the run before it", async () => {
  const fake = createFakeBridge({ running: true, failStart: "No input device is available." });

  await assert.rejects(fake.invoke("start_engine", { config: {} }), /No input device/);
  assert.deepEqual(await fake.invoke("engine_status"), { state: "notRunning", run: 2 });
  fake.failStart(null);
  const summary = await fake.invoke("start_engine", { config: {} });

  assert.equal(summary.run, 3, "the sample engine was run 1 and the failed start took run 2");
});

test("the fake engine merges a desk's change from the first channel, answers the mix and announces it", async () => {
  const fake = createFakeBridge({ running: true });
  const changes = [];
  await fake.listen("mix-changed", (payload) => changes.push(payload));
  const before = sampleStatus().musicians[1];

  const answer = await fake.invoke("set_mix", {
    address: before.address,
    mix: { channelGains: [10, 20] },
  });

  assert.deepEqual(answer.channelGains, [10, 20, ...before.channelGains.slice(2)]);
  assert.deepEqual(changes, [{ run: 1, address: before.address, origin: "desktop", mix: answer }]);
  const status = await fake.invoke("engine_status");
  assert.deepEqual(status.musicians[1].channelGains, answer.channelGains);
});

test("every change of a fake mix takes a higher revision, from the desk or the phone", async () => {
  const fake = createFakeBridge({ running: true });
  const { address, revision: start } = sampleStatus().musicians[0];

  const answer = await fake.invoke("set_mix", { address, mix: { volumePercent: 50 } });
  fake.changeFromPhone(address, { muted: true });
  const row = (await fake.invoke("engine_status")).musicians[0];

  assert.ok(answer.revision > start);
  assert.ok(row.revision > answer.revision);
});

test("the fake engine can answer a desk's change before announcing it, as the desktop may", async () => {
  const fake = createFakeBridge({ running: true, answersFirst: true });
  const order = [];
  await fake.listen("mix-changed", () => order.push("event"));
  const { address } = sampleStatus().musicians[0];

  await fake.invoke("set_mix", { address, mix: { volumePercent: 50 } });
  order.push("answer");
  await new Promise((resolve) => setTimeout(resolve, 5));

  assert.deepEqual(order, ["answer", "event"]);
});

test("the fake engine refuses what the real one refuses", async () => {
  const fake = createFakeBridge({ running: true });
  const { address } = sampleStatus().musicians[0];

  await assert.rejects(fake.invoke("set_mix", { address, mix: { volume: 80 } }), /volume/);
  await assert.rejects(fake.invoke("set_mix", { address: "10.0.0.1:50000", mix: {} }), /10\.0\.0\.1/);
  await assert.rejects(fake.invoke("no_such_command"), /no_such_command/);
});

test("a change made on a phone reaches the listeners as a phone change", async () => {
  const fake = createFakeBridge({ running: true });
  const changes = [];
  await fake.listen("mix-changed", (payload) => changes.push(payload));
  const { address } = sampleStatus().musicians[0];

  fake.changeFromPhone(address, { volumePercent: 30 });

  assert.equal(changes.length, 1);
  assert.equal(changes[0].origin, "phone");
  assert.equal(changes[0].address, address);
  assert.equal(changes[0].mix.volumePercent, 30);
});

test("the fake engine adds and removes musicians by address", async () => {
  const fake = createFakeBridge({ running: true });
  const count = sampleStatus().musicians.length;

  const added = await fake.invoke("add_target", { address: "192.168.1.40" });
  const withNew = await fake.invoke("engine_status");

  assert.equal(added, "192.168.1.40:50000");
  assert.equal(withNew.musicians.length, count + 1);
  assert.equal(withNew.musicians.at(-1).linkHealth, "unknown");
  assert.equal(withNew.musicians.at(-1).controlConnected, false);
  await assert.rejects(fake.invoke("add_target", { address: "192.168.1.40:50001" }), /192\.168\.1\.40/);

  await fake.invoke("remove_target", { address: added });
  assert.equal((await fake.invoke("engine_status")).musicians.length, count);
  await assert.rejects(fake.invoke("remove_target", { address: added }), /192\.168\.1\.40/);
});

test("a local label names a musician that announced no name, and never one that did", async () => {
  const fake = createFakeBridge({ running: true });
  const announced = sampleStatus().musicians.find((musician) => musician.clientName);
  const added = await fake.invoke("add_target", { address: "192.168.1.41" });

  await fake.invoke("set_musician_name", { address: added, name: "Keys" });
  await fake.invoke("set_musician_name", { address: announced.address, name: "Someone" });
  const { musicians } = await fake.invoke("engine_status");

  const labelled = musicians.find((musician) => musician.address === added);
  assert.equal(labelled.name, "Keys");
  assert.equal(labelled.nameIsLocal, true);
  const named = musicians.find((musician) => musician.address === announced.address);
  assert.equal(named.name, announced.clientName);
  assert.equal(named.nameIsLocal, false);
});

test("the fake engine's phones carry client ids and a musician added by address has none", async () => {
  const fake = createFakeBridge({ running: true });
  const added = await fake.invoke("add_target", { address: "192.168.1.42" });

  const { musicians } = await fake.invoke("engine_status");

  const phones = musicians.filter((musician) => musician.address !== added);
  assert.ok(
    phones.every((musician) => typeof musician.clientId === "string" && musician.clientId !== ""),
    JSON.stringify(phones.map((musician) => musician.clientId)),
  );
  assert.equal(new Set(phones.map((musician) => musician.clientId)).size, phones.length);
  assert.equal(musicians.find((musician) => musician.address === added).clientId, null);
});

test("a phone back from a new IP keeps the label the desk gave it", async () => {
  const fake = createFakeBridge({ running: true });
  const silent = sampleStatus().musicians.find((musician) => !musician.clientName);
  await fake.invoke("set_musician_name", { address: silent.address, name: "Drum kit" });
  const statuses = [];
  await fake.listen("status", (status) => statuses.push(status));

  fake.movePhone(silent.address, "192.168.1.77:50000");
  const { musicians } = await fake.invoke("engine_status");

  assert.equal(
    musicians.some((musician) => musician.address === silent.address),
    false,
    "the row moved instead of doubling",
  );
  const moved = musicians.find((musician) => musician.address === "192.168.1.77:50000");
  assert.equal(moved.clientId, silent.clientId);
  assert.equal(moved.name, "Drum kit");
  assert.equal(moved.nameIsLocal, true);
  assert.equal(statuses.length, 1, "the move shows in the next status, as the engine's would");
});

test("the sample's local label is kept for its phone, and one given by address stays there", async () => {
  const fake = createFakeBridge({ running: true });
  const silent = sampleStatus().musicians.find((musician) => !musician.clientName);
  const rack = await fake.invoke("add_target", { address: "192.168.1.43" });
  await fake.invoke("set_musician_name", { address: rack, name: "Rack A" });

  fake.movePhone(silent.address, "192.168.1.78:50000");
  const { musicians } = await fake.invoke("engine_status");

  assert.equal(musicians.find((musician) => musician.address === "192.168.1.78:50000").name, silent.name);
  assert.equal(musicians.find((musician) => musician.address === rack).name, "Rack A");
  assert.throws(() => fake.movePhone(rack, "192.168.1.79:50000"), /client id/);
});

test("clearing a phone's label clears what the row shows", async () => {
  const fake = createFakeBridge({ running: true });
  const silent = sampleStatus().musicians.find((musician) => !musician.clientName);

  await fake.invoke("set_musician_name", { address: silent.address, name: "  " });
  const { musicians } = await fake.invoke("engine_status");

  const row = musicians.find((musician) => musician.address === silent.address);
  assert.equal(row.name, null);
  assert.equal(row.nameIsLocal, false);
});

const LEAD = sampleStatus().musicians[0].address;

// Every listened event, in order, as `[event, payload]`.
async function recordEvents(fake) {
  const seen = [];
  for (const event of ["status", "mix-changed"]) {
    await fake.listen(event, (payload) => seen.push([event, payload]));
  }
  return seen;
}

test("the sample's groups and the channels in them carry the groups' ids", () => {
  const status = sampleStatus();

  assert.deepEqual(
    status.groups.map(({ id, name }) => [id, name]),
    [
      ["drums", "Drums"],
      ["vocals", "Vocals"],
    ],
  );
  assert.deepEqual(
    status.channels.map((channel) => channel.groupId),
    ["drums", "drums", "drums", null, "vocals", "vocals"],
  );
});

test("editing the fake engine's groups moves every musician's levels with their groups by id", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mix", { address: LEAD, mix: { groupLevels: [40, 70] } });
  const before = (await fake.invoke("engine_status")).musicians[0].revision;
  const seen = await recordEvents(fake);

  // Vocals first and renamed, Drums deleted, Keys new.
  const outcome = await fake.invoke("set_groups", {
    groups: [
      { id: "vocals", name: "Vox", channels: [5, 6] },
      { name: "Keys", channels: [4] },
    ],
  });

  assert.deepEqual(
    { applied: outcome.applied, saved: outcome.saved, saveError: outcome.saveError },
    { applied: true, saved: true, saveError: null },
    "a running engine took them and they are kept",
  );
  const kept = outcome.groups;
  assert.equal(kept[0].id, "vocals");
  assert.ok(kept[1].id && kept[1].id !== "drums", `a new id: ${kept[1].id}`);
  assert.deepEqual(kept[1], { id: kept[1].id, name: "Keys", channels: [4] });
  const status = await fake.invoke("engine_status");
  assert.deepEqual(
    status.groups.map(({ id, name, channels, invalid }) => [id, name, channels, invalid]),
    [
      ["vocals", "Vox", [4, 5], false],
      [kept[1].id, "Keys", [3], false],
    ],
  );
  assert.deepEqual(
    status.channels.map((channel) => channel.groupId),
    [null, null, null, kept[1].id, "vocals", "vocals"],
  );
  const lead = status.musicians[0];
  assert.deepEqual(lead.groupLevels.slice(0, 3), [70, 100, 100]);
  assert.ok(lead.revision > before, "the move is a newer revision");
  assert.deepEqual(
    seen.map(([event, payload]) => [event, payload.address ?? null, payload.origin ?? null]),
    [
      ["status", null, null],
      ...status.musicians.map((musician) => ["mix-changed", musician.address, "desktop"]),
    ],
    "the new groups are announced before the levels placed for them",
  );
  assert.deepEqual(seen[1][1].mix.groupLevels.slice(0, 2), [70, 100]);
});

test("renaming a fake group or giving it other channels keeps every level in place", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mix", { address: LEAD, mix: { groupLevels: [40, 70] } });
  const seen = await recordEvents(fake);

  await fake.invoke("set_groups", {
    groups: [
      { id: "drums", name: "Kit", channels: [1, 2, 3, 4] },
      { id: "vocals", name: "Vocals", channels: [5, 6] },
    ],
  });

  const lead = (await fake.invoke("engine_status")).musicians[0];
  assert.deepEqual(lead.groupLevels.slice(0, 2), [40, 70]);
  assert.deepEqual(
    seen.map(([event]) => event),
    ["status"],
  );

  // The same groups again change nothing and announce nothing.
  await fake.invoke("set_groups", {
    groups: [
      { id: "drums", name: "Kit", channels: [1, 2, 3, 4] },
      { id: "vocals", name: "Vocals", channels: [5, 6] },
    ],
  });
  assert.equal(seen.length, 1);
});

test("the fake engine refuses the groups the desk refuses and keeps the ones it had", async () => {
  const fake = createFakeBridge({ running: true });
  const before = (await fake.invoke("engine_status")).groups;

  for (const groups of [
    [
      { name: "Drums", channels: [1, 2] },
      { name: "Bass", channels: [2] },
    ],
    [{ name: " ", channels: [1] }],
    [
      { id: "a", name: "Drums", channels: [1] },
      { id: "a", name: "Bass", channels: [2] },
    ],
    Array.from({ length: 17 }, (_, index) => ({ name: `Group ${index}`, channels: [] })),
  ]) {
    await assert.rejects(fake.invoke("set_groups", { groups }), /./);
  }

  assert.deepEqual((await fake.invoke("engine_status")).groups, before);
});

test("groups set while the fake engine is stopped apply at the next start, a missing input marked", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("stop_engine");

  const outcome = await fake.invoke("set_groups", {
    groups: [
      { id: "drums", name: "Drums", channels: [1, 2] },
      { id: "rack", name: "Rack", channels: [9] },
    ],
  });
  assert.equal(outcome.applied, false, "no engine took them");
  assert.equal(outcome.saved, true);
  assert.equal(formatGroupsSet(outcome), "Groups saved; they apply at the next start.");
  await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");

  assert.deepEqual(
    status.groups.map(({ id, invalid }) => [id, invalid]),
    [
      ["drums", false],
      ["rack", true],
    ],
  );
  assert.equal(status.channels.some((channel) => channel.groupId === "rack"), false);
});

test("a device switch marks the fake groups the new device cannot feed", async () => {
  const fake = createFakeBridge({ running: true });

  await fake.invoke("switch_device", { device: "Microphone (USB audio)" });
  const status = await fake.invoke("engine_status");

  assert.deepEqual(
    status.groups.map(({ id, invalid }) => [id, invalid]),
    [
      ["drums", true],
      ["vocals", true],
    ],
  );
  assert.deepEqual(
    status.channels.map((channel) => channel.groupId),
    [null, null],
  );
});

// Each channel's details as `[name, kind, hidden, link]`.
function details(channels) {
  return channels.map(({ name, kind, hidden, link }) => [name, kind, hidden, link]);
}

const KEYS = [
  { name: " Kick ", kind: "drums" },
  { name: "Keys L", kind: "keys", link: true },
  { name: "Keys R", kind: "keys" },
];

test("the sample's channels carry the engineer's details", () => {
  assert.deepEqual(details(sampleStatus().channels), [
    ["Kick", "drums", false, false],
    ["Snare", "drums", false, false],
    ["Overheads", "drums", false, false],
    ["Bass", "bass", false, false],
    ["Lead vox", "voice", false, false],
    ["Backing vox", "voice", false, false],
  ]);
});

test("linking a pair on the fake engine pans it apart in every mix after the new details", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [62, 70, 40] } });
  const before = (await fake.invoke("engine_status")).musicians[0].revision;
  const seen = await recordEvents(fake);

  const outcome = await fake.invoke("set_channels", { channels: KEYS });

  assert.deepEqual(outcome, {
    channels: [
      { name: "Kick", kind: "drums", hidden: false, link: false },
      { name: "Keys L", kind: "keys", hidden: false, link: true },
      { name: "Keys R", kind: "keys", hidden: false, link: false },
    ],
    applied: true,
    saved: true,
    saveError: null,
  });
  const status = await fake.invoke("engine_status");
  assert.deepEqual(details(status.channels).slice(0, 4), [
    ["Kick", "drums", false, false],
    ["Keys L", "keys", false, true],
    ["Keys R", "keys", false, false],
    ["", "other", false, false],
  ]);
  for (const musician of status.musicians) {
    assert.deepEqual(musician.pans.slice(1, 3), [0, 100], musician.address);
    assert.equal(musician.channelGains[2], musician.channelGains[1], musician.address);
  }
  const lead = status.musicians[0];
  assert.deepEqual(lead.channelGains.slice(1, 3), [70, 70], "the first side's level");
  assert.ok(lead.revision > before, "the link is a newer revision");
  assert.deepEqual(
    seen.map(([event, payload]) => [event, payload.address ?? null]),
    [["status", null], ...status.musicians.map((musician) => ["mix-changed", musician.address])],
    "the new details are announced before the pans placed for them",
  );

  // The same details again change nothing and announce nothing.
  await fake.invoke("set_channels", { channels: KEYS });
  assert.equal(seen.length, 1 + status.musicians.length);
});

test("the fake engine keeps both sides of a pair equal whoever changes one", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_channels", { channels: KEYS });

  const answer = await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [62, 30] } });
  fake.changeFromPhone(LEAD, { channelMuted: [false, false, true] });
  const row = (await fake.invoke("engine_status")).musicians[0];

  assert.deepEqual(answer.channelGains.slice(1, 3), [30, 30], "the desk moved the first side");
  assert.deepEqual(row.channelMuted.slice(1, 3), [true, true], "the phone muted the second side");
});

test("a hidden channel is heard in no mix of the fake engine, as if every musician had muted it", () => {
  const status = sampleStatus();
  const hidden = structuredClone(status);
  hidden.channels[4].hidden = true;
  const muted = structuredClone(status);
  for (const musician of muted.musicians) {
    musician.channelMuted[4] = true;
  }
  const everyHidden = structuredClone(status);
  for (const channel of everyHidden.channels) {
    channel.hidden = true;
  }

  for (const time of [500, 1234, 4321]) {
    assert.deepEqual(syntheticMeters(hidden, time), syntheticMeters(muted, time), `at ${time} ms`);
    assert.deepEqual(syntheticMeters(hidden, time).channels, syntheticMeters(status, time).channels);
    assert.ok(
      Object.values(syntheticMeters(everyHidden, time).musicians).every(
        ({ left, right }) => left === 0 && right === 0,
      ),
      `nothing is heard at ${time} ms`,
    );
  }
  const heard = Object.values(syntheticMeters(status, 1234).musicians);
  assert.ok(
    heard.some(({ left, right }) => left > 0 || right > 0),
    "with every channel shown again, the musicians hear them",
  );
});

test("hiding every channel on the fake engine silences every musician's meter", async () => {
  const fake = createFakeBridge({ running: true });
  const meters = [];
  await fake.listen("meters", (payload) => meters.push(payload));

  await fake.invoke("set_channels", {
    channels: Array.from({ length: 6 }, () => ({ hidden: true })),
  });
  fake.emitMeters();

  assert.ok(
    Object.values(meters.at(-1).musicians).every(({ left, right }) => left === 0 && right === 0),
    JSON.stringify(meters.at(-1).musicians),
  );
});

test("the fake engine refuses the channel details the desk refuses and keeps the ones it had", async () => {
  const fake = createFakeBridge({ running: true });
  const before = (await fake.invoke("engine_status")).channels;

  for (const channels of [
    [{ kind: "kazoo" }],
    [{ link: true }],
    [{ link: true }, { hidden: true }],
    [{ name: "x".repeat(33) }],
    [{ hiden: true }],
  ]) {
    await assert.rejects(fake.invoke("set_channels", { channels }), /./, JSON.stringify(channels));
  }

  assert.deepEqual((await fake.invoke("engine_status")).channels, before);
});

test("channel details set while the fake engine is stopped apply at the next start", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("stop_engine");

  const outcome = await fake.invoke("set_channels", {
    channels: [{ name: "Click", kind: "click", hidden: true }],
  });
  assert.equal(outcome.applied, false);
  assert.equal(outcome.saved, true);
  assert.equal(formatChannelsSet(outcome), "Channel details saved; they apply at the next start.");
  await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");

  assert.deepEqual(details(status.channels)[0], ["Click", "click", true, false]);
  assert.deepEqual(details(status.channels)[1], ["", "other", false, false]);
});

test("a device switch keeps the fake details whole and shows those of the inputs that come back", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_channels", {
    channels: [...KEYS, { name: "Spare", hidden: true }],
  });
  const linkedPans = (await fake.invoke("engine_status")).musicians[0].pans.slice(1, 3);

  await fake.invoke("switch_device", { device: "Microphone (USB audio)" });
  assert.deepEqual(
    details((await fake.invoke("engine_status")).channels),
    [
      ["Kick", "drums", false, false],
      ["Keys L", "keys", false, false],
    ],
    "only what the device has is shown, and a pair missing a side shows as its first input",
  );
  const answer = await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [62, 30] } });
  assert.equal(answer.channelGains[2], 30, "the pair stays linked in every mix meanwhile");
  const seen = await recordEvents(fake);

  await fake.invoke("switch_device", { device: "Demo interface" });
  const status = await fake.invoke("engine_status");
  assert.deepEqual(details(status.channels).slice(0, 4), [
    ["Kick", "drums", false, false],
    ["Keys L", "keys", false, true],
    ["Keys R", "keys", false, false],
    ["Spare", "other", true, false],
  ]);
  assert.deepEqual(status.musicians[0].pans.slice(1, 3), linkedPans, "no pan moved");
  assert.equal(
    seen.some(([event]) => event === "mix-changed"),
    false,
    "a switch moves no mix",
  );
});

test("unlistening stops the deliveries", async () => {
  const fake = createFakeBridge({ running: true });
  const payloads = [];
  const unlisten = await fake.listen("meters", (payload) => payloads.push(payload));

  fake.emit("meters", { channels: [1], musicians: {} });
  unlisten();
  fake.emit("meters", { channels: [2], musicians: {} });

  assert.deepEqual(payloads, [{ channels: [1], musicians: {} }]);
});

test("a request built from local edits leaves the fake engine with the mix the edits show", async () => {
  const fake = createFakeBridge({ running: true });
  const row = (await fake.invoke("engine_status")).musicians[0];
  const mix = mixFromDto(row);
  const edits = { "channelGains.2": 30, "pans.1": 50, "channelMuted.3": true, volumePercent: 70 };

  const answer = await fake.invoke("set_mix", {
    address: row.address,
    mix: buildMixRequest(mix, edits),
  });

  assert.deepEqual({ ...answer, revision: 0 }, { ...applyEdits(mix, edits), revision: 0 });
  assert.ok(answer.revision > mix.revision, "the change is a newer revision");
});

test("synthetic meters cover every channel and musician with whole 0-100 peaks", () => {
  const status = sampleStatus();

  const payload = syntheticMeters(status, 1234);

  assert.equal(payload.run, status.run);
  assert.equal(payload.channels.length, status.sourceChannels);
  assert.deepEqual(
    Object.keys(payload.musicians).sort(),
    status.musicians.map((musician) => musician.address).sort(),
  );
  const peaks = [
    ...payload.channels,
    ...Object.values(payload.musicians).flatMap(({ left, right }) => [left, right]),
  ];
  assert.ok(peaks.every((peak) => Number.isInteger(peak) && peak >= 0 && peak <= 100), String(peaks));
  assert.deepEqual(syntheticMeters(status, 1234), payload, "the same time gives the same levels");
});

test("the fake engine reads back its whole channel table, the inputs a device lacks included", async () => {
  const fake = createFakeBridge({ running: true });
  await fake.invoke("set_channels", { channels: [...KEYS, { name: "Spare", hidden: true }] });
  await fake.invoke("switch_device", { device: "Microphone (USB audio)" });

  assert.deepEqual(details(await fake.invoke("channel_table")), [
    ["Kick", "drums", false, false],
    ["Keys L", "keys", false, true],
    ["Keys R", "keys", false, false],
    ["Spare", "other", true, false],
  ]);

  await fake.invoke("stop_engine");
  await fake.invoke("set_channels", { channels: [{ name: "Click", kind: "click" }] });
  assert.deepEqual(details(await fake.invoke("channel_table")), [["Click", "click", false, false]]);
});
