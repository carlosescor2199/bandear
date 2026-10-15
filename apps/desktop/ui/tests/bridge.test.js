import test from "node:test";
import assert from "node:assert/strict";

import { EVENTS, createEngineApi, createTauriBridge } from "../app/lib/bridge.js";
import { createFakeBridge, sampleStatus, syntheticMeters } from "../app/lib/fake-bridge.js";
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
  ]);
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
