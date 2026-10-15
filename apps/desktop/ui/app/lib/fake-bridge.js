// An in-memory engine behind the bridge interface (see bridge.js), for tests, the component
// gallery and a browser preview of the console. It answers the desktop commands with the same
// shapes, the same mix-merge rules and the same refusals as the real engine, and emits the same
// events. Like a Tauri command, a refused call rejects with the error text, not an Error. Every
// start, even a failed one, begins a new engine run, and every payload names its run.

import { defaultMix, mergeMixUpdate, mixFromDto } from "./mix.js";
import { clampLevel } from "./values.js";

const DEFAULT_PORT = 50000;

const GOOD = {
  controlConnected: true,
  linkHealth: "good",
  packetsPerSecond: 200,
  underrunsPerSecond: 0,
  lossPercent: 0.01,
  receiveTimeoutsPerSecond: 0,
};
const WARNING = {
  controlConnected: true,
  linkHealth: "warning",
  packetsPerSecond: 196,
  underrunsPerSecond: 2,
  lossPercent: 0.82,
  receiveTimeoutsPerSecond: 0.5,
};
const LOST = {
  controlConnected: true,
  linkHealth: "bad",
  packetsPerSecond: null,
  underrunsPerSecond: null,
  lossPercent: null,
  receiveTimeoutsPerSecond: null,
};
const NOT_CONNECTED = { ...LOST, controlConnected: false, linkHealth: "unknown" };

function clone(value) {
  return structuredClone(value);
}

function channelRows(count, groups) {
  return Array.from({ length: count }, (_, index) => ({
    number: index + 1,
    group: groups.find((group) => !group.invalid && group.channels.includes(index))?.name ?? null,
    level: 0,
  }));
}

function musicianRow(address, names, link, mixChanges = {}) {
  return {
    address,
    clientName: null,
    name: null,
    nameIsLocal: false,
    ...names,
    ...mergeMixUpdate(defaultMix(), mixChanges),
    packetsSent: 0,
    packetsDiscarded: 0,
    ...link,
  };
}

/** A running four-channel engine with three musicians: a good link, a warning and a lost one. */
export function sampleStatus() {
  const groups = [
    { name: "Drums", channels: [0, 1], invalid: false },
    { name: "Vocals", channels: [3], invalid: false },
  ];
  return {
    run: 1,
    deviceName: "Volt 4",
    captureFormat: { channels: 4, sampleRate: 48000, sampleFormat: "I24", bufferSize: "Fixed(256)" },
    controlPort: 50001,
    sampleRate: 48000,
    sourceChannels: 4,
    groups,
    channels: channelRows(4, groups),
    stopped: false,
    samplesReceived: 0,
    packetsSent: 0,
    packetsDiscarded: 0,
    musicians: [
      musicianRow("192.168.1.3:50000", { clientName: "Lead vocal", name: "Lead vocal" }, GOOD, {
        channelGains: [62, 55, 40, 88],
        pans: [50, 50, 65, 40],
      }),
      musicianRow("192.168.1.12:50000", { clientName: "Bass", name: "Bass" }, WARNING),
      musicianRow("192.168.1.20:50000", { name: "Drums", nameIsLocal: true }, LOST, {
        channelMuted: [false, false, false, true],
      }),
    ],
  };
}

/** The input devices `list_input_devices` offers; a device with 0 channels is listed but unusable. */
export function sampleDevices() {
  return [
    { name: "Volt 4", channels: 4 },
    { name: "Microphone (USB audio)", channels: 2 },
    { name: "Digital input", channels: 0 },
  ];
}

// A slow swell with a faster flutter, different for every seed, so meters look alive.
function wave(seconds, seed) {
  const swell = 0.5 + 0.5 * Math.sin(seconds * (0.6 + seed * 0.13) + seed);
  const flutter = 0.5 + 0.5 * Math.sin(seconds * (7 + seed * 1.7) + seed * 2.3);
  return clampLevel(100 * swell * (0.35 + 0.65 * flutter));
}

/** A `meters` payload for `status` at `time` milliseconds; the same time gives the same levels. */
export function syntheticMeters(status, time) {
  const seconds = time / 1000;
  const musicians = {};
  status.musicians.forEach((musician, index) => {
    musicians[musician.address] = {
      left: wave(seconds, 10 + index * 2),
      right: wave(seconds, 11 + index * 2),
    };
  });
  return {
    run: status.run,
    channels: Array.from({ length: status.sourceChannels }, (_, index) => wave(seconds, index)),
    musicians,
  };
}

function hostOf(address) {
  if (address.startsWith("[")) {
    return address.slice(1, address.indexOf("]"));
  }
  const colon = address.lastIndexOf(":");
  return colon === -1 ? address : address.slice(0, colon);
}

function withPort(address) {
  const text = String(address).trim();
  const hasPort = text.startsWith("[") ? text.includes("]:") : text.includes(":");
  if (!text || text.startsWith(":")) {
    throw `invalid musician address "${address}"`;
  }
  return hasPort ? text : `${text}:${DEFAULT_PORT}`;
}

/**
 * The fake bridge. `running` starts the engine already running, as run `status.run`; `failStart`
 * is the error every start fails with (none by default). Besides `invoke` and `listen` it offers
 * `emit(event, payload)`, `changeFromPhone(address, update)` (a phone's mix change),
 * `emitMeters()` (one synthetic `meters` event), `startMeters(intervalMs)` (synthetic meters until
 * the returned stop function is called), `failStart(message)` and `calls`, every
 * `{ command, args }` invoked.
 */
export function createFakeBridge({
  status = sampleStatus(),
  devices = sampleDevices(),
  running = false,
  failStart = null,
} = {}) {
  const engine = clone(status);
  const listeners = new Map();
  const calls = [];
  let isRunning = running;
  let run = running ? engine.run : 0;
  let startError = failStart;

  function emit(event, payload) {
    for (const handler of [...(listeners.get(event) ?? [])]) {
      handler(payload);
    }
  }

  function requireRunning() {
    if (!isRunning) {
      throw "the engine is not running";
    }
  }

  function musicianAt(address) {
    const row = engine.musicians.find((musician) => musician.address === address);
    if (!row) {
      throw `no musician at ${address}`;
    }
    return row;
  }

  function summary() {
    return { run, deviceName: engine.deviceName, captureFormat: clone(engine.captureFormat) };
  }

  function announceMix(address, origin, mix) {
    emit("mix-changed", { run, address, origin, mix: clone(mix) });
  }

  function emitMeters() {
    if (isRunning) {
      emit("meters", syntheticMeters(engine, performance.now()));
    }
  }

  function mergeInto(row, update) {
    let merged;
    try {
      merged = mergeMixUpdate(mixFromDto(row), update ?? {});
    } catch (error) {
      throw error.message;
    }
    Object.assign(row, merged);
    return merged;
  }

  const commands = {
    engine_status() {
      requireRunning();
      return clone(engine);
    },
    start_engine() {
      // As in the desktop, a start ends the previous engine and takes a run even when it fails.
      run += 1;
      isRunning = false;
      if (startError) {
        throw startError;
      }
      isRunning = true;
      engine.run = run;
      engine.stopped = false;
      emit("status", clone(engine));
      return summary();
    },
    stop_engine() {
      isRunning = false;
      return null;
    },
    list_input_devices() {
      return clone(devices);
    },
    switch_device({ device }) {
      requireRunning();
      const found = devices.find((candidate) => candidate.name === device);
      if (!found || found.channels === 0) {
        throw `no usable input device named "${device}"`;
      }
      engine.deviceName = found.name;
      engine.captureFormat = { ...engine.captureFormat, channels: found.channels };
      engine.sourceChannels = found.channels;
      engine.channels = channelRows(found.channels, engine.groups);
      return summary();
    },
    add_target({ address }) {
      requireRunning();
      const resolved = withPort(address);
      const host = hostOf(resolved);
      if (engine.musicians.some((musician) => hostOf(musician.address) === host)) {
        throw `a musician at ${host} is already listed`;
      }
      engine.musicians.push(musicianRow(resolved, {}, NOT_CONNECTED));
      return resolved;
    },
    remove_target({ address }) {
      requireRunning();
      musicianAt(address);
      engine.musicians = engine.musicians.filter((musician) => musician.address !== address);
      return address;
    },
    set_musician_name({ address, name }) {
      const row = musicianAt(address);
      const label = String(name ?? "").trim();
      // A name the phone announced always wins over the desk's label.
      if (!row.clientName) {
        row.name = label || null;
        row.nameIsLocal = Boolean(label);
      }
      return null;
    },
    set_mix({ address, mix }) {
      requireRunning();
      const merged = mergeInto(musicianAt(address), mix);
      announceMix(address, "desktop", merged);
      return clone(merged);
    },
  };

  return {
    calls,
    invoke(command, args) {
      calls.push({ command, args });
      return new Promise((resolve, reject) => {
        const handler = commands[command];
        if (!handler) {
          reject(`unknown command: ${command}`);
          return;
        }
        try {
          resolve(handler(args ?? {}));
        } catch (error) {
          reject(error);
        }
      });
    },
    listen(event, handler) {
      if (!listeners.has(event)) {
        listeners.set(event, new Set());
      }
      listeners.get(event).add(handler);
      return Promise.resolve(() => {
        listeners.get(event).delete(handler);
      });
    },
    emit,
    emitMeters,
    /** A change made on the musician's phone: merged as the engine would, announced as "phone". */
    changeFromPhone(address, update) {
      const merged = mergeInto(musicianAt(address), update);
      announceMix(address, "phone", merged);
    },
    /** Emits synthetic `meters` while the engine runs; returns the function that stops them. */
    startMeters(intervalMs = 33) {
      const timer = setInterval(emitMeters, intervalMs);
      return () => clearInterval(timer);
    },
    /** Makes every later start fail with `message`, or succeed again with `null`. */
    failStart(message) {
      startError = message;
    },
  };
}
