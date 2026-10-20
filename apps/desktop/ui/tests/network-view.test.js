import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleDevices, sampleStatus } from "../app/lib/fake-bridge.js";
import {
  BUFFER_OPTIONS,
  CONFIRM_DELAY_MS,
  HOST_OPTIONS,
  audioSetupView,
  confirmReady,
  deviceOptions,
  deviceSwitchPlan,
  musicianRows,
  switchMethod,
  validateAddress,
  windowTitle,
} from "../app/lib/network-view.js";
import { IDLE_RECORDING, RECORDING_BLOCKS_SWITCH } from "../app/lib/recording.js";

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

const running = reduce([actions.status(sampleStatus())]);
const NO_DRAFT = { draftHost: null, draftDevice: null, pendingSwitch: null };

test("a row per musician with the name, who owns it, the link and the counts", () => {
  const status = sampleStatus();
  status.musicians[0].packetsSent = 1200;
  status.musicians[0].packetsDiscarded = 30;
  const rows = musicianRows(reduce([actions.status(status)]));

  assert.equal(rows.length, 4);
  assert.deepEqual(rows[0], {
    address: "192.168.1.3:50000",
    clientId: status.musicians[0].clientId,
    name: "Lead vocal",
    editable: false,
    nameIsLocal: false,
    labelFollows: "phone",
    connected: true,
    link: {
      level: "good",
      label: "Good",
      detail: "Loss 0.01% · Glitches 0.0/s · Timeouts 0.0/s",
    },
    mix: "85% · live",
    sent: "1,200",
    discarded: "30",
    loss: "2.44%",
    lossHigh: true,
    buffer: {
      key: `client:${status.musicians[0].clientId}`,
      supported: true,
      mode: "auto",
      ms: null,
      playing: null,
      note: null,
    },
    redundancy: {
      key: `client:${status.musicians[0].clientId}`,
      on: false,
      delayBlocks: null,
      holdSupported: false,
      delays: [
        { value: 0, label: "Copy: immediate", disabled: false },
        ...[1, 2, 3, 4].map((value) => ({
          value,
          label: `Copy: ${value} ${value === 1 ? "block" : "blocks"} later (+${((value * 256) / 48).toFixed(1)} ms), needs a newer app`,
          disabled: true,
        })),
      ],
      holdNote: "Delayed copies: needs a newer app",
      bandwidth: "Sends once; on doubles this musician's bandwidth",
      recovered: null,
      duplicates: null,
    },
  });
  assert.equal(rows[1].redundancy.holdSupported, true, "Bass's phone holds packets back for copies");
  assert.equal(rows[1].buffer.note, "needs a newer app", "Bass's phone takes no buffer");
  assert.equal(rows[3].name, "Drums");
  assert.equal(rows[3].editable, true, "no announced name: the desk may label it");
  assert.equal(rows[3].nameIsLocal, true);
  assert.equal(rows[1].loss, "0.00%");
  assert.equal(rows[1].lossHigh, false);
});

test("a label follows a phone that has a client id and stays with the address otherwise", () => {
  const status = sampleStatus();
  status.musicians[1].clientId = null;
  delete status.musicians[2].clientId;
  const rows = musicianRows(reduce([actions.status(status)]));

  assert.equal(rows[3].clientId, status.musicians[3].clientId);
  assert.equal(rows[3].labelFollows, "phone");
  assert.equal(rows[1].clientId, null, "a configured target or an older phone");
  assert.equal(rows[1].labelFollows, "address");
  assert.equal(rows[2].clientId, null, "a status without the field reads as no id");
  assert.equal(rows[2].labelFollows, "address");
});

test("a muted musician's mix reads muted", () => {
  const status = sampleStatus();
  status.musicians[1].muted = true;

  assert.equal(musicianRows(reduce([actions.status(status)]))[1].mix, "100% · muted");
});

test("the driver choices put the safe one first and say what ASIO risks", () => {
  assert.deepEqual(
    HOST_OPTIONS.map(({ value }) => value),
    ["auto", "wasapi", "asio", "demo"],
    "the demo source last, never in the way of an interface",
  );
  assert.match(HOST_OPTIONS[0].label, /safe/);
  assert.match(HOST_OPTIONS[2].label, /risky/);
  assert.deepEqual(
    BUFFER_OPTIONS.map(({ value }) => value),
    [128, 256, 512, 1024],
  );
});

test("devices are listed with their channel count, or as unknown when it could not be read", () => {
  assert.deepEqual(
    deviceOptions([
      { name: "Volt 4", channels: 4 },
      { name: "Mic", channels: 1 },
      { name: "ASIO driver", channels: 0 },
    ]).map(({ label }) => label),
    ["Volt 4 — 4 channels", "Mic — 1 channel", "ASIO driver — channels unknown"],
  );
});

test("a device switch that would leave groups without inputs asks first", () => {
  const devices = sampleDevices();

  assert.deepEqual(deviceSwitchPlan(running, devices, "Demo interface"), { kind: "none" });
  assert.deepEqual(deviceSwitchPlan(running, devices, "Nowhere"), { kind: "none" });
  assert.deepEqual(deviceSwitchPlan(running, devices, "Digital input"), {
    kind: "switch",
    name: "Digital input",
  });

  const plan = deviceSwitchPlan(running, devices, "Microphone (USB audio)");
  assert.equal(plan.kind, "confirm");
  assert.equal(plan.name, "Microphone (USB audio)");
  assert.equal(
    plan.message,
    "Switching to Microphone (USB audio) (2 ch) makes 2 groups unavailable: Drums, Vocals. " +
      "They are kept, not deleted, and become available again if you switch back.",
  );
});

test("only WASAPI switches a device live; every other driver restarts with the chosen device", () => {
  assert.equal(switchMethod({ host: "wasapi" }), "live");
  assert.equal(switchMethod({ host: "auto" }), "restart");
  assert.equal(switchMethod({ host: "asio" }), "restart");
});

function withDevices(state, host, list) {
  return reduce([actions.devices(host, list)], state);
}

test("the audio setup shows the applied choices, the device and the engine's numbers", () => {
  const view = audioSetupView(withDevices(running, "auto", sampleDevices()), NO_DRAFT);

  assert.equal(view.host, "auto");
  assert.equal(view.asioPending, false);
  assert.equal(view.device, "Demo interface");
  assert.equal(view.deviceDisabled, false);
  assert.equal(view.buffer, 256);
  assert.equal(view.canApplyAsio, false);
  assert.deepEqual(view.readouts, {
    format: "6 ch · 48 kHz · 24-bit · 256",
    sampleRate: "48,000 Hz",
    controlPort: "50001",
    samplesReceived: "0",
    packets: "0 / 0",
  });
});

test("choosing ASIO waits for an explicit driver before anything starts", () => {
  const asioDrivers = withDevices(running, "asio", [{ name: "Volt ASIO", channels: 0 }]);
  const drafting = audioSetupView(asioDrivers, { ...NO_DRAFT, draftHost: "asio" });
  assert.equal(drafting.host, "asio");
  assert.equal(drafting.asioPending, true);
  assert.equal(drafting.device, "", "no driver is picked for the engineer");
  assert.equal(drafting.canApplyAsio, false);

  const picked = audioSetupView(asioDrivers, {
    ...NO_DRAFT,
    draftHost: "asio",
    draftDevice: "Volt ASIO",
  });
  assert.equal(picked.device, "Volt ASIO");
  assert.equal(picked.canApplyAsio, true);
});

test("devices listed for another driver are never offered under the one shown", () => {
  const stale = withDevices(running, "auto", sampleDevices());

  const view = audioSetupView(stale, { ...NO_DRAFT, draftHost: "asio" });

  assert.deepEqual(view.devices, []);
  assert.equal(view.listing, true);
  assert.equal(view.deviceDisabled, true);
});

test("with no engine the device cannot be switched, and the numbers are blank", () => {
  const view = audioSetupView(withDevices(initialState, "auto", sampleDevices()), NO_DRAFT);

  assert.equal(view.deviceDisabled, true);
  assert.equal(view.readouts.format, "—");
  assert.equal(
    audioSetupView(withDevices(running, "auto", []), NO_DRAFT).deviceDisabled,
    true,
    "nothing to pick",
  );
});

test("while a take records, the driver, device and buffer are locked and the setup says why", () => {
  const free = audioSetupView(withDevices(running, "auto", sampleDevices()), NO_DRAFT);
  assert.equal(free.recordingLock, null);
  assert.equal(free.hostDisabled, false);
  assert.equal(free.bufferDisabled, false);

  const status = { ...sampleStatus(), recording: { ...IDLE_RECORDING, state: "recording" } };
  const recording = reduce([actions.status(status), actions.devices("auto", sampleDevices())]);
  const view = audioSetupView(recording, NO_DRAFT);
  assert.equal(view.recordingLock, RECORDING_BLOCKS_SWITCH);
  assert.equal(view.hostDisabled, true);
  assert.equal(view.deviceDisabled, true);
  assert.equal(view.bufferDisabled, true);

  const stopping = reduce([
    actions.status({ ...status, recording: { ...IDLE_RECORDING, state: "stopping" } }),
  ]);
  assert.equal(audioSetupView(stopping, NO_DRAFT).recordingLock, RECORDING_BLOCKS_SWITCH);
});

test("the audio selects show the remembered choice, and the running device while one runs", () => {
  const volt = { host: "wasapi", device: "Microphone (USB audio)", bufferFrames: 512 };
  const idle = reduce([actions.audio(volt), actions.devices("wasapi", sampleDevices())]);

  const view = audioSetupView(idle, NO_DRAFT);
  assert.equal(view.host, "wasapi");
  assert.equal(view.device, "Microphone (USB audio)", "the input the next start opens");
  assert.equal(view.buffer, 512);

  const asio = reduce([
    actions.audio({ host: "asio", asioDriver: "Volt ASIO" }),
    actions.devices("asio", [{ name: "Volt ASIO", channels: 0 }]),
  ]);
  assert.equal(audioSetupView(asio, NO_DRAFT).device, "Volt ASIO", "under ASIO, its driver");

  const runningOnDefault = reduce([actions.audio(volt), actions.status(sampleStatus())]);
  assert.equal(audioSetupView(runningOnDefault, NO_DRAFT).device, "Demo interface");
});

test("an address is checked before it is sent, with a sentence that says what is wrong", () => {
  assert.deepEqual(validateAddress(" 192.168.1.50 "), { ok: true, address: "192.168.1.50" });
  assert.deepEqual(validateAddress("192.168.1.50:50000"), {
    ok: true,
    address: "192.168.1.50:50000",
  });
  assert.deepEqual(validateAddress("[fe80::1]:50000"), { ok: true, address: "[fe80::1]:50000" });
  assert.deepEqual(validateAddress("stage-rack.local"), {
    ok: true,
    address: "stage-rack.local",
  });

  assert.match(validateAddress("").message, /Type the phone's address/);
  assert.match(validateAddress("192.168.1 50").message, /no spaces/);
  assert.match(validateAddress("192.168.1.300").message, /192\.168\.1\.300 is not an IP address/);
  assert.match(validateAddress("192.168.1.5:0").message, /port.*1 to 65535/);
  assert.match(validateAddress("192.168.1.5:70000").message, /port.*1 to 65535/);
});

test("a removal can only be confirmed after a pause, so a double click cannot remove", () => {
  assert.equal(confirmReady(1000, 1000), false);
  assert.equal(confirmReady(1000, 1000 + CONFIRM_DELAY_MS - 1), false);
  assert.equal(confirmReady(1000, 1000 + CONFIRM_DELAY_MS), true);
  assert.equal(confirmReady(null, 5000), false, "nothing armed");
});

test("the window is titled after the input device, as before", () => {
  assert.equal(windowTitle(running), "MixLink Server — Demo interface");
  assert.equal(windowTitle(initialState), "MixLink Server");
});
