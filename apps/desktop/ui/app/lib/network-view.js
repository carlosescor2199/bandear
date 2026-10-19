// What the Network and system screen shows and decides, worked out from the console state. Pure
// functions, no DOM. The rules are those of the first window, which they replace, kept on purpose:
//
// - ASIO loads a kernel-mode driver, so it is never chosen by default and never applied by a
//   driver change alone: the engineer picks an ASIO driver explicitly before anything starts.
// - Only WASAPI switches the input device live; any other driver restarts the engine on the
//   chosen device (ASIO holds one driver at a time, so a live switch there always fails).
// - A switch that would leave configured groups without their inputs asks first.
// - While a take records (or its files are being finished) the driver, the device and the buffer
//   stay as they are, and the setup says why: a take never mixes two devices.

import { rememberedDevice } from "./engine-config.js";
import { formatCaptureFormat, formatPercent, linkState } from "./format.js";
import { RECORDING_BLOCKS_SWITCH, isRecording, recordingOf } from "./recording.js";
import { SOUNDCHECK_BLOCKS_SWITCH, isSoundchecking, soundcheckOf } from "./soundcheck.js";

const DASH = "—";

/** The drivers, the safe one first. */
export const HOST_OPTIONS = Object.freeze([
  Object.freeze({ value: "auto", label: "Auto — the system's own (safe)" }),
  Object.freeze({ value: "wasapi", label: "WASAPI (shared)" }),
  Object.freeze({ value: "asio", label: "ASIO — kernel driver (risky)" }),
]);

/** The capture buffer sizes, in frames. */
export const BUFFER_OPTIONS = Object.freeze([
  Object.freeze({ value: 128, label: "128 frames — lowest" }),
  Object.freeze({ value: 256, label: "256 frames — balanced" }),
  Object.freeze({ value: 512, label: "512 frames" }),
  Object.freeze({ value: 1024, label: "1024 frames — safest" }),
]);

/** How long a removal must stay armed before it can be confirmed. */
export const CONFIRM_DELAY_MS = 600;

function count(value) {
  return Number(value ?? 0).toLocaleString("en-US");
}

/**
 * One row per musician: `{ address, clientId, name, editable, nameIsLocal, labelFollows,
 * connected, link, mix, sent, discarded, loss, lossHigh }`. A name the phone announced is the
 * phone's, so it is not editable; a local label is only offered while the phone announced none.
 * The desk keeps a local label for the phone when it has a client id, so the label follows it to
 * any IP (`labelFollows: "phone"`), and for the address otherwise (`"address"`). `loss` is the
 * share of packets this server discarded for that phone.
 */
export function musicianRows(state) {
  return state.musicians.map((musician) => {
    const sent = musician.packetsSent ?? 0;
    const discarded = musician.packetsDiscarded ?? 0;
    const total = sent + discarded;
    const loss = total === 0 ? 0 : (discarded / total) * 100;
    const clientId = musician.clientId ?? null;
    return {
      address: musician.address,
      clientId,
      name: musician.name ?? "",
      editable: !musician.clientName,
      nameIsLocal: Boolean(musician.nameIsLocal),
      labelFollows: clientId ? "phone" : "address",
      connected: Boolean(musician.controlConnected),
      link: linkState(musician),
      mix: `${formatPercent(musician.mix.volumePercent)} · ${musician.mix.muted ? "muted" : "live"}`,
      sent: count(sent),
      discarded: count(discarded),
      loss: `${loss.toFixed(2)}%`,
      lossHigh: loss > 0,
    };
  });
}

/** The device choices: "Volt 4 — 4 channels", or "channels unknown" when it could not be read. */
export function deviceOptions(devices) {
  return devices.map((device) => {
    let channels = "channels unknown";
    if (device.channels > 0) {
      channels = `${device.channels} ${device.channels === 1 ? "channel" : "channels"}`;
    }
    return { value: device.name, label: `${device.name} ${DASH} ${channels}` };
  });
}

// The configured groups that name a channel a device with `channels` inputs does not have.
function groupsMissingWith(status, channels) {
  return (status?.groups ?? [])
    .filter((group) => group.channels.some((channel) => channel >= channels))
    .map((group) => group.name);
}

/**
 * What picking device `name` should do: `{ kind: "none" }` for the current device or an unknown
 * one, `{ kind: "confirm", name, message }` when groups would lose inputs (a device whose channel
 * count is unknown cannot tell, so it never asks), `{ kind: "switch", name }` otherwise.
 */
export function deviceSwitchPlan(state, devices, name) {
  const status = state.engine.status;
  const device = devices.find((candidate) => candidate.name === name);
  if (!name || !device || name === status?.deviceName) {
    return { kind: "none" };
  }
  const missing = device.channels > 0 ? groupsMissingWith(status, device.channels) : [];
  if (missing.length === 0) {
    return { kind: "switch", name };
  }
  const noun = missing.length === 1 ? "group" : "groups";
  return {
    kind: "confirm",
    name,
    message:
      `Switching to ${device.name} (${device.channels} ch) makes ${missing.length} ${noun} ` +
      `unavailable: ${missing.join(", ")}. They are kept, not deleted, and become available ` +
      "again if you switch back.",
  };
}

/** "live" when the applied driver switches devices without a restart (WASAPI), else "restart". */
export function switchMethod(audio) {
  return audio.host === "wasapi" ? "live" : "restart";
}

/**
 * The audio setup: the driver shown (an ASIO choice still waiting for its driver, or the applied
 * one, which the desk remembers between runs), its devices, the device shown (the running one, or
 * with no engine the one the next start opens), the buffer and the engine's numbers. `local` is the
 * screen's own `{ draftHost, draftDevice, pendingSwitch }`. Devices listed for another driver are
 * never offered: until the shown driver's list arrives, `listing` is true and nothing is offered.
 * While a take records, `recordingLock` says why the driver, device and buffer cannot change, and
 * each is disabled (`hostDisabled`, `deviceDisabled`, `bufferDisabled`); otherwise it is null.
 */
export function audioSetupView(state, local) {
  const status = state.engine.status;
  // A take recording, or a take playing in place of the inputs, holds the input device as it is.
  const soundcheck = isSoundchecking(soundcheckOf(status));
  const recording = isRecording(recordingOf(status)) || soundcheck;
  const host = local.draftHost ?? state.audio.host;
  const listing = state.devices.host !== host;
  const devices = listing ? [] : state.devices.list;
  const asioPending = local.draftHost === "asio";
  let device = status?.deviceName ?? rememberedDevice(state.audio) ?? "";
  if (asioPending) {
    device = local.draftDevice ?? "";
  }
  const canSwitch = Boolean(status) && !status.stopped;
  return {
    host,
    asioPending,
    listing,
    devices: deviceOptions(devices),
    device,
    hostDisabled: recording,
    deviceDisabled: recording || devices.length === 0 || (!asioPending && !canSwitch),
    canApplyAsio: !recording && asioPending && Boolean(local.draftDevice),
    buffer: state.audio.bufferFrames,
    bufferDisabled: recording,
    recordingLock: soundcheck
      ? SOUNDCHECK_BLOCKS_SWITCH
      : recording
        ? RECORDING_BLOCKS_SWITCH
        : null,
    warning: local.pendingSwitch?.message ?? null,
    readouts: {
      format: status
        ? `${status.captureFormat.channels} ch · ${formatCaptureFormat(status.captureFormat)}`
        : DASH,
      sampleRate: status ? `${count(status.sampleRate)} Hz` : DASH,
      controlPort: status ? String(status.controlPort) : DASH,
      samplesReceived: status ? count(status.samplesReceived) : DASH,
      packets: status ? `${count(status.packetsSent)} / ${count(status.packetsDiscarded)}` : DASH,
    },
  };
}

const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?::(\d+))?$/;
const PORT_AFTER = /:(\d+)$/;

/**
 * Checks a phone's address before it goes to the engine: `{ ok: true, address }` or
 * `{ ok: false, message }`. Obvious mistakes (nothing typed, spaces, an IPv4 part over 255, a
 * port out of range) are caught here; anything else, such as a name that does not resolve, is the
 * engine's to refuse, with its own words.
 */
export function validateAddress(text) {
  const address = String(text ?? "").trim();
  if (address === "") {
    return { ok: false, message: "Type the phone's address, for example 192.168.1.50." };
  }
  if (/\s/.test(address)) {
    return { ok: false, message: `An address has no spaces: ${address}` };
  }
  const ipv4 = IPV4.exec(address);
  if (/^[\d.:]+$/.test(address) && !ipv4) {
    return { ok: false, message: `${address} is not an IP address such as 192.168.1.50.` };
  }
  if (ipv4 && ipv4.slice(1, 5).some((part) => Number(part) > 255)) {
    return { ok: false, message: `${address} is not an IP address: each part goes up to 255.` };
  }
  const port = PORT_AFTER.exec(address)?.[1];
  const bare = !address.startsWith("[") && address.split(":").length > 2;
  if (port !== undefined && !bare && !(Number(port) >= 1 && Number(port) <= 65535)) {
    return { ok: false, message: `The port must be from 1 to 65535, not ${port}.` };
  }
  return { ok: true, address };
}

/** True once a removal armed at `armedAt` has waited long enough to be confirmed at `now`. */
export function confirmReady(armedAt, now, delay = CONFIRM_DELAY_MS) {
  return armedAt !== null && now - armedAt >= delay;
}

/** The app window's title: the old window's, after the input device in use. */
export function windowTitle(state) {
  const device = state.engine.status?.deviceName;
  return device ? `MixLink Server — ${device}` : "MixLink Server";
}
