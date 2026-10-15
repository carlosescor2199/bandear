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

test("the fake engine answers engine_status only while it runs, as the real one does", async () => {
  const fake = createFakeBridge();

  await assert.rejects(fake.invoke("engine_status"), /not running/);
  await fake.invoke("start_engine", { config: {} });
  const status = await fake.invoke("engine_status");

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
  await assert.rejects(fake.invoke("engine_status"), /not running/);
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
  assert.deepEqual(changes, [{ address: before.address, origin: "desktop", mix: answer }]);
  const status = await fake.invoke("engine_status");
  assert.deepEqual(status.musicians[1].channelGains, answer.channelGains);
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

  assert.deepEqual(answer, applyEdits(mix, edits));
});

test("synthetic meters cover every channel and musician with whole 0-100 peaks", () => {
  const status = sampleStatus();

  const payload = syntheticMeters(status, 1234);

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
