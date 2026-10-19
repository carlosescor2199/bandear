// An in-memory engine behind the bridge interface (see bridge.js), for tests, the component
// gallery and a browser preview of the console. It answers the desktop commands with the same
// shapes, the same mix-merge rules and the same refusals as the real engine, and emits the same
// events. Like a Tauri command, a refused call rejects with the error text, not an Error. Every
// start, even a failed one, begins a new engine run, and every payload names its run.

import {
  CHANNEL_KINDS,
  checkChannels,
  defaultChannel,
  fitChannels,
  keepLocked,
  keepPairsEqual,
  lockPairs,
} from "./channels.js";
import { HOSTS } from "./engine-config.js";
import { MAX_GROUPS, defaultMix, mergeEngineerUpdate, mergeMixUpdate, mixFromDto } from "./mix.js";
import { checkPresetName, presetOf, sameShared, withPreset, withShared } from "./mixes.js";
import { IDLE_RECORDING, RECORD_FORMATS } from "./recording.js";
import { IDLE_SOUNDCHECK } from "./soundcheck.js";
import { BACKUP_MAX_WAIT_MS, BACKUP_QUIET_MS, KEPT_BACKUPS } from "./mix-backups-view.js";
import { checkSessionName } from "./sessions.js";
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

/** The outputs the fake's ASIO driver has: under ASIO it offers only these, its own. */
const ASIO_OUTPUTS = 4;

/** The fake desk's default recordings folder: the Music folder + MixLink\Recordings. */
export const FAKE_RECORDINGS_FOLDER = "C:\\Users\\Demo\\Music\\MixLink\\Recordings";

/** The fake desk's own settings folder, which no recordings folder may be in. */
export const FAKE_CONFIG_FOLDER = "C:\\Users\\Demo\\AppData\\Roaming\\MixLink";

/** The rate every take is recorded at. */
const RECORD_RATE = 48000;

const RECORDING_FIELDS = Object.freeze(["folder", "format", "includeLocal", "armed"]);

function clone(value) {
  return structuredClone(value);
}

/** The status's local output while none plays, the default. */
function offOutput() {
  return {
    device: null,
    firstChannel: null,
    state: "off",
    reason: null,
    underrunFrames: 0,
    droppedFrames: 0,
    notice: null,
  };
}

/** The status's local output playing on `device` from `firstChannel`. */
function playingOutput(device, firstChannel) {
  return { ...offOutput(), device, firstChannel, state: "running" };
}

/** An output in the desk's words, as the desktop's notices write it: "Volt 4 · outputs 3/4". */
function outputWords({ device, firstChannel }) {
  return `${device} · outputs ${firstChannel + 1}/${firstChannel + 2}`;
}

const OUTPUT_FIELDS = Object.freeze(["device", "firstChannel"]);

// The status's channels on a device with `count` inputs: each with the engineer's `details` (fitted
// to the device, see fitChannels; a kind this build does not know shown as it applies, `other`)
// and its group.
function channelRows(count, groups, details = []) {
  const fitted = fitChannels(details, count);
  return Array.from({ length: count }, (_, index) => {
    // As the desktop does: an invalid group owns no channel.
    const group = groups.find(
      (candidate) => !candidate.invalid && candidate.channels.includes(index),
    );
    const kind = CHANNEL_KINDS.includes(fitted[index].kind) ? fitted[index].kind : "other";
    return {
      number: index + 1,
      ...fitted[index],
      kind,
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
    capabilities: [],
    rttMs: null,
    rttLastMs: null,
    linkLatest: null,
    buffer: { mode: "auto" },
    redundancy: { on: false },
    duplicatesSent: 0,
    recovered: 0,
    ...identity,
    bufferSupported: (identity.capabilities ?? []).includes("playback.buffer"),
    redundancySupported: (identity.capabilities ?? []).includes("redundancy.recovered"),
    redundancyHoldSupported: (identity.capabilities ?? []).includes("redundancy.hold"),
    ...mergeMixUpdate(defaultMix(), mixChanges),
    revision: 1,
    packetsSent: 0,
    packetsDiscarded: 0,
    ...link,
    mirror: null,
  };
}

// The change a loaded session's `saved` mix makes to `row`'s, as the desktop pushes it: each saved
// input from CH 1 on, every live group's level by its id (`groups`, a status's; a group the saved
// mix does not name keeps its level), the master volume and mute; only the master for a musician
// whose mix `follows` another's. Never the phone's cap or the engineer's controls.
function savedUpdate(saved, groups, row, follows) {
  const master = { volumePercent: saved.volumePercent ?? 100, muted: Boolean(saved.muted) };
  if (follows) {
    return master;
  }
  const channels = (saved.channels ?? []).slice(0, row.channelGains.length);
  return {
    ...master,
    channelGains: channels.map((channel) => channel.gain ?? 100),
    pans: channels.map((channel) => channel.pan ?? 50),
    channelMuted: channels.map((channel) => Boolean(channel.muted)),
    channelSolo: channels.map((channel) => Boolean(channel.solo)),
    groupLevels: groups.map((group, position) =>
      Object.hasOwn(saved.groups ?? {}, group.id) ? saved.groups[group.id] : row.groupLevels[position],
    ),
  };
}

// The key the desk keeps a musician's local label under, as the desktop's session does: the
// phone's client id when it has one, so the label follows it to any IP, else the address.
function labelKey(row) {
  return row.clientId ? `client:${row.clientId}` : `address:${row.address}`;
}

// The keys a musician is known by, the strongest first: their phone's, then their address's.
function keysOf(row) {
  return row.clientId ? [`client:${row.clientId}`, `address:${row.address}`] : [`address:${row.address}`];
}

// Whether the desk keys `one` and `other` name one musician as the desktop compares them with no
// engine: two phones by client id, two addresses by host (the engine keeps one musician per IP);
// a phone and an address are told apart.
function sameMusician(one, other) {
  const kind = (key) => key.slice(0, key.indexOf(":"));
  const value = (key) => key.slice(key.indexOf(":") + 1);
  if (kind(one) !== kind(other)) {
    return false;
  }
  return kind(one) === "client"
    ? value(one) === value(other)
    : hostOf(value(one)) === hostOf(value(other));
}

// Adds to `known` every key of each of `groups` (keys naming one musician) that shares a musician
// with it (sameMusician), until no group adds more.
function widenKeys(known, groups) {
  let before;
  do {
    before = known.length;
    for (const group of groups) {
      if (group.some((key) => known.some((own) => sameMusician(key, own)))) {
        for (const key of group) {
          if (!known.includes(key)) {
            known.push(key);
          }
        }
      }
    }
  } while (known.length !== before);
}

/**
 * True when `text` reads as the desktop reads a musician address (Rust's `SocketAddr`): an IPv4
 * address and a port, or a bracketed IPv6 address and a port. A host name and port is not one.
 */
export function isSocketAddress(text) {
  const value = String(text ?? "");
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3}):(\d{1,5})$/.exec(value);
  if (v4) {
    const octets = v4.slice(1, 5);
    return (
      octets.every((octet) => Number(octet) <= 255 && !/^0\d/.test(octet)) &&
      Number(v4[5]) <= 65535
    );
  }
  const v6 = /^\[([0-9a-fA-F:.]+)(%[^\]]+)?\]:(\d{1,5})$/.exec(value);
  return Boolean(v6) && v6[1].includes(":") && Number(v6[3]) <= 65535;
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
          capabilities: ["stats.v2", "playback.buffer"],
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
        {
          clientName: "Bass",
          clientId: "a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64",
          name: "Bass",
          capabilities: ["stats.v2", "redundancy.recovered", "redundancy.hold"],
        },
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
    // The engineer's local mix, the engine's default, and no local output, the default.
    localMix: { ...defaultMix(), revision: 1 },
    localOutput: offOutput(),
    // No take records, and none has ended yet.
    recording: clone(IDLE_RECORDING),
    lastTake: null,
    // The inputs are live: no soundcheck plays.
    soundcheck: clone(IDLE_SOUNDCHECK),
  };
}

/**
 * The ASIO drivers `list_input_devices` offers for the "asio" host: named from the registry, as
 * the desktop lists them, so their channel count is unknown (0) until one is loaded.
 */
export function sampleAsioDrivers() {
  return [{ name: "Demo ASIO driver", channels: 0 }];
}

/**
 * The output devices `output_devices` offers under the system's driver (auto or WASAPI): a
 * four-output interface and the computer's own speakers.
 */
export function sampleOutputs() {
  return [
    { name: "Demo interface", channels: 4 },
    { name: "Speakers (Realtek Audio)", channels: 2 },
  ];
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
  // The ceiling of what the musician is sent: the lower of the phone's level cap and the
  // engineer's limit.
  const cap = Math.min(musician.maxLevelPercent ?? 100, musician.engineerLimitPercent ?? 100);
  return { left: clampLevel(Math.min(left, cap)), right: clampLevel(Math.min(right, cap)) };
}

/**
 * A `meters` payload for `status` at `time` milliseconds; the same time gives the same levels.
 * Each musician's output, and the engineer's local mix (`local`), is mixed from the inputs as the
 * engine mixes them, so a hidden input is heard in no one's meter.
 */
export function syntheticMeters(status, time) {
  const seconds = time / 1000;
  const channels = Array.from({ length: status.sourceChannels }, (_, index) => wave(seconds, index));
  const hidden = channels.map((_, index) => Boolean(status.channels?.[index]?.hidden));
  const musicians = {};
  for (const musician of status.musicians) {
    musicians[musician.address] = heardLevels(musician, hidden, channels);
  }
  const local = status.localMix
    ? heardLevels(status.localMix, hidden, channels)
    : { left: 0, right: 0 };
  return { run: status.run, channels, musicians, local };
}

/** How many one-second samples a link history keeps, as the desktop's does: ten minutes. */
export const LINK_HISTORY_SAMPLES = 600;

/** The time one link history sample stands for, in milliseconds. */
export const LINK_SAMPLE_PERIOD_MS = 1000;

// A stable number in [0, 1) for `text` and `index`: the same musician and second give the same one.
function noise(text, index) {
  let hash = (2166136261 ^ index) >>> 0;
  for (const char of String(text)) {
    hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  }
  hash = Math.imul(hash ^ (hash >>> 15), 2246822507);
  hash ^= hash >>> 13;
  return (hash >>> 0) / 4294967296;
}

// How each fake link behaves, by the row's level: the mean loss in percent, an underrun every so
// many seconds (0 for none), the round trip and jitter ranges in milliseconds, the phone's
// cushion, the second of every ten minutes it reconnected at (null for never) and how many of the newest seconds
// are gaps (its reports stopped). A phone that never reported only has a round trip.
const LINK_PROFILES = Object.freeze({
  good: {
    loss: 0.05,
    underrunEvery: 0,
    rtt: [3, 6],
    jitter: [0.5, 2],
    cushion: 40,
    reconnectedAt: null,
    silentFor: 0,
  },
  warning: {
    loss: 1.2,
    underrunEvery: 9,
    rtt: [8, 25],
    jitter: [3, 9],
    cushion: 60,
    reconnectedAt: 360,
    silentFor: 0,
  },
  bad: {
    loss: 0.4,
    underrunEvery: 0,
    rtt: [12, 40],
    jitter: [2, 6],
    cushion: 40,
    reconnectedAt: null,
    silentFor: 20,
  },
  unknown: { rtt: [4, 9] },
});

const round1 = (value) => Math.round(value * 10) / 10;

// The control channel's round trip of `row` in the second `second`; null for a row not connected.
function fakeRoundTrip(row, second) {
  if (!row.controlConnected) {
    return null;
  }
  const [low, high] = (LINK_PROFILES[row.linkHealth] ?? LINK_PROFILES.unknown).rtt;
  return round1(low + (high - low) * noise(row.clientId ?? row.address, second * 5 + 1));
}

// The level a second's numbers reach, on the desktop's thresholds (see health.rs).
function sampleHealth(lost, expected, underruns) {
  if (lost * 10000 >= 200 * expected || underruns >= 2) {
    return "bad";
  }
  return underruns > 0 || lost * 10000 >= 50 * expected ? "warning" : "good";
}

// A 256-frame packet at 48 kHz, the blocks the demo interface sends, in milliseconds.
const FAKE_PACKET_MS = (256 * 1000) / 48000;

/**
 * The cushion a phone plays for a fixed buffer of `ms` milliseconds, as it reports it: the length
 * rounded up to whole packets of the demo's blocks, to the hundredth.
 */
export function fixedCushionMs(ms) {
  const packets = Math.ceil(ms / FAKE_PACKET_MS - 1e-9);
  return Math.round(packets * FAKE_PACKET_MS * 100) / 100;
}

/**
 * The link history of `row` (a status row) up to `time` milliseconds, as `link_history` answers
 * its samples: one a second, the oldest first, the newest `count` of at most LINK_HISTORY_SAMPLES,
 * each numbered by its second (`seq`) with the clock's milliseconds as its monotonic time.
 * The same row and time give the same samples. A row that never reported (`linkHealth` "unknown")
 * or is not connected has none; a "warning" link reconnects once every ten minutes; a lost ("bad")
 * link's newest 20 seconds are gaps; a phone that does not announce "stats.v2" reports no jitter,
 * cushion or backlog, and only one that announces "redundancy.recovered" reports recovered packets.
 * `fixed`, `{ ms, since }`, is a fixed buffer the desk set at the second `since`: from that second on
 * the phone reports the cushion it plays for it (fixedCushionMs) instead of its own.
 * `redundantSince` is the second the desk turned redundancy on for the musician (null while off),
 * with the copies `delayBlocks` blocks late: from then on a copy brings most packets the phone lost
 * (three in four), received, not lost. Immediate copies (0) do so for any phone, and are never
 * counted as recovered (they arrive in order); delayed ones only for a phone that announces
 * "redundancy.hold", whose cushion then holds the delay too, and one that announces
 * "redundancy.recovered" counts them as `recovered`. Before it, or off, such a phone reports 0;
 * a phone that cannot hold packets back drops delayed copies, so its losses stay as they were.
 */
export function syntheticLinkHistory(
  row,
  time,
  count = LINK_HISTORY_SAMPLES,
  fixed = null,
  redundantSince = null,
  delayBlocks = 0,
) {
  const profile = LINK_PROFILES[row.linkHealth];
  if (!profile || row.linkHealth === "unknown" || !row.controlConnected) {
    return [];
  }
  const capabilities = row.capabilities ?? [];
  const extras = capabilities.includes("stats.v2");
  const recovering = capabilities.includes("redundancy.recovered");
  const holding = delayBlocks > 0 && capabilities.includes("redundancy.hold");
  const key = row.clientId ?? row.address;
  const newest = Math.floor(time / LINK_SAMPLE_PERIOD_MS);
  const samples = [];
  for (let back = Math.min(count, LINK_HISTORY_SAMPLES) - 1; back >= 0; back -= 1) {
    const second = newest - back;
    const atMs = second * LINK_SAMPLE_PERIOD_MS;
    const rttMs = fakeRoundTrip(row, second);
    const reconnected = profile.reconnectedAt === second % LINK_HISTORY_SAMPLES;
    if (back < profile.silentFor) {
      samples.push({
        seq: second,
        monoMs: atMs,
        atMs,
        gap: true,
        reconnected,
        health: "unknown",
        intervalMs: null,
        packetsReceived: null,
        packetsLost: null,
        lossPercent: null,
        underruns: null,
        receiveTimeouts: null,
        rttMs,
        jitterMs: null,
        cushionMs: null,
        backlogMs: null,
        recovered: null,
      });
      continue;
    }
    const expected = 200;
    const dropped = Math.round(noise(key, second * 5 + 2) * 2 * expected * (profile.loss / 100));
    const redundant = redundantSince !== null && second >= redundantSince;
    const copied = redundant && (delayBlocks === 0 || holding);
    const lost = copied ? Math.floor(dropped / 4) : dropped;
    const underruns = profile.underrunEvery && second % profile.underrunEvery === 0 ? 1 : 0;
    const [low, high] = profile.jitter;
    const held = redundant && holding ? delayBlocks * FAKE_PACKET_MS : 0;
    const cushion = (fixed && second >= fixed.since ? fixedCushionMs(fixed.ms) : profile.cushion) + held;
    samples.push({
      seq: second,
      monoMs: atMs,
      atMs,
      gap: false,
      reconnected,
      health: sampleHealth(lost, expected, underruns),
      intervalMs: LINK_SAMPLE_PERIOD_MS,
      packetsReceived: expected - lost,
      packetsLost: lost,
      lossPercent: (lost * 100) / expected,
      underruns,
      receiveTimeouts: 0,
      rttMs,
      jitterMs: extras ? round1(low + (high - low) * noise(key, second * 5 + 3)) : null,
      cushionMs: extras ? cushion : null,
      backlogMs: extras ? round1(cushion - 10 * noise(key, second * 5 + 4)) : null,
      recovered: recovering ? (holding ? dropped - lost : 0) : null,
    });
  }
  return samples;
}

/**
 * The round trip a status row shows for `row` at `time` milliseconds: `{ rttMs, rttLastMs }`, the
 * smoothed one (the mean of the last eight seconds) and the last; both null for a row not
 * connected. A phone that never reported has one too: it comes from the control channel.
 */
export function syntheticRoundTrip(row, time) {
  const newest = Math.floor(time / LINK_SAMPLE_PERIOD_MS);
  const last = fakeRoundTrip(row, newest);
  if (last === null) {
    return { rttMs: null, rttLastMs: null };
  }
  let sum = 0;
  for (let back = 0; back < 8; back += 1) {
    sum += fakeRoundTrip(row, newest - back);
  }
  return { rttMs: round1(sum / 8), rttLastMs: last };
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

// A mix backup's stamp, as the desk names its files: UTC to the millisecond, `20261018T190405123Z`.
const BACKUP_STAMP = /^\d{8}T\d{9}Z$/;

function backupStamp(at) {
  return new Date(at).toISOString().replace(/[^0-9TZ]/g, "");
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
 * channels, applied, saved, saveError }` and `channel_table` reads them back whole; every start takes
 * them, and the engine keeps them whole
 * while a status shows those of the inputs the device has, so a device switch shows the details of
 * inputs that come back again, as the engine does. A hidden input is heard in no musician's meter; a new
 * pair is panned apart (0 and 100) in every mix, announced in a status before the moved mixes; and
 * every change of a pair's level, mute or solo, from the desk or a phone, moves both sides (see
 * keepPairsEqual). The engineer's controls too: `set_engineer_controls` merges a limit and locks as
 * the engine does (a pair locks as one, also when it is linked after the lock), announces the mix,
 * keeps them for the phone's client id (else the address) and answers `{ address, limitPercent,
 * lockedChannels, keptFor, applied, saved, saveError, mix }`; a phone cannot change a locked
 * channel, the desk can; the limit caps the musician's meter with the phone's cap, the lower
 * winning; with the engine stopped they are kept for the address, and every start gives each
 * musician the controls kept for them. A start asks for its input as the engine does (the first
 * device whose name contains the one asked for, in any case) and fails when none does, as with an
 * interface left unplugged. The audio input is kept as the desktop's session keeps it:
 * `audio_settings` reads it (null before any; `savedAudio` seeds it, as a session kept before) and
 * `set_audio_settings` keeps it only while an engine runs, refusing an unknown driver and ASIO
 * without a picked driver. Presets, copies and mirrors too, with the engine's rules (see mixes.js):
 * `save_preset` keeps a running musician's mix as a new preset (each input and the group levels by
 * id), `rename_preset` and `delete_preset` work with or without an engine, `apply_preset` and
 * `copy_mix` change the named mixes (never a master, cap or the engineer's controls) and refuse a
 * musician whose mix follows another's; `set_mirror` makes a follower take their leader's mix at
 * once and on every later change (from the desk, a phone, a preset or a copy, and on to whoever
 * follows them), holds the follower's phone off its channels and groups, refuses a circle, keeps it
 * like the engineer's controls (by phone, else address; idle edits for the next start) and every
 * start puts the kept mirrors in force; every status row says whom it follows (`mirror`). Named
 * sessions too, with the desktop's rules (see sessions.js): `save_session` keeps the whole desk
 * session (labels, groups, channel details, limits and locks, mirrors, presets and the audio input)
 * and, while the engine runs, every musician's mix by their key; `sessions` lists them newest first
 * (`savedSessions` seeds them, one with a `problem` cannot be loaded or renamed); `rename_session`
 * and `delete_session`; `load_session` checks the saved groups and channel details, replaces the
 * desk session and, while the engine runs, applies the groups and details, then the limits, locks
 * and mirrors, then pushes each saved mix to the musician here under its key (a phone that is not
 * connected and a musician not here are skipped; a follower gets only its master), answering as
 * the desktop does; its audio input is taken only as the engineer could choose it (ASIO only with a
 * picked driver) and only for the next start. `session_notice` answers `sessionNotice`; `clock`
 * gives the time a session is saved at. Besides `invoke` and `listen` it offers
 * `emit(event, payload)`, `changeFromPhone(address, update)` (a phone's mix change),
 * `movePhone(from, to)` (a phone back from a new IP, its row and label moving with it),
 * `emitMeters()` (one synthetic `meters` event), `startMeters(intervalMs)` and `startDemo()`
 * (timers until the returned stop function is called), `holdAnswers(command)` and
 * `releaseAnswers()` (answers that arrive late), `failNext(command, message)` (one failed call),
 * `failStart(message)`, `engineerKeys()` (the keys the engineer's controls are kept under),
 * `mirrors()` (the mirrors kept), `savedAudio()` (the audio input kept) and `calls`, every `{
 * command, args }` invoked. `presets` seeds the presets the session keeps.
 *
 * The engineer's local mix too, with the desktop's rules (see local-mix.js): `set_local_mix`
 * merges a change as `set_mix` does (no level cap: `maxLevelPercent` is refused), keeps it in the
 * session by group id and announces it as `{ run, id: "local", origin: "desktop", mix }`; a group
 * edit or a new pair moves it as it moves every musician's. `output_devices` lists `outputs` while
 * the engine runs (under ASIO only the driver's own outputs); `set_local_output` plays the local mix
 * on a pair of an output it offers, or none, refusing as the engine does, and with the engine
 * stopped keeps the choice for the next start. Every start plays the kept local mix by group id
 * and opens the kept output only when its driver offers it; one it does not is left off and the
 * status says why (`localOutput.notice`), never another device in its place. A saved session keeps
 * them and a load takes them the same way; one saved without them leaves the desk's own. The
 * `meters` carry the local mix's peak (`local`). `savedLocal` seeds what the session keeps, and
 * `savedLocal()` reads it; `failLocalOutput(reason)` stops the output in use, as an unplugged
 * device does.
 *
 * Recording takes too, with the desktop's rules (see recording.js): `recording_settings` and
 * `set_recording_settings` keep the settings as the session does (`savedRecording` seeds them and
 * `savedRecording()` reads them; the default folder is `recordingsFolder`, and a folder that is
 * not a whole path, goes up with `..` or is in FAKE_CONFIG_FOLDER is refused). `start_recording`
 * lays the tracks out as the engine does (an armed input past the device left out and named, a
 * linked pair as one stereo track, the local mix last) in a folder named in the window's local
 * time; its frames advance with `clock`, and the status carries `recording` and `lastTake`.
 * `add_marker` marks the frame recorded now (blank: the next "Marker n"), `stop_recording` ends
 * the take into the folder's list (`holdStop(times)` makes stops answer that the files are still
 * being finished), and a stop or restart of the engine ends it as `engineStopped`; a take stops a
 * device switch. `record_estimate` gives the bytes a second of the armed tracks against
 * `freeBytes`; `list_takes` lists `savedTakes` and the takes recorded, the newest first, with
 * `unreadableTakes` left out; `open_take_folder` opens only a listed take (`openedFolders()`
 * reads them); `failRecording(reason)` stops the take as a disk error does.
 *
 * The virtual soundcheck too, with the desktop's rules (see soundcheck.js): `start_soundcheck`
 * plays only a listed take of the recordings folder, refused as the engine refuses it (a
 * soundcheck already playing, a take recording, a rate other than 48 kHz, tracks that do not fit
 * the device's inputs and linked pairs now, no input audio, a start past the end), each in the
 * desktop's words; its position advances with `clock`, a take that does not loop pauses at its end
 * (resume starts it again), a looping one starts again at its first frame; a seek keeps a paused
 * take paused and plays one that stopped at its end. `pause_soundcheck`,
 * `resume_soundcheck`, `seek_soundcheck`, `set_soundcheck_loop` and `stop_soundcheck` answer the
 * soundcheck and send a status at once; the status carries `soundcheck` (paced by the local output
 * while it plays, else the timer). While a soundcheck plays, a take and a device switch are
 * refused; a stop or start of the engine ends it. `failSoundcheck(reason)` stops the take as a read
 * error does: only going back to the live inputs is left.
 *
 * Mix backups too, with the desktop's rules (see mix-backups-view.js): every musician's mix change
 * is noted, and a backup of every musician's mix (as of the last change) is taken once the mixes
 * have been still for BACKUP_QUIET_MS by `clock`, or BACKUP_MAX_WAIT_MS after the first change
 * while they keep changing, at the first command after it is due; nobody, or the same mixes as the
 * last backup, takes none, and only the newest KEPT_BACKUPS are kept. `list_mix_backups` lists them
 * newest first (`mixBackups` seeds them, one with a `problem` cannot be restored), and
 * `restore_mix_backup` pushes each backed-up mix to the musician here under its key as a load does
 * (a phone not connected and a musician not here are skipped), refusing a stamp that names no
 * backup and a restore with the engine stopped; `mixBackups()` reads them. Link histories too:
 * every connected row carries a round trip (`rttMs`, `rttLastMs`) and its newest second
 * (`linkLatest`), and `link_history` answers a musician's ten minutes of generated seconds (see
 * syntheticLinkHistory) by their key, numbered after `sinceSeq` when given, as the desktop does,
 * with a `historyId` per musician that a new engine start replaces. Playback buffers too (see
 * playback-buffer.js): `set_musician_buffer` keeps Auto or a fixed length for a phone that
 * announces "playback.buffer" (Lead vocal), as the desktop's session does, for the phone's client
 * id else its address, across engine starts and in named sessions; every row says its `buffer` and
 * `bufferSupported`, and from the second it was set the phone's reported cushion is the fixed
 * length rounded up to whole packets (fixedCushionMs). Any other phone answers "unsupported".
 * Redundancy too (see redundancy.js): `set_musician_redundancy` keeps it on, with the copies' delay
 * (0, immediate, by default), for any musician here, kept as buffers are; a delayed copy only for a
 * phone that announces "redundancy.hold" (Bass), any other answering "unsupported". Every row says
 * its `redundancy`, `duplicatesSent` (the copies sent since it was turned on), `recovered` (the
 * packets the phone reported a delayed copy recovered over its history), `redundancySupported`
 * (the phone announces "redundancy.recovered": Bass) and `redundancyHoldSupported`, and from the
 * second it was turned on the copies save most of its losses (syntheticLinkHistory).
 */
export function createFakeBridge({
  status = sampleStatus(),
  devices = sampleDevices(),
  outputs = sampleOutputs(),
  running = false,
  failStart = null,
  answersFirst = false,
  statusOnStart = true,
  savedAudio = null,
  savedLocal = null,
  presets = [],
  savedSessions = [],
  sessionNotice = null,
  mixBackups = [],
  clock = () => Date.now(),
  recordingsFolder = FAKE_RECORDINGS_FOLDER,
  savedRecording = null,
  savedTakes = [],
  unreadableTakes = 0,
  freeBytes = 500_000_000_000,
} = {}) {
  const engine = clone(status);
  engine.localMix ??= { ...defaultMix(), revision: 0 };
  engine.localOutput ??= offOutput();
  const listeners = new Map();
  const calls = [];
  let isRunning = running;
  // Each musician's link history id in the running engine, handed out as asked; a start forgets
  // them all, as a new engine starts every history anew.
  let historyIds = new Map();
  let nextHistoryId = 1;
  let run = running ? engine.run : 0;
  let startError = failStart;
  let revision = Math.max(
    0,
    engine.localMix.revision ?? 0,
    ...engine.musicians.map((musician) => musician.revision ?? 0),
  );
  // The local mix and its output as the desk's session keeps them, `{ mix, output }` (the mix as a
  // saved session keeps a mix, the output `{ device, firstChannel }`), or null before either was
  // kept; and the driver the engine runs, whose outputs it offers.
  let keptLocal = savedLocal ? clone(savedLocal) : null;
  let runningHost = "auto";
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
  // The engineer's controls as the desk's session keeps them, `{ engineerLimitPercent,
  // lockedChannels }` by labelKey: for the phone's client id when it has one, else the address.
  const engineerBook = new Map();
  // The playback buffers as the desk's session keeps them, `{ mode: "fixed", ms }` by labelKey (Auto
  // is kept as nothing), and the second each was set at, so a phone's reported cushion follows it
  // from then on.
  const bufferBook = new Map();
  const bufferSince = new Map();
  // The redundancy settings as the desk's session keeps them, `{ delayBlocks }` by labelKey (off is
  // kept as nothing), and the second each was turned on at.
  const redundancyBook = new Map();
  const redundancySince = new Map();
  // The audio input as the desk's session keeps it, `{ host, device, bufferFrames, asioDriver }`,
  // or null before any was kept.
  let keptAudio = savedAudio ? clone(savedAudio) : null;
  // The engineer's mix presets as the desk's session keeps them, in the order they were saved.
  let keptPresets = clone(presets);
  let lastPresetId = 0;
  // The mirrors as the desk's session keeps them, `{ leader, name }` by the follower's labelKey
  // (the leader by theirs): every start and `set_mirror` put them in force.
  const mirrorBook = new Map();
  // The named sessions, `{ name, savedAt, session, mixes, problem }` by id.
  const sessionBook = new Map(
    clone(savedSessions).map(({ id, ...saved }) => [id, { mixes: {}, problem: null, ...saved }]),
  );
  let lastSessionId = 0;
  // The backups of the musicians' mixes, `{ stamp, savedAt, musicians, problem }` (the musicians as
  // a backup file keeps them), as the desk takes them; the changes waiting for one, `{ first, last,
  // musicians }` (the mixes as of the last change), and the musicians of the last one taken.
  const backupBook = clone(mixBackups)
    .map((kept) => ({ musicians: [], problem: null, ...kept }))
    .sort((one, other) => (one.stamp < other.stamp ? -1 : 1));
  let backupWait = null;
  let lastBackedUp = null;
  // How the desk records takes, as its session keeps them (the folder null for the default).
  let keptRecording = {
    folder: null,
    format: "int24",
    includeLocal: false,
    armed: [],
    ...(savedRecording ? clone(savedRecording) : {}),
  };
  // The takes that ended, as their take.json describe them; the take being recorded (`{ folder,
  // name, started, startedAt, endedAt, format, device, tracks, markers, stopping, failure }`), the
  // last take that ended while this engine ran, the stops still to answer "finishing", and every
  // folder opened in the file manager.
  const takeBook = clone(savedTakes);
  let take = null;
  let lastTake = null;
  let finishingStops = 0;
  const openedFolders = [];

  // A rule of mixes.js or mix.js broken, as the desk refuses it: the error text, not an Error.
  function checked(rule) {
    try {
      return rule();
    } catch (error) {
      throw error.message;
    }
  }

  // The row a desk key names: a phone by its client id, an address by its host.
  function rowForKey(key) {
    if (key.startsWith("client:")) {
      const id = key.slice("client:".length);
      return engine.musicians.find((row) => row.clientId === id) ?? null;
    }
    const host = hostOf(key.slice("address:".length));
    return engine.musicians.find((row) => hostOf(row.address) === host) ?? null;
  }

  // The mirror kept for `row`'s musician: for their phone first, then for their address.
  function keptMirror(row) {
    for (const key of keysOf(row)) {
      if (mirrorBook.has(key)) {
        return mirrorBook.get(key);
      }
    }
    return null;
  }

  // Keeps `mirror` under `key`, or drops what is kept there with null.
  function keepMirror(key, mirror) {
    if (mirror) {
      mirrorBook.set(key, mirror);
    } else {
      mirrorBook.delete(key);
    }
  }

  // Whether, with no engine running, the musician known by the keys `follower` following the one
  // known by the keys `leader` would close a circle with the kept mirrors, as the desktop decides it:
  // at every step a musician is known by all the keys the desk has for them (the window's, and each
  // kept mirror's key with the address kept beside a phone's key), joined when two keys name one
  // musician (sameMusician), and the mirror kept for them is looked up by each of those keys,
  // phones first. A chain longer than the mirrors can only go round already.
  function closesCircleOffline(follower, leader) {
    const kept = [...mirrorBook.entries()].map(([key, mirror]) => [
      mirror.followerAddress ? [key, `address:${mirror.followerAddress}`] : [key],
      mirror.leaderAddress ? [mirror.leader, `address:${mirror.leaderAddress}`] : [mirror.leader],
    ]);
    const groups = [follower, leader, ...kept.flat()];
    const named = (keys) => keys.some((key) => follower.some((own) => sameMusician(key, own)));
    let known = [...leader];
    for (let step = 0; step <= kept.length; step += 1) {
      widenKeys(known, groups);
      if (named(known)) {
        return true;
      }
      known.sort((one, other) => Number(one.startsWith("address:")) - Number(other.startsWith("address:")));
      const next = known
        .map((key) => kept.find(([follows]) => sameMusician(follows[0], key)))
        .find(Boolean);
      if (!next) {
        return false;
      }
      known = [...next[1]];
    }
    return true;
  }

  // Whether following the kept mirrors from `leaderKey` reaches the follower (known by `keys`, and
  // by `row` when they are here): then following that leader would close a circle.
  function closesCircleFrom(leaderKey, keys, row = null) {
    let key = leaderKey;
    for (let step = 0; step <= mirrorBook.size; step += 1) {
      const here = rowForKey(key);
      if (keys.includes(key) || (row && here === row)) {
        return true;
      }
      const kept = here ? keptMirror(here) : (mirrorBook.get(key) ?? null);
      if (!kept) {
        return false;
      }
      key = kept.leader;
    }
    return true;
  }

  // Every row's `mirror` as the status shows it: whom it follows, and where the leader is now.
  function showMirrors() {
    for (const row of engine.musicians) {
      const kept = keptMirror(row);
      const leader = kept ? rowForKey(kept.leader) : null;
      row.mirror = kept
        ? {
            leader: kept.leader,
            leaderAddress: leader && leader !== row ? leader.address : null,
            name: kept.name,
          }
        : null;
    }
  }

  // The status as the desktop answers it, every row saying whom it follows, with the take being
  // recorded as it stands now and the last take that ended.
  function statusNow() {
    showMirrors();
    showBuffers();
    showRedundancy();
    showLinks();
    engine.recording = recordingNow();
    engine.lastTake = lastTake ? clone(lastTake) : null;
    engine.soundcheck = soundcheckNow();
    return clone(engine);
  }

  // Each row's round trip and newest link history sample now, as the engine reports them.
  function showLinks() {
    const now = clock();
    for (const row of engine.musicians) {
      Object.assign(row, syntheticRoundTrip(row, now));
      row.linkLatest =
        syntheticLinkHistory(row, now, 1, fixedOf(row), redundantSinceOf(row), delayOf(row))[0] ?? null;
    }
  }

  // The fixed buffer kept for `row`'s musician (for their phone first, then their address) when
  // their phone takes one, else null.
  function keptBuffer(row) {
    if (!(row.capabilities ?? []).includes("playback.buffer")) {
      return null;
    }
    const key = keysOf(row).find((candidate) => bufferBook.has(candidate));
    return key ? { key, buffer: bufferBook.get(key) } : null;
  }

  // What a phone's reports follow: the fixed buffer kept for it, `{ ms, since }`, or null.
  function fixedOf(row) {
    const kept = keptBuffer(row);
    return kept ? { ms: kept.buffer.ms, since: bufferSince.get(kept.key) ?? 0 } : null;
  }

  // Each row's buffer as the engine reports it: the one kept for it, else Auto, and whether its
  // phone takes one.
  function showBuffers() {
    for (const row of engine.musicians) {
      row.bufferSupported = (row.capabilities ?? []).includes("playback.buffer");
      row.buffer = clone(keptBuffer(row)?.buffer ?? { mode: "auto" });
    }
  }

  // The redundancy kept for `row`'s musician (for their phone first, then their address), `{ key,
  // redundancy }`, or null when it is off.
  function keptRedundancy(row) {
    const key = keysOf(row).find((candidate) => redundancyBook.has(candidate));
    return key ? { key, redundancy: redundancyBook.get(key) } : null;
  }

  // The second redundancy was turned on for `row`'s musician, or null while it is off.
  function redundantSinceOf(row) {
    const kept = keptRedundancy(row);
    return kept ? (redundancySince.get(kept.key) ?? 0) : null;
  }

  // The delay of the copies kept for `row`'s musician, in blocks (0 while off or immediate).
  function delayOf(row) {
    return keptRedundancy(row)?.redundancy.delayBlocks ?? 0;
  }

  // Each row's redundancy as the engine reports it: on with its delay, or off; the copies sent since
  // it was turned on (a 256-frame block at 48 kHz is 187.5 a second); the packets the phone reported
  // recovered over its history; whether the phone reports them, and whether it holds packets back
  // for delayed copies.
  function showRedundancy() {
    const now = clock();
    const second = Math.floor(now / LINK_SAMPLE_PERIOD_MS);
    for (const row of engine.musicians) {
      const kept = keptRedundancy(row);
      const since = redundantSinceOf(row);
      row.redundancySupported = (row.capabilities ?? []).includes("redundancy.recovered");
      row.redundancyHoldSupported = (row.capabilities ?? []).includes("redundancy.hold");
      row.redundancy = kept ? { on: true, delayBlocks: kept.redundancy.delayBlocks } : { on: false };
      row.duplicatesSent = kept && row.controlConnected ? Math.floor(Math.max(0, second - since) * 187.5) : 0;
      row.recovered = syntheticLinkHistory(row, now, LINK_HISTORY_SAMPLES, null, since, delayOf(row)).reduce(
        (total, sample) => total + (sample.recovered ?? 0),
        0,
      );
    }
  }

  // ---- Recording takes, with the desktop's rules (see recording.js and commands/recording.rs).

  // The folder takes are made in: the kept one, or the default.
  function recordingsFolderOf(kept) {
    return kept.folder ?? recordingsFolder;
  }

  function settingsAnswer() {
    return {
      folder: recordingsFolderOf(keptRecording),
      defaultFolder: recordingsFolder,
      folderIsDefault: keptRecording.folder === null,
      format: keptRecording.format,
      includeLocal: keptRecording.includeLocal,
      armed: [...keptRecording.armed],
      notice: null,
    };
  }

  // A Windows path's parts, in lower case for comparing.
  function pathParts(path) {
    return path
      .toLowerCase()
      .split(/[\\/]+/)
      .filter((part) => part !== "");
  }

  function isInside(path, folder) {
    const [inner, outer] = [pathParts(path), pathParts(folder)];
    return outer.every((part, index) => inner[index] === part);
  }

  // The folder a setting keeps, as the desktop checks it: null for the default.
  function keptFolder(folder) {
    const text = typeof folder === "string" ? folder.trim() : "";
    if (text === "") {
      return null;
    }
    if (!/^([A-Za-z]:[\\/]|\\\\|\/)/.test(text)) {
      throw `the recordings folder must be a whole path, such as D:\\Recordings, and \`${text}\` is not`;
    }
    if (text.split(/[\\/]+/).includes("..")) {
      throw `the recordings folder must not go up with \`..\`: \`${text}\``;
    }
    if (isInside(text, recordingsFolder) && isInside(recordingsFolder, text)) {
      return null;
    }
    if (isInside(text, FAKE_CONFIG_FOLDER)) {
      throw `the recordings folder cannot be in MixLink's own settings folder (${FAKE_CONFIG_FOLDER}): \`${text}\``;
    }
    return text;
  }

  // The tracks a take of `armed` (1-based) lays out on the running device, as the engine plans
  // them: a mono file per input, a stereo file per linked pair (either side arms it), in input
  // order, and the local mix last.
  function planTracks(armed, includeLocal) {
    const count = engine.sourceChannels;
    const tracks = [];
    for (let index = 0; index < count; index += 1) {
      const channel = engine.channels[index] ?? {};
      const pair = Boolean(channel.link) && index + 1 < count;
      const sources = pair ? [index, index + 1] : [index];
      const first = index + 1;
      index += sources.length - 1;
      if (!sources.some((source) => armed.includes(source + 1))) {
        continue;
      }
      const pad = (number) => String(number).padStart(2, "0");
      const fallback = pair ? `CH ${first}-${first + 1}` : `CH ${first}`;
      const prefix = pair ? `${pad(first)}-${pad(first + 1)}` : pad(first);
      const name = channel.name?.trim() ? channel.name : fallback;
      tracks.push({
        file: `${prefix} ${safeName(name, fallback)}.wav`,
        kind: pair ? "pair" : "input",
        channels: sources.map((source) => source + 1),
        name,
        instrument: channel.kind ?? "other",
      });
    }
    if (includeLocal) {
      tracks.push({
        file: "Local mix.wav",
        kind: "local",
        channels: [],
        name: "Local mix",
        instrument: null,
      });
    }
    return tracks;
  }

  // `name` made safe for a file name, as the engine makes it.
  function safeName(name, fallback) {
    const cleaned = String(name)
      .replace(/[<>:"/\\|?*]/g, "-")
      .slice(0, 60)
      .trim()
      .replace(/\.+$/, "");
    return cleaned || fallback;
  }

  function bytesPerSecond(tracks, format) {
    const bytes = format === "float32" ? 4 : 3;
    const width = (track) => (track.kind === "local" ? 2 : track.channels.length);
    return tracks.reduce((sum, track) => sum + width(track) * RECORD_RATE * bytes, 0);
  }

  // The armed inputs the running device has, and the ones it does not.
  function splitArmed(armed) {
    return [
      armed.filter((input) => input <= engine.sourceChannels),
      armed.filter((input) => input > engine.sourceChannels),
    ];
  }

  // The frames the take has recorded: they advance with the clock until it is asked to stop.
  function framesOf(running) {
    const end = running.endedAt ?? clock();
    return Math.max(0, Math.floor(((end - running.startedAt) * RECORD_RATE) / 1000));
  }

  function markerOf({ frame, name }) {
    return { frame, seconds: frame / RECORD_RATE, name };
  }

  // The take as `take.json` describes it once it ended as `ended`.
  function summaryOf(running, ended, reason = null) {
    const frames = framesOf(running);
    return {
      folder: running.folder,
      name: running.name,
      started: running.started,
      sampleRate: RECORD_RATE,
      format: running.format,
      device: running.device,
      tracks: running.tracks.map((track) => ({ ...track, frames })),
      frames,
      seconds: frames / RECORD_RATE,
      markers: running.markers.map(markerOf),
      droppedFrames: 0,
      ended,
      reason,
    };
  }

  // The recorder as the status reports it.
  function recordingNow() {
    if (!take) {
      return clone(IDLE_RECORDING);
    }
    const frames = framesOf(take);
    const rate = bytesPerSecond(take.tracks, take.format);
    const written = Math.floor((frames * rate) / RECORD_RATE);
    const free = Math.max(0, freeBytes - written);
    let state = "recording";
    if (take.failure) {
      state = "failed";
    } else if (take.stopping) {
      state = "stopping";
    }
    return {
      state,
      reason: take.failure,
      takeFolder: take.folder,
      frames,
      seconds: frames / RECORD_RATE,
      droppedFrames: 0,
      bytesWritten: written,
      tracks: take.tracks.map((track) => track.name),
      markers: take.markers.map(markerOf),
      freeBytes: free,
      minutesLeft: rate > 0 ? Math.floor(free / rate / 60) : null,
    };
  }

  // Ends the take as `ended`: its summary joins the folder and is the last take.
  function finishTake(ended, reason = null) {
    take.endedAt ??= clock();
    const summary = summaryOf(take, ended, reason);
    takeBook.push(summary);
    lastTake = summary;
    take = null;
    return clone(summary);
  }

  // The take folder's name, `<YYYY-MM-DD_HHMMSS>_<name>` in local time by `offset` minutes, never
  // one the folder has.
  function takeFolder(root, name, offset) {
    const local = new Date(clock() + offset * 60_000);
    const pad = (number) => String(number).padStart(2, "0");
    const stamp =
      `${local.getUTCFullYear()}-${pad(local.getUTCMonth() + 1)}-${pad(local.getUTCDate())}_` +
      `${pad(local.getUTCHours())}${pad(local.getUTCMinutes())}${pad(local.getUTCSeconds())}`;
    const base = `${root}\\${stamp}_${safeName(name, "Take")}`;
    const taken = new Set(takeBook.map((summary) => summary.folder));
    let folder = base;
    for (let copy = 2; taken.has(folder); copy += 1) {
      folder = `${base} (${copy})`;
    }
    return folder;
  }

  // Every take the recordings folder holds, the take being recorded included, the newest first.
  function listedTakes() {
    const root = `${recordingsFolderOf(keptRecording)}\\`.toLowerCase();
    const all = [...takeBook.map(clone), ...(take ? [summaryOf(take, "recording")] : [])];
    return all
      .filter((summary) => summary.folder.toLowerCase().startsWith(root))
      .sort(
        (one, other) =>
          other.started.localeCompare(one.started) || other.folder.localeCompare(one.folder),
      );
  }

  // ---- The virtual soundcheck, with the desktop's rules (see soundcheck.js and
  // commands/soundcheck.rs).

  // The refusals in the desktop's words (its `refusal_words`).
  const SOUNDCHECK_WORDS = Object.freeze({
    notRunning: "the engine is not running, so no soundcheck is playing",
    recording: "a take is recording: stop the recording before the soundcheck",
    already: "a soundcheck is already playing a take: go back to the live inputs first",
    notPlaying: "no soundcheck is playing",
    recordingRefused:
      "a soundcheck is playing a take: go back to the live inputs before recording (a take never " +
      "records playback)",
    switchRefused:
      "a soundcheck is playing a take: go back to the live inputs before switching the input device",
    empty: "the take has no input audio to play",
  });

  // `frames` of a take as the refusals say them: "2.5 s".
  function refusalSeconds(frames) {
    return `${(frames / RECORD_RATE).toFixed(1)} s`;
  }

  function pastEnd(frame, length) {
    return `${refusalSeconds(frame)} is past the end of the take (${refusalSeconds(length)})`;
  }

  // Why the tracks of `listed` (a take as `list_takes` lists it) do not fit the running device's
  // inputs now, as the engine checks them, or null when they fit; also the frames they play for.
  function checkLayout(listed) {
    const count = engine.sourceChannels;
    const units = [];
    for (let index = 0; index < count; index += 1) {
      const pair = Boolean(engine.channels[index]?.link) && index + 1 < count;
      units.push(pair ? [index, index + 1] : [index]);
      index += pair ? 1 : 0;
    }
    const taken = new Set();
    let length = null;
    for (const track of listed.tracks) {
      if (track.kind === "local") {
        continue;
      }
      const inputs = track.channels.map((channel) => channel - 1);
      const past = inputs.find((input) => input >= count);
      if (past !== undefined) {
        return { reason: `the take recorded CH ${past + 1}, and this device has ${count} inputs` };
      }
      const twice = inputs.find((input) => taken.has(input));
      if (twice !== undefined) {
        return { reason: `CH ${twice + 1} is in two tracks of the take` };
      }
      const same = (unit) => unit.length === inputs.length && unit.every((side, at) => side === inputs[at]);
      if (!units.some(same)) {
        const first = inputs[0] + 1;
        if (inputs.length === 2) {
          return {
            reason: `CH ${first}-${first + 1} were recorded as a linked pair, which they are not now`,
          };
        }
        const pair = units.find((unit) => unit.includes(inputs[0]))?.[0] ?? inputs[0];
        return {
          reason: `CH ${first} was recorded on its own, and it is a side of the linked pair CH ${pair + 1}-${pair + 2} now`,
        };
      }
      inputs.forEach((input) => taken.add(input));
      const frames = track.frames ?? listed.frames ?? 0;
      length = length === null ? frames : Math.min(length, frames);
    }
    return { reason: null, length: length ?? 0 };
  }

  // The take playing in place of the inputs, `{ folder, name, length, base, since, paused, ended,
  // looping, failure }` (`base` the frame at `since`), or null while the inputs are live.
  let check = null;

  // The take's frame now: it advances with `clock` while it plays; at the end of a take that does
  // not loop it stops there (paused), and a looping one starts again from its first frame.
  function checkPosition() {
    if (check.paused || check.ended || check.failure) {
      return check.base;
    }
    const advanced = check.base + Math.floor(((clock() - check.since) * RECORD_RATE) / 1000);
    if (advanced < check.length) {
      return advanced;
    }
    if (check.looping) {
      return advanced % check.length;
    }
    check.base = check.length;
    check.ended = true;
    return check.base;
  }

  // Makes the frame now the take's base, before a change of how it plays.
  function settleCheck() {
    check.base = checkPosition();
    check.since = clock();
  }

  // The soundcheck as the status reports it.
  function soundcheckNow() {
    if (!check) {
      return clone(IDLE_SOUNDCHECK);
    }
    const position = checkPosition();
    let state = "playing";
    if (check.failure) {
      state = "failed";
    } else if (check.paused || check.ended) {
      state = "paused";
    }
    return {
      state,
      reason: check.failure,
      takeFolder: check.folder,
      takeName: check.name,
      positionFrames: position,
      positionSeconds: position / RECORD_RATE,
      lengthFrames: check.length,
      lengthSeconds: check.length / RECORD_RATE,
      looping: check.looping,
      underrunFrames: 0,
      // As the engine does: the local output's device paces the take while it plays.
      clock: engine.localOutput.state === "running" ? "output" : "timer",
    };
  }

  // The take that plays, to change how it plays: refused as the engine refuses it.
  function playingCheck() {
    if (!isRunning) {
      throw SOUNDCHECK_WORDS.notRunning;
    }
    if (!check) {
      throw SOUNDCHECK_WORDS.notPlaying;
    }
    if (check.failure) {
      throw `the soundcheck stopped playing: ${check.failure}; go back to the live inputs to stop it`;
    }
    return check;
  }

  // A soundcheck change made from the desk: the status goes out at once, as the desktop sends it.
  function soundcheckChanged() {
    if (isRunning) {
      emit("status", statusNow());
    }
    return soundcheckNow();
  }

  // As the engine does: `row`'s mix takes `leader`'s channels and group levels under a new
  // revision when they differ; its master, cap and the engineer's controls stay. Returns whether
  // it changed.
  function takeMix(row, leader) {
    const mix = mixFromDto(row);
    const led = mixFromDto(leader);
    if (sameShared(mix, led)) {
      return false;
    }
    revision += 1;
    Object.assign(row, withShared(mix, led), { revision });
    return true;
  }

  // As the engine does after any change of `leader`'s mix: every musician who follows it takes it,
  // and those who follow them in turn, each mix that moved announced.
  function propagate(leader) {
    const queue = [leader];
    const seen = new Set(queue);
    while (queue.length > 0) {
      const current = queue.shift();
      for (const row of engine.musicians) {
        const kept = keptMirror(row);
        if (seen.has(row) || !kept || rowForKey(kept.leader) !== current) {
          continue;
        }
        seen.add(row);
        if (takeMix(row, current)) {
          announceMix(row.address, "desktop", mixFromDto(row));
        }
        queue.push(row);
      }
    }
  }

  // As every start does: each musician on a kept mirror takes their leader's mix when the leader is
  // here, a follower of a follower after the one they follow.
  function giveKeptMirrors() {
    showMirrors();
    for (let pass = 0; pass < engine.musicians.length; pass += 1) {
      let moved = false;
      for (const row of engine.musicians) {
        const leader = row.mirror?.leaderAddress ? musicianAt(row.mirror.leaderAddress) : null;
        moved = (leader !== null && takeMix(row, leader)) || moved;
      }
      if (!moved) {
        return;
      }
    }
  }

  // The rows a desk change names, each once, as the engine finds them: an unknown musician, or one
  // whose mix follows another's, refuses the whole change.
  function deskTargets(addresses) {
    const rows = [];
    for (const address of addresses ?? []) {
      const row = musicianAt(address);
      const kept = keptMirror(row);
      if (kept) {
        throw `${address} follows ${kept.name}'s mix: stop that mirror before changing their mix`;
      }
      if (!rows.includes(row)) {
        rows.push(row);
      }
    }
    return rows;
  }

  function presetAt(id) {
    const preset = keptPresets.find((kept) => kept.id === id);
    if (!preset) {
      throw `there is no preset with the id \`${id}\``;
    }
    return preset;
  }

  function presetsAnswer(preset) {
    return { preset: preset ? clone(preset) : null, presets: clone(keptPresets) };
  }

  // A named session as `sessions` lists it.
  function sessionSummary(id, saved) {
    return {
      id,
      name: saved.name,
      savedAt: saved.savedAt ?? null,
      mixes: saved.problem ? 0 : Object.keys(saved.mixes ?? {}).length,
      problem: saved.problem ?? null,
    };
  }

  // Every named session, the newest first, as the desktop lists them.
  function listSessions() {
    return [...sessionBook.entries()]
      .map(([id, saved]) => sessionSummary(id, saved))
      .sort((one, other) => (other.savedAt ?? -1) - (one.savedAt ?? -1) || (one.id < other.id ? -1 : 1));
  }

  function sessionAt(id) {
    const saved = sessionBook.get(id);
    if (!saved) {
      throw `there is no saved session with the id \`${id}\``;
    }
    return saved;
  }

  // The desk's whole session, as a named session keeps it: while the engine runs, with the local
  // mix it plays.
  function deskSession() {
    const local = isRunning
      ? { mix: savedMixOf(engine.localMix), output: keptLocal?.output ?? null }
      : keptLocal;
    return clone({
      labels: [...labels.entries()],
      groups: savedGroups,
      channels: savedChannels,
      engineer: [...engineerBook.entries()],
      buffers: [...bufferBook.entries()],
      redundancies: [...redundancyBook.entries()],
      mirrors: [...mirrorBook.entries()],
      presets: keptPresets,
      audio: keptAudio,
      ...(local ? { local } : {}),
    });
  }

  // A mix as the desk keeps a whole mix: the device's inputs, the group levels by id, the master
  // volume and mute; never a level cap.
  function savedMixOf(mix) {
    return {
      channels: Array.from({ length: engine.sourceChannels }, (_, index) => ({
        gain: mix.channelGains[index],
        pan: mix.pans[index],
        muted: mix.channelMuted[index],
        solo: mix.channelSolo[index],
      })),
      groups: Object.fromEntries(
        engine.groups.map((group, position) => [group.id, mix.groupLevels[position]]),
      ),
      volumePercent: mix.volumePercent,
      muted: mix.muted,
    };
  }

  // Every musician's mix by their key, as a named session keeps it.
  function mixesNow() {
    return Object.fromEntries(engine.musicians.map((row) => [labelKey(row), savedMixOf(row)]));
  }

  // The outputs the running driver offers: under ASIO only the driver's own.
  function offeredOutputs() {
    return runningHost === "asio" ? [{ name: engine.deviceName, channels: ASIO_OUTPUTS }] : outputs;
  }

  // Why the running engine refuses to play on `output`, as the engine words it, or null.
  function outputRefusal({ device, firstChannel }) {
    const offered = offeredOutputs().find((candidate) => candidate.name === device);
    if (!offered) {
      return runningHost === "asio"
        ? `"${device}" is not an output the engine offers: under ASIO only the capture driver's own outputs can be used`
        : `"${device}" is not an output device of this computer`;
    }
    const last = firstChannel + 2;
    if (last > offered.channels) {
      return `outputs ${last - 1}-${last} are past "${device}", which has ${offered.channels} outputs`;
    }
    return null;
  }

  // The output the window sent, as the desktop reads it: `{ device, firstChannel }` with nothing
  // else, the device named.
  function outputRequest(output) {
    if (output === null || output === undefined) {
      return null;
    }
    for (const field of Object.keys(output)) {
      if (!OUTPUT_FIELDS.includes(field)) {
        throw `unknown field \`${field}\`, expected \`device\` or \`firstChannel\``;
      }
    }
    if (typeof output.device !== "string" || output.device === "") {
      throw "an output needs the name of its device";
    }
    // As the desktop reads it (a u16): required, a whole number from 0 to 65535. Like the engine,
    // it takes any pair within the device, odd ones included; the select offers 1/2, 3/4 and so on.
    if (!Object.hasOwn(output, "firstChannel")) {
      throw "missing field `firstChannel`";
    }
    const first = output.firstChannel;
    if (!Number.isInteger(first) || first < 0 || first > 65535) {
      throw `invalid value ${JSON.stringify(first)} for \`firstChannel\`, expected a whole number from 0 to 65535`;
    }
    return { device: output.device, firstChannel: first };
  }

  // Keeps `change` of the local section (its mix or its output) in the desk's session.
  function keepLocal(change) {
    keptLocal = { mix: null, output: null, ...keptLocal, ...change };
  }

  function announceLocal() {
    emit("mix-changed", { run, id: "local", origin: "desktop", mix: clone(engine.localMix) });
  }

  // Merges `update` into the local mix as the engine does (a pair's sides kept equal) under a new
  // revision.
  function mergeLocal(update) {
    let merged;
    try {
      const before = mixFromDto(engine.localMix);
      merged = keepPairsEqual(before, mergeMixUpdate(before, update ?? {}), runningChannels);
    } catch (error) {
      throw error.message;
    }
    revision += 1;
    merged.revision = revision;
    engine.localMix = merged;
    return merged;
  }

  // As every start and a load do with a kept local section: its mix through the desk's path, by
  // group id, then its output, opened only when the running driver offers it; one it does not, or
  // none kept, leaves the output off, never another device in its place. Answers why the kept
  // output was not opened, or null.
  function applyKeptLocal(local) {
    if (local?.mix) {
      mergeLocal(savedUpdate(local.mix, engine.groups, engine.localMix, null));
      announceLocal();
    }
    const output = local?.output ?? null;
    const now = engine.localOutput;
    if (
      output &&
      now.state === "running" &&
      now.device === output.device &&
      now.firstChannel === output.firstChannel
    ) {
      return null;
    }
    const refusal = output ? outputRefusal(output) : null;
    engine.localOutput = output && !refusal ? playingOutput(output.device, output.firstChannel) : offOutput();
    if (!refusal) {
      return null;
    }
    return (
      `The saved local mix output, ${outputWords(output)}, was not opened: the running audio ` +
      "driver does not offer it. The local mix plays nowhere until you choose an output."
    );
  }

  // The audio input a loaded session leaves the desk with, as the desktop takes it: the saved one
  // when the engineer could have chosen it, else the desk's own and why.
  function loadedAudio(saved) {
    if (!saved) {
      return { audio: keptAudio, loaded: false, skipped: null };
    }
    const host = String(saved.host ?? "auto").trim().toLowerCase();
    const named = (name) => (typeof name === "string" && name.trim() ? name.trim() : null);
    const asioDriver = named(saved.asioDriver);
    if (!HOSTS.includes(host)) {
      return {
        audio: keptAudio,
        loaded: false,
        skipped: `the audio driver \`${saved.host}\` is none of auto, wasapi, asio`,
      };
    }
    if (host === "asio" && !asioDriver) {
      return {
        audio: keptAudio,
        loaded: false,
        skipped: "ASIO is kept only with the ASIO driver the engineer picked, and none is named",
      };
    }
    return {
      audio: { host, device: named(saved.device), bufferFrames: saved.bufferFrames ?? null, asioDriver },
      loaded: true,
      skipped: null,
    };
  }

  // Pushes each of a loaded session's `mixes` to the musician here under its key, as the desktop
  // does, and answers the mixes pushed and the ones that went to nobody.
  function pushSavedMixes(mixes, skippedMix) {
    const pushed = [];
    const skipped = [];
    const used = new Set();
    for (const row of engine.musicians) {
      const key = keysOf(row).find((candidate) => Object.hasOwn(mixes, candidate));
      if (!key) {
        continue;
      }
      used.add(key);
      if (row.clientId && !row.controlConnected) {
        skipped.push(skippedMix(key, "notConnected"));
        continue;
      }
      const follows = keptMirror(row)?.name ?? null;
      const merged = mergeInto(row, savedUpdate(mixes[key], engine.groups, row, follows));
      announceMix(row.address, "desktop", merged);
      propagate(row);
      pushed.push({ address: row.address, follows, mix: clone(merged) });
    }
    for (const key of Object.keys(mixes).filter((candidate) => !used.has(candidate))) {
      skipped.push(skippedMix(key, "notHere"));
    }
    return { pushed, skipped };
  }

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

  // For each input, whether it starts a pair the running details link and `previous` did not.
  function newPairs(previous) {
    return runningChannels.map((channel, index) => channel.link && !previous[index]?.link);
  }

  // As the engine does when pairs are linked: every musician's new pairs are panned apart (0 and
  // 100) and take the first side's level, mute and solo, each mix under a new revision; a new pair
  // with either side locked is locked whole. Returns the rows that moved: every row when a pair was
  // linked that `previous` did not have, else none. The pairs are the whole table's, as the
  // engine's mixes take them, so a device switch moves none.
  function linkMixes(previous) {
    const linked = newPairs(previous);
    if (!linked.some(Boolean)) {
      return [];
    }
    const link = (mix) => {
      const before = mixFromDto(mix);
      const panned = mixFromDto(mix);
      linked.forEach((pair, first) => {
        if (pair) {
          panned.pans[first] = 0;
          panned.pans[first + 1] = 100;
        }
      });
      revision += 1;
      return { before, panned };
    };
    // As the engine does, the local mix moves with them (announced after them, see set_channels).
    const local = link(engine.localMix);
    engine.localMix = { ...keepPairsEqual(local.before, local.panned, runningChannels), revision };
    return engine.musicians.map((row) => {
      const { before, panned } = link(row);
      Object.assign(row, keepPairsEqual(before, panned, runningChannels), {
        lockedChannels: lockPairs(panned.lockedChannels, runningChannels),
        revision,
      });
      return row;
    });
  }

  // As every start does: each musician's mix is new to the engine, so it takes the controls kept
  // for its phone, else for its address, or none, a pair locked whole; a mix they change takes a
  // new revision.
  function giveKeptControls() {
    for (const row of engine.musicians) {
      const kept =
        (row.clientId && engineerBook.get(`client:${row.clientId}`)) ??
        engineerBook.get(`address:${row.address}`);
      const limit = kept?.engineerLimitPercent ?? 100;
      const locked = lockPairs(kept?.lockedChannels ?? defaultMix().lockedChannels, runningChannels);
      if (
        row.engineerLimitPercent !== limit ||
        JSON.stringify(row.lockedChannels) !== JSON.stringify(locked)
      ) {
        revision += 1;
        Object.assign(row, { engineerLimitPercent: limit, lockedChannels: locked, revision });
      }
    }
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
    noteBackup();
  }

  // Every musician as a backup keeps them: their key, the name the window shows, their client id
  // and address, and their whole mix (never the phone's cap).
  function backedUpNow() {
    return engine.musicians.map((row) => ({
      key: labelKey(row),
      name: row.name ?? null,
      clientId: row.clientId ?? null,
      address: row.address,
      mix: savedMixOf(row),
    }));
  }

  // A musician's mix changed: as the desk does, a backup is due once the mixes have been still for
  // BACKUP_QUIET_MS, or BACKUP_MAX_WAIT_MS after the first change while they keep changing. The
  // fake takes it at the first command after it is due, with the mixes as of the last change.
  function noteBackup() {
    settleBackups();
    const now = clock();
    backupWait = { first: backupWait?.first ?? now, last: now, musicians: backedUpNow() };
  }

  function settleBackups() {
    if (!backupWait) {
      return;
    }
    const due = Math.min(backupWait.last + BACKUP_QUIET_MS, backupWait.first + BACKUP_MAX_WAIT_MS);
    if (clock() < due) {
      return;
    }
    const { musicians } = backupWait;
    backupWait = null;
    // As the desk does: nobody, or the same mixes as the last backup, is not backed up again.
    const written = JSON.stringify(musicians);
    if (musicians.length === 0 || written === lastBackedUp) {
      return;
    }
    let at = due;
    while (backupBook.some((kept) => kept.stamp === backupStamp(at))) {
      at += 1;
    }
    backupBook.push({ stamp: backupStamp(at), savedAt: at, musicians: clone(musicians), problem: null });
    lastBackedUp = written;
    backupBook.sort((one, other) => (one.stamp < other.stamp ? -1 : 1));
    backupBook.splice(0, Math.max(0, backupBook.length - KEPT_BACKUPS));
  }

  // A backup as `list_mix_backups` lists it.
  function backupSummary(kept) {
    return {
      stamp: kept.stamp,
      savedAt: kept.problem ? null : (kept.savedAt ?? null),
      musicians: kept.problem ? 0 : kept.musicians.length,
      names: kept.problem
        ? []
        : kept.musicians.map((musician) => musician.name ?? musician.address ?? "a phone"),
      problem: kept.problem ?? null,
    };
  }

  function emitMeters() {
    if (isRunning) {
      emit("meters", syntheticMeters(engine, performance.now()));
    }
  }

  // Merges `update` into `row`'s mix as the engine does; a phone's change leaves every channel the
  // engineer locked as it was, and while the mix follows another's every channel and group level.
  function mergeInto(row, update, { fromPhone = false } = {}) {
    let merged;
    try {
      const before = mixFromDto(row);
      const applied = mergeMixUpdate(before, update ?? {});
      let allowed = fromPhone ? keepLocked(before, applied) : applied;
      if (fromPhone && keptMirror(row)) {
        allowed = withShared(allowed, before);
      }
      merged = keepPairsEqual(before, allowed, runningChannels);
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
      return isRunning ? { state: "running", ...statusNow() } : { state: "notRunning", run };
    },
    start_engine({ config } = {}) {
      // As in the desktop, a start ends the previous engine and takes a run even when it fails.
      run += 1;
      if (take) {
        // As the engine does: a take ends with its engine.
        finishTake("engineStopped");
      }
      // A new engine has had no take yet, and its inputs are live.
      lastTake = null;
      check = null;
      isRunning = false;
      if (startError) {
        throw startError;
      }
      // As the engine does: the first device whose name contains the one asked for, in any case,
      // and no start when none does (an interface left unplugged); with none asked for, the
      // driver's first one when a driver is named.
      const offered = config?.host === "asio" ? sampleAsioDrivers() : devices;
      const filter = config?.deviceFilter ?? null;
      let wanted = null;
      if (filter !== null) {
        const sought = filter.toLowerCase();
        wanted = offered.find((device) => device.name.toLowerCase().includes(sought));
        if (!wanted) {
          throw `no input device name contains "${sought}".`;
        }
      } else if (config?.host) {
        wanted = offered[0];
      }
      // As the desktop does: a start that names no groups takes the kept ones; one that names
      // some runs with those and keeps nothing.
      const named = config?.groups ?? [];
      runningGroups = named.length > 0 ? withIds(named) : savedGroups;
      if (wanted) {
        useDevice(wanted);
      }
      // As the engine does: a new engine has the default local mix and no local output, on the
      // driver it was started with.
      runningHost = config?.host ?? "auto";
      revision += 1;
      engine.localMix = { ...defaultMix(), revision };
      engine.localOutput = offOutput();
      // As the desktop does: every start takes the kept channel details, so a new engine pans
      // every kept pair apart before the window hears of it.
      runningChannels = savedChannels;
      showGroups(runningGroups);
      linkMixes([]);
      giveKeptControls();
      giveKeptMirrors();
      historyIds = new Map();
      isRunning = true;
      engine.run = run;
      engine.stopped = false;
      // As the desktop does last: the kept local mix, then the kept output when the driver
      // offers it; the status says why one it does not offer was left off.
      const notice = applyKeptLocal(keptLocal);
      engine.localOutput.notice = notice;
      if (statusOnStart) {
        emit("status", statusNow());
      }
      return summary();
    },
    stop_engine() {
      if (take) {
        // As the engine does: the take ends with it, its files finished.
        finishTake("engineStopped");
      }
      // A soundcheck ends with its engine too.
      check = null;
      isRunning = false;
      return null;
    },
    list_input_devices({ host } = {}) {
      return clone(host === "asio" ? sampleAsioDrivers() : devices);
    },
    audio_settings() {
      return keptAudio ? clone(keptAudio) : null;
    },
    set_audio_settings({ settings = {} } = {}) {
      // As the desktop does: only while an engine runs, a driver it knows, never ASIO without the
      // driver the engineer picked; names trimmed, an empty one none.
      if (!isRunning) {
        throw "the engine is not running: only the audio input an engine runs with is kept";
      }
      const host = String(settings.host ?? "auto").trim().toLowerCase();
      if (!HOSTS.includes(host)) {
        throw `the audio driver \`${settings.host}\` is none of auto, wasapi, asio`;
      }
      const named = (name) => (typeof name === "string" && name.trim() ? name.trim() : null);
      const asioDriver = named(settings.asioDriver);
      if (host === "asio" && !asioDriver) {
        throw "ASIO is kept only with the ASIO driver the engineer picked, and none is named";
      }
      keptAudio = {
        host,
        device: named(settings.device),
        bufferFrames: settings.bufferFrames ?? null,
        asioDriver,
      };
      return clone(keptAudio);
    },
    switch_device({ device }) {
      requireRunning();
      if (take && !take.failure) {
        throw "a take is recording: stop the recording before switching the input device (a take never mixes two devices)";
      }
      if (check) {
        throw SOUNDCHECK_WORDS.switchRefused;
      }
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
      // A label the desk kept for this address before shows again, as in the desktop, and a mirror
      // kept for it is in force at once.
      showLabel(row);
      engine.musicians.push(row);
      giveKeptMirrors();
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
      const row = musicianAt(address);
      const merged = mergeInto(row, mix);
      if (answersFirst) {
        // The answer settles at once; the event follows in a later task.
        setTimeout(() => announceMix(address, "desktop", merged), 0);
      } else {
        announceMix(address, "desktop", merged);
      }
      // As the engine does: everyone who follows this mix takes it at once.
      propagate(row);
      return clone(merged);
    },
    set_local_mix({ mix } = {}) {
      requireRunning();
      if (mix?.maxLevelPercent !== undefined) {
        throw "the local mix has no level cap: `maxLevelPercent` belongs to a musician's phone";
      }
      const merged = mergeLocal(mix);
      // As the desktop does: the engine takes it, then the session keeps it by group id.
      keepLocal({ mix: savedMixOf(merged) });
      if (answersFirst) {
        setTimeout(announceLocal, 0);
      } else {
        announceLocal();
      }
      return clone(merged);
    },
    output_devices() {
      if (!isRunning) {
        throw "the engine is not running: the outputs offered depend on the driver it runs";
      }
      return clone(offeredOutputs());
    },
    set_local_output({ output = null } = {}) {
      const chosen = outputRequest(output);
      if (isRunning) {
        // As the engine does: a refusal changes nothing and the output in use keeps playing.
        const refusal = chosen ? outputRefusal(chosen) : null;
        if (refusal) {
          throw refusal;
        }
        engine.localOutput = chosen ? playingOutput(chosen.device, chosen.firstChannel) : offOutput();
      }
      keepLocal({ output: chosen });
      return {
        output: clone(chosen),
        localOutput: isRunning ? clone(engine.localOutput) : null,
        applied: isRunning,
        saved: true,
        saveError: null,
      };
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
        emit("status", statusNow());
        const moved =
          before.length !== after.length || before.some((id, index) => id !== after[index]);
        if (moved) {
          for (const row of engine.musicians) {
            revision += 1;
            row.groupLevels = regroupedLevels(row.groupLevels, before, after);
            row.revision = revision;
            announceMix(row.address, "desktop", mixFromDto(row));
          }
          // As the engine does: the local mix's levels follow their groups too, announced last.
          revision += 1;
          engine.localMix = {
            ...engine.localMix,
            groupLevels: regroupedLevels(engine.localMix.groupLevels, before, after),
            revision,
          };
          announceLocal();
        }
      }
      return outcome;
    },
    set_channels({ channels } = {}) {
      const next = channelRequests(channels ?? []);
      try {
        // As the desktop does: a kind it does not know only where the session keeps that kind.
        checkChannels(next, savedChannels);
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
          emit("status", statusNow());
        }
        const linked = newPairs(previous).some(Boolean);
        for (const row of linkMixes(previous)) {
          announceMix(row.address, "desktop", mixFromDto(row));
        }
        if (linked) {
          announceLocal();
        }
      }
      return outcome;
    },
    channel_table() {
      // As the desktop answers: the running engine's whole table, else the session's.
      return clone(isRunning ? runningChannels : savedChannels);
    },
    set_engineer_controls({ address, controls, clientId = null } = {}) {
      const outcome = (kept, keptFor, applied, mix) => ({
        address,
        limitPercent: kept.engineerLimitPercent,
        lockedChannels: [...kept.lockedChannels],
        keptFor,
        applied,
        saved: true,
        saveError: null,
        mix,
      });
      const merge = (mix) => {
        try {
          return mergeEngineerUpdate(mix, controls ?? {});
        } catch (error) {
          throw error.message;
        }
      };
      if (!isRunning) {
        // As the desktop does with no engine: the address must read as a musician's (an IP and a
        // port, never a host name), an empty client id names no phone, and the controls are merged
        // into those kept for the musician and kept under the strongest key the window gave, the
        // phone's (dropping the address's) else the address, for the next start.
        if (!isSocketAddress(address)) {
          throw `invalid musician address \`${address}\``;
        }
        const phone = typeof clientId === "string" && clientId.trim() ? clientId : null;
        const phoneKey = phone ? `client:${phone}` : null;
        const addressKey = `address:${address}`;
        const kept = merge(
          (phoneKey && engineerBook.get(phoneKey)) ?? engineerBook.get(addressKey) ?? defaultMix(),
        );
        if (phoneKey) {
          engineerBook.delete(addressKey);
        }
        engineerBook.set(phoneKey ?? addressKey, kept);
        return outcome(kept, phoneKey ? "phone" : "address", false, null);
      }
      const row = musicianAt(address);
      const before = mixFromDto(row);
      const merged = merge(before);
      const { lockedChannels } = keepPairsEqual(before, { ...before, ...merged }, runningChannels, [
        "lockedChannels",
      ]);
      revision += 1;
      Object.assign(row, {
        engineerLimitPercent: merged.engineerLimitPercent,
        lockedChannels,
        revision,
      });
      // As the desktop keeps them: for the phone, dropping those kept for its address.
      const kept = { engineerLimitPercent: row.engineerLimitPercent, lockedChannels: [...lockedChannels] };
      if (row.clientId) {
        engineerBook.delete(`address:${row.address}`);
      }
      engineerBook.set(labelKey(row), kept);
      const mix = mixFromDto(row);
      announceMix(address, "desktop", mix);
      return outcome(kept, row.clientId ? "phone" : "address", true, clone(mix));
    },
    presets() {
      return clone(keptPresets);
    },
    save_preset({ address, name } = {}) {
      // As the desktop does: from a mix the engine plays, under a new name and a new id.
      if (!isRunning) {
        throw "the engine is not running: a preset is saved from a mix the engine plays";
      }
      const kept = checked(() => checkPresetName(name, keptPresets));
      const row = musicianAt(address);
      lastPresetId += 1;
      const preset = { id: `p${lastPresetId}`, name: kept, mix: presetOf(mixFromDto(row), engine) };
      keptPresets = [...keptPresets, preset];
      return presetsAnswer(preset);
    },
    rename_preset({ id, name } = {}) {
      const preset = presetAt(id);
      const kept = checked(() => checkPresetName(name, keptPresets, id));
      keptPresets = keptPresets.map((other) => (other === preset ? { ...other, name: kept } : other));
      return presetsAnswer(keptPresets.find((other) => other.id === id));
    },
    delete_preset({ id } = {}) {
      const preset = presetAt(id);
      keptPresets = keptPresets.filter((other) => other !== preset);
      return presetsAnswer(null);
    },
    apply_preset({ id, musicians = [] } = {}) {
      if (!isRunning) {
        throw "the engine is not running: a preset is put on mixes the engine plays";
      }
      const preset = presetAt(id);
      const rows = deskTargets(musicians);
      const mixes = rows.map((row) => {
        revision += 1;
        Object.assign(row, withPreset(mixFromDto(row), preset.mix, engine.groups, runningChannels), {
          revision,
        });
        announceMix(row.address, "desktop", mixFromDto(row));
        propagate(row);
        return { address: row.address, mix: clone(mixFromDto(row)) };
      });
      return { mixes };
    },
    copy_mix({ from, to = [] } = {}) {
      if (!isRunning) {
        throw "the engine is not running: a mix is copied between mixes the engine plays";
      }
      const source = musicianAt(from);
      const rows = deskTargets(to);
      if (rows.includes(source)) {
        throw `${from}'s mix cannot be copied onto itself`;
      }
      const mixes = rows.map((row) => {
        if (takeMix(row, source)) {
          announceMix(row.address, "desktop", mixFromDto(row));
          propagate(row);
        }
        return { address: row.address, mix: clone(mixFromDto(row)) };
      });
      return { mixes };
    },
    set_mirror({ address, clientId = null, leader = null } = {}) {
      const phoneOf = (id) => (typeof id === "string" && id.trim() ? id : null);
      const answer = (kept, keptFor, applied, mix) => ({
        address,
        leader: leader?.address ?? null,
        name: kept?.name ?? null,
        keptFor,
        applied,
        saved: true,
        saveError: null,
        mix,
      });
      const named = (fallback) => String(leader?.name ?? "").trim() || fallback;
      const circle = () =>
        `${leader.address} already follows ${address}'s mix, directly or through another ` +
        "musician: a mirror cannot go round in a circle";
      if (!isRunning) {
        // As the desktop does with no engine: kept under the strongest keys the window gave, for
        // the next start, never on oneself or in a circle with those kept.
        if (!isSocketAddress(address)) {
          throw `invalid musician address \`${address}\``;
        }
        const phone = phoneOf(clientId);
        const keys = phone ? [`client:${phone}`, `address:${address}`] : [`address:${address}`];
        let kept = null;
        if (leader) {
          if (!isSocketAddress(leader.address)) {
            throw `invalid musician address \`${leader.address}\``;
          }
          const leaderPhone = phoneOf(leader.clientId);
          const leading = leaderPhone
            ? [`client:${leaderPhone}`, `address:${leader.address}`]
            : [`address:${leader.address}`];
          // Keys are compared as the desktop compares them: phones by client id, addresses by IP.
          if (leading.some((key) => keys.some((own) => sameMusician(key, own)))) {
            throw `${address} cannot follow their own mix`;
          }
          if (closesCircleOffline(keys, leading)) {
            throw circle();
          }
          // As the desktop keeps it: a phone's key with its address beside it, so a later check
          // with no engine knows both keys of that musician.
          kept = {
            leader: leading[0],
            name: named(hostOf(leader.address)),
            ...(leaderPhone ? { leaderAddress: leader.address } : {}),
            ...(phone ? { followerAddress: address } : {}),
          };
        }
        if (phone) {
          mirrorBook.delete(`address:${address}`);
        }
        keepMirror(keys[0], kept);
        return answer(kept, phone ? "phone" : "address", false, null);
      }
      const row = musicianAt(address);
      let kept = null;
      if (leader) {
        const leading = musicianAt(leader.address);
        if (leading === row) {
          throw `${address} cannot follow their own mix`;
        }
        if (closesCircleFrom(labelKey(leading), keysOf(row), row)) {
          throw circle();
        }
        kept = {
          leader: labelKey(leading),
          name: named(leading.clientName ?? hostOf(leading.address)),
          ...(leading.clientId ? { leaderAddress: leading.address } : {}),
          ...(row.clientId ? { followerAddress: row.address } : {}),
        };
      }
      // As the desktop keeps it: for the phone, dropping what was kept for its address.
      if (row.clientId) {
        mirrorBook.delete(`address:${row.address}`);
      }
      keepMirror(labelKey(row), kept);
      showMirrors();
      // As the engine does: the phone is told (a new revision), and a new leader's mix is taken
      // at once and carried on to whoever follows this musician.
      revision += 1;
      row.revision = revision;
      const leading = kept ? rowForKey(kept.leader) : null;
      if (leading) {
        takeMix(row, leading);
      }
      announceMix(address, "desktop", mixFromDto(row));
      if (leading) {
        propagate(row);
      }
      return answer(kept, row.clientId ? "phone" : "address", true, clone(mixFromDto(row)));
    },
    session_notice() {
      return sessionNotice;
    },
    sessions() {
      return listSessions();
    },
    save_session({ name } = {}) {
      const saved = {
        name: checked(() => checkSessionName(name, listSessions())),
        savedAt: clock(),
        session: deskSession(),
        mixes: isRunning ? mixesNow() : {},
        problem: null,
      };
      let id;
      do {
        lastSessionId += 1;
        id = `s${lastSessionId}`;
      } while (sessionBook.has(id));
      sessionBook.set(id, saved);
      return { session: sessionSummary(id, saved), engineRunning: isRunning };
    },
    rename_session({ id, name } = {}) {
      const saved = sessionAt(id);
      if (saved.problem) {
        throw `the saved session \`${id}\` cannot be renamed: ${saved.problem}`;
      }
      saved.name = checked(() => checkSessionName(name, listSessions(), id));
      return sessionSummary(id, saved);
    },
    delete_session({ id } = {}) {
      sessionAt(id);
      sessionBook.delete(id);
      return null;
    },
    load_session({ id } = {}) {
      const saved = sessionAt(id);
      if (saved.problem) {
        throw `the saved session \`${id}\` cannot be loaded: ${saved.problem}`;
      }
      const { session, mixes } = clone(saved);
      // As the desktop does: the saved groups and details are checked before anything changes.
      try {
        checkGroups(withIds(session.groups ?? []));
      } catch (error) {
        throw `the saved groups cannot be used: ${error}`;
      }
      try {
        // A kind a newer build wrote loads as written and applies as `other`, as in the desktop.
        const loaded = channelRequests(session.channels ?? []);
        checkChannels(loaded, loaded);
      } catch (error) {
        throw `the saved channel details cannot be used: ${error.message ?? error}`;
      }
      const audio = loadedAudio(session.audio);
      const savedLabels = new Map(session.labels ?? []);
      const skippedMix = (key, reason) => ({
        key,
        label: savedLabels.get(key) ?? null,
        reason,
        error: null,
      });
      // The desk's session is the loaded one from now on.
      labels.clear();
      for (const [key, label] of savedLabels) {
        labels.set(key, label);
      }
      engineerBook.clear();
      for (const [key, controls] of session.engineer ?? []) {
        engineerBook.set(key, controls);
      }
      mirrorBook.clear();
      for (const [key, mirror] of session.mirrors ?? []) {
        mirrorBook.set(key, mirror);
      }
      // As the desktop does: the loaded buffers replace the kept ones, so a phone on a fixed buffer
      // the loaded session does not keep goes back to Auto.
      bufferBook.clear();
      for (const [key, buffer] of session.buffers ?? []) {
        bufferBook.set(key, buffer);
        bufferSince.set(key, Math.floor(clock() / LINK_SAMPLE_PERIOD_MS));
      }
      // And its redundancy settings: a musician the loaded session does not keep is sent once.
      redundancyBook.clear();
      for (const [key, redundancy] of session.redundancies ?? []) {
        redundancyBook.set(key, redundancy);
        redundancySince.set(key, Math.floor(clock() / LINK_SAMPLE_PERIOD_MS));
      }
      keptPresets = session.presets ?? [];
      keptAudio = audio.audio;
      // As the desktop does: a saved session from before the local mix leaves the desk's own.
      const localLoaded = Boolean(session.local);
      if (localLoaded) {
        keptLocal = clone(session.local);
      }
      let pushed = [];
      let skipped = [];
      let localMix = null;
      let localOutputNotice = null;
      if (isRunning) {
        // In the desktop's order: groups and details (each status before the mixes they move),
        // then limits, locks and mirrors, then the saved mixes, then the local mix and output.
        commands.set_groups({ groups: session.groups ?? [] });
        // The loaded details are the session's first, so a kind a newer build wrote is taken.
        savedChannels = channelRequests(session.channels ?? []);
        commands.set_channels({ channels: session.channels ?? [] });
        engine.musicians.forEach(showLabel);
        giveKeptControls();
        giveKeptMirrors();
        emit("status", statusNow());
        ({ pushed, skipped } = pushSavedMixes(mixes ?? {}, skippedMix));
        if (localLoaded) {
          localOutputNotice = applyKeptLocal(keptLocal);
          engine.localOutput.notice = localOutputNotice;
          localMix = keptLocal.mix ? clone(engine.localMix) : null;
        }
      } else {
        savedGroups = withIds(session.groups ?? []);
        savedChannels = channelRequests(session.channels ?? []);
        skipped = Object.keys(mixes ?? {}).map((key) => skippedMix(key, "notRunning"));
      }
      return {
        session: sessionSummary(id, saved),
        applied: isRunning,
        saved: true,
        saveError: null,
        mixes: pushed,
        skipped,
        refused: [],
        damaged: [],
        audio: clone(keptAudio),
        audioLoaded: audio.loaded,
        audioSkipped: audio.skipped,
        presets: clone(keptPresets),
        localLoaded,
        localMix,
        localOutputNotice,
      };
    },
    link_history({ musicianKey, sinceSeq = null } = {}) {
      // As the desktop answers: a key of the desk's forms, then a running engine, then a musician.
      const key = String(musicianKey ?? "");
      if (!key.startsWith("client:") && !key.startsWith("address:")) {
        throw `\`${key}\` names no phone or address`;
      }
      if (!isRunning) {
        throw "the engine is not running";
      }
      const row = rowForKey(key);
      if (!row) {
        throw `\`${key}\` names no musician`;
      }
      const samples = syntheticLinkHistory(
        row,
        clock(),
        LINK_HISTORY_SAMPLES,
        fixedOf(row),
        redundantSinceOf(row),
        delayOf(row),
      ).filter(
        (sample) => sinceSeq === null || sample.seq > sinceSeq,
      );
      if (!historyIds.has(key)) {
        historyIds.set(key, nextHistoryId);
        nextHistoryId += 1;
      }
      return {
        musicianKey: key,
        historyId: historyIds.get(key),
        samples,
        maxSamples: LINK_HISTORY_SAMPLES,
        periodMs: LINK_SAMPLE_PERIOD_MS,
      };
    },
    set_musician_redundancy({ musicianKey, on, delayBlocks = null } = {}) {
      // As the desktop answers: a key of the desk's forms, then the delay asked for, then a running
      // engine, then a musician here, then, for a delayed copy, a phone that holds packets back for
      // it ("redundancy.hold"). Every phone takes immediate copies.
      const key = String(musicianKey ?? "");
      if (!key.startsWith("client:") && !key.startsWith("address:")) {
        throw `\`${key}\` names no phone or address`;
      }
      const answer = (outcome, redundancy, message) => ({
        musicianKey: key,
        outcome,
        redundancy,
        bandwidthFactor: redundancy?.on ? 2 : 1,
        saved: redundancy !== null,
        saveError: null,
        message,
        minDelayBlocks: 0,
        maxDelayBlocks: 4,
        defaultDelayBlocks: 0,
      });
      const delay = delayBlocks ?? 0;
      if (on && (!Number.isInteger(delay) || delay < 0 || delay > 4)) {
        return answer("invalid", null, `Not set: a copy must be from 0 to 4 blocks late, got ${delay}.`);
      }
      if (!isRunning) {
        throw "the engine is not running";
      }
      const row = rowForKey(key);
      if (!row) {
        const named = key.startsWith("client:") ? `the phone ${key.slice(7)}` : key.slice(8);
        throw `${named} is not here`;
      }
      if (on && delay > 0 && !(row.capabilities ?? []).includes("redundancy.hold")) {
        return answer(
          "unsupported",
          null,
          "Not set: this musician's phone needs a newer app to play a delayed copy; an immediate copy works with any app.",
        );
      }
      // Kept for the phone when it has a client id (dropping one kept for its address), else for
      // the address; off is kept as nothing.
      const keptFor = labelKey(row);
      if (row.clientId) {
        redundancyBook.delete(`address:${row.address}`);
      }
      if (on) {
        redundancyBook.set(keptFor, { delayBlocks: delay });
        redundancySince.set(keptFor, Math.floor(clock() / LINK_SAMPLE_PERIOD_MS));
        return answer(
          "set",
          { on: true, delayBlocks: delay },
          delay === 0
            ? "Each packet is sent twice, the copy right after it: this musician's bandwidth doubles."
            : `Each packet is sent twice, the copy ${delay} ${delay === 1 ? "block" : "blocks"} later: this musician's bandwidth doubles.`,
        );
      }
      redundancyBook.delete(keptFor);
      return answer("set", { on: false }, "Each packet is sent once again: this musician's bandwidth is back to normal.");
    },
    set_musician_buffer({ musicianKey, mode, ms = null } = {}) {
      // As the desktop answers: a key of the desk's forms, then the buffer asked for, then a
      // running engine, then a musician here whose phone takes one.
      const key = String(musicianKey ?? "");
      if (!key.startsWith("client:") && !key.startsWith("address:")) {
        throw `\`${key}\` names no phone or address`;
      }
      const answer = (outcome, buffer, message) => ({
        musicianKey: key,
        outcome,
        buffer,
        saved: buffer !== null,
        saveError: null,
        message,
        minMs: 5,
        maxMs: 500,
      });
      let buffer;
      if (mode === "auto") {
        buffer = { mode: "auto" };
      } else if (mode !== "fixed") {
        return answer("invalid", null, `Not set: \`${mode}\` is no buffer mode: it is \`auto\` or \`fixed\`.`);
      } else if (ms === null || ms === undefined) {
        return answer("invalid", null, "Not set: a fixed buffer needs its length in ms.");
      } else if (!Number.isInteger(ms) || ms < 5 || ms > 500) {
        return answer(
          "invalid",
          null,
          `Not set: a fixed buffer must be between 5 and 500 ms, got ${ms} ms.`,
        );
      } else {
        buffer = { mode: "fixed", ms };
      }
      if (!isRunning) {
        throw "the engine is not running";
      }
      const row = rowForKey(key);
      if (!row) {
        const named = key.startsWith("client:") ? `the phone ${key.slice(7)}` : key.slice(8);
        throw `${named} is not here`;
      }
      if (!(row.capabilities ?? []).includes("playback.buffer")) {
        return answer(
          "unsupported",
          null,
          "This phone needs a newer app to take its buffer from the desk.",
        );
      }
      // Kept for the phone when it has a client id (dropping one kept for its address), else for
      // the address; Auto is kept as nothing.
      const keptFor = labelKey(row);
      if (row.clientId) {
        bufferBook.delete(`address:${row.address}`);
      }
      if (buffer.mode === "fixed") {
        bufferBook.set(keptFor, buffer);
      } else {
        bufferBook.delete(keptFor);
      }
      bufferSince.set(keptFor, Math.floor(clock() / LINK_SAMPLE_PERIOD_MS));
      const words = buffer.mode === "fixed" ? `a fixed ${buffer.ms} ms` : "Auto";
      return row.controlConnected
        ? answer("sent", buffer, `Sent to the phone: ${words}.`)
        : answer("notConnected", buffer, `Kept: the phone gets ${words} when it is connected again.`);
    },
    list_mix_backups() {
      settleBackups();
      return [...backupBook].reverse().map(backupSummary);
    },
    restore_mix_backup({ stamp } = {}) {
      settleBackups();
      // As the desktop does: only a stamp of a backup's form, naming one in the backups folder.
      const kept = BACKUP_STAMP.test(String(stamp ?? ""))
        ? backupBook.find((candidate) => candidate.stamp === stamp)
        : null;
      if (!kept) {
        throw `\`${stamp}\` is not a mix backup in ${FAKE_CONFIG_FOLDER}\\backups`;
      }
      if (kept.problem) {
        throw `the mix backup \`${stamp}\` cannot be restored: ${kept.problem}`;
      }
      if (!isRunning) {
        throw "the engine is not running: a backup is put back on the musicians who are connected";
      }
      const mixes = Object.fromEntries(kept.musicians.map((musician) => [musician.key, musician.mix]));
      const skippedMix = (key, reason) => ({
        key,
        label: labels.get(key) ?? kept.musicians.find((musician) => musician.key === key)?.name ?? null,
        reason,
        error: null,
      });
      const { pushed, skipped } = pushSavedMixes(mixes, skippedMix);
      return { backup: backupSummary(kept), restored: pushed, skipped };
    },
    recording_settings() {
      return settingsAnswer();
    },
    set_recording_settings({ settings = {} } = {}) {
      // As the desktop reads it: no field it does not know, then each one checked; a refusal
      // changes nothing.
      for (const field of Object.keys(settings ?? {})) {
        if (!RECORDING_FIELDS.includes(field)) {
          throw `unknown field \`${field}\`, expected one of \`folder\`, \`format\`, \`includeLocal\`, \`armed\``;
        }
      }
      const name = typeof settings.format === "string" ? settings.format.trim() : "";
      const format = name === "" ? RECORD_FORMATS[0] : name;
      if (!RECORD_FORMATS.includes(format)) {
        throw `the format \`${name}\` is none of int24, float32`;
      }
      for (const input of settings.armed ?? []) {
        if (!Number.isInteger(input) || input < 1 || input > 255) {
          throw `CH ${input} cannot be armed: inputs are numbered from 1 to 255`;
        }
      }
      keptRecording = {
        folder: keptFolder(settings.folder),
        format,
        includeLocal: Boolean(settings.includeLocal),
        armed: [...new Set(settings.armed ?? [])].sort((one, other) => one - other),
      };
      return settingsAnswer();
    },
    start_recording({ takeName = "", utcOffsetMinutes = null } = {}) {
      const offset = utcOffsetMinutes ?? 0;
      if (Math.abs(offset) > 14 * 60) {
        throw `the time zone offset of ${offset} minutes is more than 14 hours`;
      }
      if (!isRunning) {
        throw "the engine is not running: start it to record";
      }
      if (check) {
        throw SOUNDCHECK_WORDS.recordingRefused;
      }
      if (take && !take.failure) {
        throw "a take is already recording: stop it first";
      }
      if (take) {
        // As the engine does: a take a disk error stopped is collected before a new one starts.
        finishTake("diskError", take.failure);
      }
      const [armed, skipped] = splitArmed(keptRecording.armed);
      if (armed.length === 0 && !keptRecording.includeLocal) {
        throw skipped.length === 0
          ? "nothing is armed: arm at least one input, or record the local mix"
          : `nothing is armed on this device: ${skipped.map((input) => `CH ${input}`).join(", ")} ` +
              `are past its ${engine.sourceChannels} inputs; arm one of its inputs, or record the ` +
              "local mix";
      }
      const name = String(takeName ?? "").trim() || "Take";
      const root = recordingsFolderOf(keptRecording);
      const startedAt = clock();
      take = {
        folder: takeFolder(root, name, offset),
        name,
        started: new Date(startedAt).toISOString().replace(/\.\d{3}Z$/, "Z"),
        startedAt,
        endedAt: null,
        format: keptRecording.format,
        device: engine.deviceName,
        tracks: planTracks(armed, keptRecording.includeLocal),
        markers: [],
        stopping: false,
        failure: null,
      };
      return {
        recording: recordingNow(),
        folder: root,
        armed,
        skipped,
        includeLocal: keptRecording.includeLocal,
        format: keptRecording.format,
      };
    },
    stop_recording() {
      if (!isRunning) {
        throw "the engine is not running, so nothing is recording";
      }
      if (!take) {
        throw "nothing is recording";
      }
      take.stopping = true;
      take.endedAt ??= clock();
      if (finishingStops > 0) {
        finishingStops -= 1;
        return {
          finishing: true,
          take: null,
          message: "the take's files are still being finished; try again",
        };
      }
      const ended = take.failure ? "diskError" : "stopped";
      return { finishing: false, take: finishTake(ended, take.failure), message: null };
    },
    add_marker({ name = "" } = {}) {
      if (!isRunning) {
        throw "the engine is not running, so no take is recording";
      }
      if (!take || take.stopping) {
        throw "no take is recording";
      }
      if (take.failure) {
        throw `the take stopped recording: ${take.failure}`;
      }
      const trimmed = String(name ?? "").trim();
      let named = trimmed.slice(0, 64);
      if (!trimmed) {
        const taken = new Set(take.markers.map((marker) => marker.name));
        let number = take.markers.length + 1;
        while (taken.has(`Marker ${number}`)) {
          number += 1;
        }
        named = `Marker ${number}`;
      }
      const marker = { frame: framesOf(take), name: named };
      take.markers.push(marker);
      return markerOf(marker);
    },
    record_estimate() {
      const folder = recordingsFolderOf(keptRecording);
      const unknown = (problem) => ({
        folder,
        bytesPerSecond: null,
        freeBytes,
        minutesLeft: null,
        problem,
      });
      if (!isRunning) {
        return unknown("the engine is not running: the tracks depend on the input device");
      }
      const [armed] = splitArmed(keptRecording.armed);
      const tracks = planTracks(armed, keptRecording.includeLocal);
      if (tracks.length === 0) {
        return unknown("arm at least one input, or record the local mix");
      }
      const rate = bytesPerSecond(tracks, keptRecording.format);
      return {
        folder,
        bytesPerSecond: rate,
        freeBytes,
        minutesLeft: Math.floor(freeBytes / rate / 60),
        problem: null,
      };
    },
    list_takes() {
      const takes = listedTakes();
      return {
        folder: recordingsFolderOf(keptRecording),
        takes,
        unreadable: unreadableTakes,
        missing: takes.length === 0 && unreadableTakes === 0,
      };
    },
    open_take_folder({ folder } = {}) {
      // As the desktop does: only a take folder directly inside the recordings folder.
      const root = recordingsFolderOf(keptRecording);
      if (!listedTakes().some((listed) => listed.folder === folder)) {
        throw `${folder} is not a take folder in the recordings folder ${root}`;
      }
      openedFolders.push(folder);
      return null;
    },
    start_soundcheck({ takeFolder = "", startFrame = 0, looping = false } = {}) {
      if (!isRunning) {
        throw "the engine is not running: start it to soundcheck a take";
      }
      // As the desktop does: only a take folder directly inside the recordings folder.
      const root = recordingsFolderOf(keptRecording);
      const folder = String(takeFolder ?? "").trim();
      const listed = listedTakes().find((summary) => summary.folder === folder);
      if (!listed) {
        throw `${folder} is not a take folder in the recordings folder ${root}`;
      }
      // Then as the engine does, in its order.
      if (check) {
        throw SOUNDCHECK_WORDS.already;
      }
      if (take && !take.failure) {
        throw SOUNDCHECK_WORDS.recording;
      }
      if (listed.sampleRate !== RECORD_RATE) {
        throw (
          `the take was recorded at ${listed.sampleRate} Hz; only ${RECORD_RATE} Hz takes play ` +
          "(there is no resampler)"
        );
      }
      const { reason, length } = checkLayout(listed);
      if (reason) {
        throw (
          `the take does not fit this device's inputs now: ${reason}; play it on the device and ` +
          "with the linked pairs it was recorded with"
        );
      }
      if (length === 0) {
        throw SOUNDCHECK_WORDS.empty;
      }
      const from = Math.max(0, Math.floor(Number(startFrame) || 0));
      if (from > length) {
        throw pastEnd(from, length);
      }
      check = {
        folder,
        name: listed.name,
        length,
        base: from,
        since: clock(),
        paused: false,
        ended: false,
        looping: Boolean(looping),
        failure: null,
      };
      return soundcheckChanged();
    },
    pause_soundcheck() {
      playingCheck();
      settleCheck();
      check.paused = true;
      return soundcheckChanged();
    },
    resume_soundcheck() {
      playingCheck();
      settleCheck();
      if (check.ended || check.base >= check.length) {
        // As the engine does: a take at its end, stopped there or paused and moved there, starts
        // again from its first frame.
        check.base = 0;
        check.ended = false;
      }
      check.paused = false;
      return soundcheckChanged();
    },
    seek_soundcheck({ frame = 0 } = {}) {
      playingCheck();
      const to = Math.max(0, Math.floor(Number(frame) || 0));
      if (to > check.length) {
        throw pastEnd(to, check.length);
      }
      settleCheck();
      // As the engine does: a take the engineer paused stays paused at the new frame; one that
      // stopped at its end (not paused) plays on from it.
      check.base = to;
      check.ended = false;
      return soundcheckChanged();
    },
    set_soundcheck_loop({ looping = false } = {}) {
      playingCheck();
      settleCheck();
      check.looping = Boolean(looping);
      return soundcheckChanged();
    },
    stop_soundcheck() {
      if (!isRunning) {
        throw SOUNDCHECK_WORDS.notRunning;
      }
      if (!check) {
        throw SOUNDCHECK_WORDS.notPlaying;
      }
      check = null;
      return soundcheckChanged();
    },
  };

  // A change made on the musician's phone: merged as the engine would (a locked channel kept as it
  // was, and every channel and group of a mix that follows another's), announced as "phone", and
  // taken at once by everyone who follows this mix.
  function changeFromPhone(address, update) {
    const row = musicianAt(address);
    announceMix(address, "phone", mergeInto(row, update, { fromPhone: true }));
    propagate(row);
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
      emit("status", statusNow());
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
    /** The keys the engineer's controls are kept under, as the desktop's session keys them. */
    engineerKeys() {
      return [...engineerBook.keys()];
    },
    /** The mirrors kept, `[followerKey, { leader, name }]`, as the desktop's session keys them. */
    mirrors() {
      return clone([...mirrorBook.entries()]);
    },
    /** The audio input the session keeps, as `audio_settings` reads it: null before any. */
    savedAudio() {
      return keptAudio ? clone(keptAudio) : null;
    },
    /** The local mix and output the session keeps, `{ mix, output }`: null before either. */
    savedLocal() {
      return keptLocal ? clone(keptLocal) : null;
    },
    /** The mix backups taken, `{ stamp, savedAt, musicians, problem }`, the oldest first. */
    mixBackups() {
      settleBackups();
      return clone(backupBook);
    },
    /** The recording settings the session keeps, `{ folder, format, includeLocal, armed }`. */
    savedRecording() {
      return clone(keptRecording);
    },
    /** Every take folder opened in the file manager, in order. */
    openedFolders() {
      return [...openedFolders];
    },
    /** The next `times` stops answer that the take's files are still being finished. */
    holdStop(times = 1) {
      finishingStops = times;
    },
    /** The take being recorded stops on its own with `reason`, as a full disk stops it. */
    failRecording(reason) {
      if (take && !take.failure) {
        take.failure = reason;
        take.endedAt ??= clock();
        if (isRunning) {
          emit("status", statusNow());
        }
      }
    },
    /** The take playing in place of the inputs stops on a read error with `reason`. */
    failSoundcheck(reason) {
      if (check && !check.failure) {
        settleCheck();
        check.failure = reason;
        if (isRunning) {
          emit("status", statusNow());
        }
      }
    },
    /** The local output in use stops on its own with `reason`, as an unplugged device does. */
    failLocalOutput(reason) {
      if (engine.localOutput.device !== null) {
        engine.localOutput = { ...engine.localOutput, state: "failed", reason };
        if (isRunning) {
          emit("status", statusNow());
        }
      }
    },
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
            emit("status", statusNow());
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
