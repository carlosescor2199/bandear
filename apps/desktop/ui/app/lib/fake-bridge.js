// An in-memory engine behind the bridge interface (see bridge.js), for tests, the component
// gallery and a browser preview of the console. It answers the desktop commands with the same
// shapes, the same mix-merge rules and the same refusals as the real engine, and emits the same
// events. Like a Tauri command, a refused call rejects with the error text, not an Error. Every
// start, even a failed one, begins a new engine run, and every payload names its run.

import { checkChannels, defaultChannel, fitChannels, keepPairsEqual } from "./channels.js";
import { MAX_GROUPS, defaultMix, mergeMixUpdate, mixFromDto } from "./mix.js";
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
const NO_REPORTS = { ...LOST, linkHealth: "unknown" };
const NOT_CONNECTED = { ...NO_REPORTS, controlConnected: false };

const STATUS_INTERVAL_MS = 250;
const METER_INTERVAL_MS = 33;

function clone(value) {
  return structuredClone(value);
}

// The status's channels on a device with `count` inputs: each with the engineer's `details` (fitted
// to the device, see fitChannels) and its group.
function channelRows(count, groups, details = []) {
  const fitted = fitChannels(details, count);
  return Array.from({ length: count }, (_, index) => {
    // As the desktop does: an invalid group owns no channel.
    const group = groups.find(
      (candidate) => !candidate.invalid && candidate.channels.includes(index),
    );
    return {
      number: index + 1,
      ...fitted[index],
      group: group?.name ?? null,
      groupId: group?.id ?? null,
      level: 0,
    };
  });
}

// The engineer's details of a status's channels, as the desk keeps them; a status from before
// channel details has the defaults.
function channelDetails(channels) {
  return channels.map((channel) => ({
    name: channel.name ?? "",
    kind: channel.kind ?? "other",
    hidden: Boolean(channel.hidden),
    link: Boolean(channel.link),
  }));
}

const CHANNEL_FIELDS = Object.freeze(["name", "kind", "hidden", "link"]);

// The details the window sent, as the desktop reads them: every field may be left out and takes
// its default, names are trimmed, and a field the desk does not know is refused.
function channelRequests(channels) {
  return channels.map((channel) => {
    for (const field of Object.keys(channel ?? {})) {
      if (!CHANNEL_FIELDS.includes(field)) {
        throw `unknown field \`${field}\`, expected one of \`${CHANNEL_FIELDS.join("`, `")}\``;
      }
    }
    return {
      name: String(channel?.name ?? "").trim(),
      kind: channel?.kind ?? "other",
      hidden: Boolean(channel?.hidden),
      link: Boolean(channel?.link),
    };
  });
}

/** The sample engine's inputs, as the engineer named them. */
const SAMPLE_CHANNELS = Object.freeze([
  ["Kick", "drums"],
  ["Snare", "drums"],
  ["Overheads", "drums"],
  ["Bass", "bass"],
  ["Lead vox", "voice"],
  ["Backing vox", "voice"],
]);

// The status's groups for the engineer's `definitions` (1-based channels) on a device with
// `count` inputs, as the engine reports them: 0-based channels, and a group naming an input the
// device lacks kept but invalid.
function groupStatuses(definitions, count) {
  return definitions.map(({ id, name, channels }) => ({
    id,
    name,
    channels: channels.map((channel) => Math.max(channel - 1, 0)),
    invalid: channels.some((channel) => channel < 1 || channel > count),
  }));
}

// The definitions behind a status's groups.
function groupDefinitions(groups) {
  return groups.map(({ id, name, channels }) => ({
    id,
    name,
    channels: channels.map((channel) => channel + 1),
  }));
}

// What the desk refuses, as the engine's `check_groups` does: more than 16 groups, an empty or
// repeated id, an empty name, a channel in two groups.
function checkGroups(definitions) {
  if (definitions.length > MAX_GROUPS) {
    throw `at most ${MAX_GROUPS} groups are supported, received ${definitions.length}`;
  }
  const ids = new Set();
  const owners = new Map();
  for (const { id, name, channels } of definitions) {
    if (ids.has(id)) {
      throw `group id "${id}" is used by more than one group`;
    }
    ids.add(id);
    if (!name) {
      throw `group "${id}" has an empty name`;
    }
    for (const channel of channels) {
      if (owners.has(channel)) {
        throw `channel ${channel} is listed in both "${owners.get(channel)}" and "${name}"`;
      }
      owners.set(channel, name);
    }
  }
}

// A musician's group levels after the groups went from `before` to `after` (lists of ids): each
// kept group's level moves to its new place, a new group starts at 100, a deleted group's level is
// dropped, as the engine's regroup does.
function regroupedLevels(levels, before, after) {
  return levels.map((_, position) => {
    const previous = position < after.length ? before.indexOf(after[position]) : -1;
    return previous === -1 ? 100 : levels[previous];
  });
}

// `identity` is what the phone announced (`clientName`, `clientId`) and the desk's label (`name`,
// `nameIsLocal`); a musician added by address announced nothing.
function musicianRow(address, identity, link, mixChanges = {}) {
  return {
    address,
    clientName: null,
    clientId: null,
    name: null,
    nameIsLocal: false,
    ...identity,
    ...mergeMixUpdate(defaultMix(), mixChanges),
    revision: 1,
    packetsSent: 0,
    packetsDiscarded: 0,
    ...link,
  };
}

// The key the desk keeps a musician's local label under, as the desktop's session does: the
// phone's client id when it has one, so the label follows it to any IP, else the address.
function labelKey(row) {
  return row.clientId ? `client:${row.clientId}` : `address:${row.address}`;
}

/**
 * A running six-channel engine with named inputs (Kick, Snare, Overheads, Bass, Lead vox and
 * Backing vox, none hidden or linked), two groups (Drums on CH 1-3 and Vocals on CH 5-6, by ids
 * `drums` and `vocals`) and four
 * musicians whose links are good, a warning, unreported and lost. All four are phones with client
 * ids; the last announced no name and shows the desk's local label.
 */
export function sampleStatus() {
  const groups = [
    { id: "drums", name: "Drums", channels: [0, 1, 2], invalid: false },
    { id: "vocals", name: "Vocals", channels: [4, 5], invalid: false },
  ];
  const details = SAMPLE_CHANNELS.map(([name, kind]) => ({ ...defaultChannel(), name, kind }));
  return {
    run: 1,
    deviceName: "Demo interface",
    captureFormat: { channels: 6, sampleRate: 48000, sampleFormat: "I24", bufferSize: "Fixed(256)" },
    controlPort: 50001,
    sampleRate: 48000,
    sourceChannels: 6,
    groups,
    channels: channelRows(6, groups, details),
    stopped: false,
    samplesReceived: 0,
    packetsSent: 0,
    packetsDiscarded: 0,
    musicians: [
      musicianRow(
        "192.168.1.3:50000",
        {
          clientName: "Lead vocal",
          clientId: "5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90",
          name: "Lead vocal",
        },
        GOOD,
        {
          channelGains: [62, 55, 40, 58, 88, 52],
          pans: [50, 50, 50, 50, 50, 40],
          volumePercent: 85,
          maxLevelPercent: 90,
        },
      ),
      musicianRow(
        "192.168.1.12:50000",
        { clientName: "Bass", clientId: "a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64", name: "Bass" },
        WARNING,
      ),
      musicianRow(
        "192.168.1.17:50000",
        { clientName: "Keys", clientId: "c81e4f2a-9b3d-4a7e-8c6f-0d2b5e9a1f37", name: "Keys" },
        NO_REPORTS,
        { pans: [50, 50, 50, 50, 30, 70] },
      ),
      musicianRow(
        "192.168.1.20:50000",
        { clientId: "e4b7c1d9-2a6f-4d3e-9b8c-5f0a7e2d6c18", name: "Drums", nameIsLocal: true },
        LOST,
        { channelMuted: [false, false, false, true] },
      ),
    ],
  };
}

/**
 * The ASIO drivers `list_input_devices` offers for the "asio" host: named from the registry, as
 * the desktop lists them, so their channel count is unknown (0) until one is loaded.
 */
export function sampleAsioDrivers() {
  return [{ name: "Demo ASIO driver", channels: 0 }];
}

/** The input devices `list_input_devices` offers; a device with 0 channels is listed but unusable. */
export function sampleDevices() {
  return [
    { name: "Demo interface", channels: 6 },
    { name: "Microphone (USB audio)", channels: 2 },
    { name: "Digital input", channels: 0 },
  ];
}

// A slow swell with a faster flutter, different for every seed, so meters look alive. Peaks stay
// below 75 % of full scale, as a sensible gain structure would, and only now and then run hot.
function wave(seconds, seed) {
  const swell = 0.5 + 0.5 * Math.sin(seconds * (0.6 + seed * 0.13) + seed);
  const flutter = 0.5 + 0.5 * Math.sin(seconds * (7 + seed * 1.7) + seed * 2.3);
  return clampLevel(75 * swell * (0.35 + 0.65 * flutter));
}

// The peak of what `musician` hears of inputs at `levels`, as the engine mixes it: a hidden input
// is heard by no one and its solo counts for nothing; a muted input, or one left out by another's
// solo, is silent; the rest are scaled by their gain and the master and placed by their pan (the
// equal-power law), and the louder input on each side sets its peak.
function heardLevels(musician, hidden, levels) {
  const shown = levels.map((_, index) => !hidden[index]);
  const soloed = shown.some((audible, index) => audible && musician.channelSolo?.[index]);
  let left = 0;
  let right = 0;
  if (!musician.muted) {
    levels.forEach((level, index) => {
      if (!shown[index] || musician.channelMuted?.[index]) {
        return;
      }
      if (soloed && !musician.channelSolo?.[index]) {
        return;
      }
      const gain = ((musician.channelGains?.[index] ?? 100) / 100) * ((musician.volumePercent ?? 100) / 100);
      const angle = ((musician.pans?.[index] ?? 50) / 100) * (Math.PI / 2);
      left = Math.max(left, level * gain * Math.cos(angle));
      right = Math.max(right, level * gain * Math.sin(angle));
    });
  }
  // The level cap is the ceiling of what the musician is sent.
  const cap = musician.maxLevelPercent ?? 100;
  return { left: clampLevel(Math.min(left, cap)), right: clampLevel(Math.min(right, cap)) };
}

/**
 * A `meters` payload for `status` at `time` milliseconds; the same time gives the same levels.
 * Each musician's output is mixed from the inputs as the engine mixes them, so a hidden input is
 * heard in no one's meter.
 */
export function syntheticMeters(status, time) {
  const seconds = time / 1000;
  const channels = Array.from({ length: status.sourceChannels }, (_, index) => wave(seconds, index));
  const hidden = channels.map((_, index) => Boolean(status.channels?.[index]?.hidden));
  const musicians = {};
  for (const musician of status.musicians) {
    musicians[musician.address] = heardLevels(musician, hidden, channels);
  }
  return { run: status.run, channels, musicians };
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
 * is the error every start fails with (none by default); `answersFirst` delivers a `set_mix`
 * answer before its `mix-changed` event, an order the desktop's two channels allow (by default
 * the event comes first); `statusOnStart: false` starts without announcing a status, as a window
 * without an event pump. Every stored mix change takes a higher `revision`, as in the engine.
 * Local labels are kept as the desktop's session keeps them: for the phone's client id when it has
 * one, else for the address. Groups too: `set_groups` keeps them (a new group gets an id), applies
 * them while the engine runs (every musician's group levels follow their groups by id, and the new
 * groups are announced in a status before the moved mixes), answers `{ groups, applied, saved,
 * saveError }` as the desktop does, and every start that names no groups takes them. Channel
 * details too: `set_channels` keeps them, applies them while the engine runs and answers `{
 * channels, applied, saved, saveError }`; every start takes them, and the engine keeps them whole
 * while a status shows those of the inputs the device has, so a device switch shows the details of
 * inputs that come back again, as the engine does. A hidden input is heard in no musician's meter; a new
 * pair is panned apart (0 and 100) in every mix, announced in a status before the moved mixes; and
 * every change of a pair's level, mute or solo, from the desk or a phone, moves both sides (see
 * keepPairsEqual). Besides `invoke` and `listen` it offers
 * `emit(event, payload)`, `changeFromPhone(address, update)` (a phone's mix change),
 * `movePhone(from, to)` (a phone back from a new IP, its row and label moving with it),
 * `emitMeters()` (one synthetic `meters` event), `startMeters(intervalMs)` and `startDemo()`
 * (timers until the returned stop function is called), `holdAnswers(command)` and
 * `releaseAnswers()` (answers that arrive late), `failNext(command, message)` (one failed call),
 * `failStart(message)` and `calls`, every `{ command, args }` invoked.
 */
export function createFakeBridge({
  status = sampleStatus(),
  devices = sampleDevices(),
  running = false,
  failStart = null,
  answersFirst = false,
  statusOnStart = true,
} = {}) {
  const engine = clone(status);
  const listeners = new Map();
  const calls = [];
  let isRunning = running;
  let run = running ? engine.run : 0;
  let startError = failStart;
  let revision = Math.max(0, ...engine.musicians.map((musician) => musician.revision ?? 0));
  // The desk's local labels by labelKey; the labels the status already shows are kept ones.
  const labels = new Map();
  for (const row of engine.musicians) {
    if (row.nameIsLocal && row.name) {
      labels.set(labelKey(row), row.name);
    }
  }
  // The engineer's groups as the desk's session keeps them (1-based channels): every start that
  // names none takes them, and `set_groups` replaces them. `running` is what the engine has now.
  let savedGroups = groupDefinitions(engine.groups);
  let runningGroups = savedGroups;
  let lastGroupId = 0;
  // The engineer's channel details as the session keeps them (every start takes them and
  // `set_channels` replaces them), and the ones the engine has now. As in the engine, the running
  // ones are kept whole, entries past the device included: only what the status shows is fitted
  // to the inputs (see channelRows), and the mixes' pairs come from the whole table, so a pair
  // missing its second side stays linked in every mix.
  let savedChannels = channelDetails(engine.channels);
  let runningChannels = savedChannels;

  // As the desktop does: a group the window sent back keeps its id, a new one gets a fresh one,
  // and names are trimmed.
  function withIds(groups) {
    const taken = new Set(groups.map((group) => group.id).filter((id) => String(id ?? "").trim()));
    return groups.map((group) => {
      let id = group.id;
      while (!String(id ?? "").trim() || (id !== group.id && taken.has(id))) {
        lastGroupId += 1;
        id = `g${lastGroupId}`;
      }
      taken.add(id);
      return { id, name: String(group.name ?? "").trim(), channels: [...(group.channels ?? [])] };
    });
  }

  // Shows `definitions` as the running engine's groups on the current device, a group naming an
  // input the device lacks marked invalid.
  function showGroups(definitions) {
    runningGroups = definitions;
    engine.groups = groupStatuses(definitions, engine.sourceChannels);
    engine.channels = channelRows(engine.sourceChannels, engine.groups, runningChannels);
  }

  // Gives the running engine the channel `details`, kept whole and shown fitted to the device.
  function showChannels(details) {
    runningChannels = details;
    engine.channels = channelRows(engine.sourceChannels, engine.groups, runningChannels);
  }

  // As the engine does when pairs are linked: every musician's new pairs are panned apart (0 and
  // 100) and take the first side's level, mute and solo, each mix under a new revision. Returns the
  // rows that moved: every row when a pair was linked that `previous` did not have, else none. The
  // pairs are the whole table's, as the engine's mixes take them, so a device switch moves none.
  function linkMixes(previous) {
    const linked = runningChannels.map((channel, index) => channel.link && !previous[index]?.link);
    if (!linked.some(Boolean)) {
      return [];
    }
    return engine.musicians.map((row) => {
      const before = mixFromDto(row);
      const panned = mixFromDto(row);
      linked.forEach((link, first) => {
        if (link) {
          panned.pans[first] = 0;
          panned.pans[first + 1] = 100;
        }
      });
      revision += 1;
      Object.assign(row, keepPairsEqual(before, panned, runningChannels), { revision });
      return row;
    });
  }

  // Shows on `row` the label kept for its phone, else the one kept for its address. A name the
  // phone announced always wins over the desk's label.
  function showLabel(row) {
    const forPhone = row.clientId ? labels.get(`client:${row.clientId}`) : undefined;
    const local = forPhone ?? labels.get(`address:${row.address}`) ?? null;
    row.name = row.clientName ?? local;
    row.nameIsLocal = !row.clientName && local !== null;
  }

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

  // Captures from `device`; one whose channel count is unknown keeps the current count. As in the
  // engine, the groups are re-checked against the new inputs, never rewritten, and the channel
  // details are kept whole: the status shows those of the inputs the device has, so the details of
  // inputs that come back are shown again.
  function useDevice(device) {
    engine.deviceName = device.name;
    if (device.channels > 0) {
      engine.captureFormat = { ...engine.captureFormat, channels: device.channels };
      engine.sourceChannels = device.channels;
      showGroups(runningGroups);
    }
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
      const before = mixFromDto(row);
      merged = keepPairsEqual(before, mergeMixUpdate(before, update ?? {}), runningChannels);
    } catch (error) {
      throw error.message;
    }
    revision += 1;
    merged.revision = revision;
    Object.assign(row, merged);
    return merged;
  }

  const commands = {
    engine_status() {
      // As the desktop answers: no engine running is an answer, not an error.
      return isRunning ? { state: "running", ...clone(engine) } : { state: "notRunning", run };
    },
    start_engine({ config } = {}) {
      // As in the desktop, a start ends the previous engine and takes a run even when it fails.
      run += 1;
      isRunning = false;
      if (startError) {
        throw startError;
      }
      // As the desktop does: a start that names no groups takes the kept ones; one that names
      // some runs with those and keeps nothing.
      const named = config?.groups ?? [];
      runningGroups = named.length > 0 ? withIds(named) : savedGroups;
      // As the engine does: the named device, else the driver's first one when a driver is named.
      const offered = config?.host === "asio" ? sampleAsioDrivers() : devices;
      const chosen = offered.find((device) => device.name === config?.deviceFilter);
      const wanted = chosen ?? (config?.host && !config?.deviceFilter ? offered[0] : null);
      if (wanted) {
        useDevice(wanted);
      }
      // As the desktop does: every start takes the kept channel details, so a new engine pans
      // every kept pair apart before the window hears of it.
      runningChannels = savedChannels;
      showGroups(runningGroups);
      linkMixes([]);
      isRunning = true;
      engine.run = run;
      engine.stopped = false;
      if (statusOnStart) {
        emit("status", clone(engine));
      }
      return summary();
    },
    stop_engine() {
      isRunning = false;
      return null;
    },
    list_input_devices({ host } = {}) {
      return clone(host === "asio" ? sampleAsioDrivers() : devices);
    },
    switch_device({ device }) {
      requireRunning();
      const found = devices.find((candidate) => candidate.name === device);
      if (!found || found.channels === 0) {
        throw `no usable input device named "${device}"`;
      }
      useDevice(found);
      return summary();
    },
    add_target({ address }) {
      requireRunning();
      const resolved = withPort(address);
      const host = hostOf(resolved);
      if (engine.musicians.some((musician) => hostOf(musician.address) === host)) {
        throw `a musician at ${host} is already listed`;
      }
      const row = musicianRow(resolved, {}, NOT_CONNECTED);
      // A label the desk kept for this address before shows again, as in the desktop.
      showLabel(row);
      engine.musicians.push(row);
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
      // As in the desktop: a label kept for the phone replaces one kept for its address, so the
      // row shows what was just set.
      labels.delete(`address:${row.address}`);
      if (label) {
        labels.set(labelKey(row), label);
      } else {
        labels.delete(labelKey(row));
      }
      showLabel(row);
      return null;
    },
    set_mix({ address, mix }) {
      requireRunning();
      const merged = mergeInto(musicianAt(address), mix);
      if (answersFirst) {
        // The answer settles at once; the event follows in a later task.
        setTimeout(() => announceMix(address, "desktop", merged), 0);
      } else {
        announceMix(address, "desktop", merged);
      }
      return clone(merged);
    },
    set_groups({ groups } = {}) {
      const next = withIds(groups ?? []);
      checkGroups(next);
      savedGroups = next;
      // As the desktop answers: a running engine took them, or they apply at the next start.
      const outcome = { groups: clone(next), applied: isRunning, saved: true, saveError: null };
      if (isRunning) {
        const previous = JSON.stringify(engine.groups);
        const before = engine.groups.map((group) => group.id);
        showGroups(next);
        if (JSON.stringify(engine.groups) === previous) {
          // As in the engine, the same groups change nothing and announce nothing.
          return outcome;
        }
        const after = engine.groups.map((group) => group.id);
        // As the desktop does: the new groups go out first, then every mix whose levels moved.
        emit("status", clone(engine));
        const moved =
          before.length !== after.length || before.some((id, index) => id !== after[index]);
        if (moved) {
          for (const row of engine.musicians) {
            revision += 1;
            row.groupLevels = regroupedLevels(row.groupLevels, before, after);
            row.revision = revision;
            announceMix(row.address, "desktop", mixFromDto(row));
          }
        }
      }
      return outcome;
    },
    set_channels({ channels } = {}) {
      const next = channelRequests(channels ?? []);
      try {
        checkChannels(next);
      } catch (error) {
        throw error.message;
      }
      savedChannels = next;
      // As the desktop answers: a running engine took them, or they apply at the next start.
      const outcome = { channels: clone(next), applied: isRunning, saved: true, saveError: null };
      if (isRunning) {
        const previous = runningChannels;
        const shown = JSON.stringify(engine.channels);
        showChannels(next);
        // As the desktop does: the new details go out first, then every mix a new pair moved. The
        // same details, or a change only to inputs the device lacks, announce no status.
        if (JSON.stringify(engine.channels) !== shown) {
          emit("status", clone(engine));
        }
        for (const row of linkMixes(previous)) {
          announceMix(row.address, "desktop", mixFromDto(row));
        }
      }
      return outcome;
    },
  };

  // A change made on the musician's phone: merged as the engine would, announced as "phone".
  function changeFromPhone(address, update) {
    announceMix(address, "phone", mergeInto(musicianAt(address), update));
  }

  // The phone at `from` comes back from another IP with its client id, as after a DHCP change: as
  // the engine's registration does, its row (mix included) moves to `to` and any row already at
  // that IP gives way. The label kept for the phone follows it; the next status shows the move.
  function movePhone(from, to) {
    const row = musicianAt(from);
    if (!row.clientId) {
      throw new Error(`the musician at ${from} has no client id, so a new IP would be a new row`);
    }
    engine.musicians = engine.musicians.filter(
      (musician) => musician === row || hostOf(musician.address) !== hostOf(to),
    );
    row.address = to;
    showLabel(row);
    if (isRunning) {
      emit("status", clone(engine));
    }
  }

  // Commands whose answers wait for `releaseAnswers`, and the answers waiting.
  const held = new Set();
  const waiting = [];
  // Commands whose next call fails: { message, afterEffect }.
  const failing = new Map();

  const titles = [];

  return {
    calls,
    /** Every window title set, in order. */
    titles,
    setTitle(title) {
      titles.push(title);
      return Promise.resolve();
    },
    invoke(command, args) {
      calls.push({ command, args });
      // The command takes effect at once, events included; only its answer may be held back.
      const handler = commands[command];
      const failure = failing.get(command);
      if (failure) {
        failure.times -= 1;
        if (failure.times <= 0) {
          failing.delete(command);
        }
      }
      let outcome;
      if (failure) {
        if (failure.afterEffect) {
          try {
            handler?.(args ?? {});
          } catch {
            // The failure below is the answer either way.
          }
        }
        outcome = { ok: false, value: failure.message };
      } else if (handler) {
        try {
          outcome = { ok: true, value: handler(args ?? {}) };
        } catch (error) {
          outcome = { ok: false, value: error };
        }
      } else {
        outcome = { ok: false, value: `unknown command: ${command}` };
      }
      return new Promise((resolve, reject) => {
        const answer = () => (outcome.ok ? resolve(outcome.value) : reject(outcome.value));
        if (held.has(command)) {
          waiting.push(answer);
        } else {
          answer();
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
    changeFromPhone,
    movePhone,
    /** From now on the answers to `command` wait for `releaseAnswers`, as a slow link would. */
    holdAnswers(command) {
      held.add(command);
    },
    /**
     * Makes the next call (or `times` calls) of `command` fail with `message`: before it does
     * anything, or, with `afterEffect`, after it took effect, as a command whose answer is an
     * error but whose work was done.
     */
    failNext(command, message, { afterEffect = false, times = 1 } = {}) {
      failing.set(command, { message, afterEffect, times });
    },
    /** Delivers every answer waiting now, in the order the commands were invoked. */
    releaseAnswers() {
      for (const answer of waiting.splice(0)) {
        answer();
      }
    },
    /** Emits synthetic `meters` while the engine runs; returns the function that stops them. */
    startMeters(intervalMs = METER_INTERVAL_MS) {
      const timer = setInterval(emitMeters, intervalMs);
      return () => clearInterval(timer);
    },
    /**
     * Plays a live engine for a browser preview: meters about 30 times a second, a status 4 times
     * a second, and now and then a fader moved on a phone that is reporting. Returns the function
     * that stops it.
     */
    startDemo({ phoneChangeEveryMs = 4000 } = {}) {
      let step = 0;
      const timers = [
        setInterval(emitMeters, METER_INTERVAL_MS),
        setInterval(() => {
          if (isRunning) {
            emit("status", clone(engine));
          }
        }, STATUS_INTERVAL_MS),
        setInterval(() => {
          const phones = engine.musicians.filter(
            (musician) => musician.controlConnected && typeof musician.lossPercent === "number",
          );
          if (!isRunning || phones.length === 0) {
            return;
          }
          const row = phones[step % phones.length];
          const channel = step % engine.sourceChannels;
          step += 1;
          const gains = row.channelGains.slice(0, channel + 1);
          gains[channel] = (gains[channel] + 37) % 101;
          changeFromPhone(row.address, { channelGains: gains });
        }, phoneChangeEveryMs),
      ];
      return () => timers.forEach((timer) => clearInterval(timer));
    },
    /** Makes every later start fail with `message`, or succeed again with `null`. */
    failStart(message) {
      startError = message;
    },
  };
}
