// What the Mix screen shows, worked out from the console state. Pure functions, no DOM: the
// screen's organisms only render these plain objects.

import { balanceToPans, channelLabel, pairUnits, pansToBalance } from "./channels.js";
import { musicianByAddress, musiciansNeedingAttention, viewMix } from "./console-state.js";
import { formatCaptureFormat, formatPercent, linkState, musicianLabel } from "./format.js";
import { editKey } from "./mix.js";
import { groupTone } from "./values.js";

const SEPARATOR = " · ";

/** Which action the engine button offers in each phase; none while a start or stop is on. */
const ENGINE_ACTIONS = Object.freeze({
  idle: "start",
  error: "start",
  starting: null,
  stopping: null,
  running: "stop",
  stopped: "stop",
});

/**
 * The top bar: `{ phase, device, format, attention, attentionLabel, action }`. `action` is "start",
 * "stop" or null; `attentionLabel` is null when no phone needs attention.
 */
export function topBarView(state) {
  const { phase, status } = state.engine;
  const attention = musiciansNeedingAttention(state).length;
  let attentionLabel = null;
  if (attention > 0) {
    attentionLabel = `${attention} ${attention === 1 ? "phone needs" : "phones need"} attention`;
  }
  return {
    phase,
    device: status?.deviceName ?? null,
    format: status ? formatCaptureFormat(status.captureFormat) : null,
    attention,
    attentionLabel,
    action: ENGINE_ACTIONS[phase] ?? null,
  };
}

/** One tab per musician, in the engine's order: `{ address, label, link, selected }`. */
export function musicianTabs(state) {
  return state.musicians.map((musician) => ({
    address: musician.address,
    label: musicianLabel(musician),
    link: linkState(musician),
    selected: musician.address === state.selected,
  }));
}

// The configured groups with the tone each one's chip and strip share.
function groupsWithTones(state) {
  return (state.engine.status?.groups ?? []).map((group, index) => ({
    ...group,
    index,
    tone: groupTone(index),
  }));
}

/**
 * One strip per shown input of the device, left to right, for `address`'s mix as it shows (with
 * local edits): `{ index, sides, stereo, label, kind, group, gain, pan, muted, solo, tone,
 * readout }`. A hidden input has no strip. A stereo pair is one strip (`stereo`, `sides` both
 * inputs, `index` the first): its level, mute and solo are its first side's, which the engine keeps
 * equal to the second's, and its pan is the balance the two pans make (pansToBalance). A strip is
 * called by its input's name, or its number when it has none (channelLabel). A muted strip reads
 * "muted", not only a duller fill. A channel's group is found by id: two groups may share a name.
 */
export function channelStrips(state, address) {
  const mix = viewMix(state, address);
  const status = state.engine.status;
  if (!mix || !status) {
    return [];
  }
  const groups = groupsWithTones(state);
  const channels = status.channels.slice(0, status.sourceChannels);
  return pairUnits(channels)
    .filter(([first]) => !channels[first].hidden)
    .map((sides) => {
      const [index, second] = sides;
      const channel = channels[index];
      const group = channel.groupId
        ? groups.find((candidate) => candidate.id === channel.groupId)
        : null;
      const stereo = second !== undefined;
      const muted = mix.channelMuted[index];
      return {
        index,
        sides,
        stereo,
        label: channelLabel(channel),
        kind: channel.kind ?? "other",
        group: group
          ? { id: group.id, name: group.name, tone: group.tone, invalid: group.invalid }
          : null,
        gain: mix.channelGains[index],
        pan: stereo ? pansToBalance(mix.pans[index], mix.pans[second]) : mix.pans[index],
        muted,
        solo: mix.channelSolo[index],
        tone: muted ? "muted" : "default",
        readout: muted ? "muted" : formatPercent(mix.channelGains[index]),
      };
    });
}

/**
 * The local edits a move on a channel strip makes, keyed as editKey does: a stereo strip's level,
 * mute or solo goes to both of its inputs, and its balance becomes both pans (balanceToPans); a
 * strip of one input edits that input. `strip` is `{ sides, stereo }` from channelStrips.
 */
export function channelEdits({ sides, stereo }, field, value) {
  if (field === "pans" && stereo) {
    const [left, right] = balanceToPans(value);
    return { [editKey("pans", sides[0])]: left, [editKey("pans", sides[1])]: right };
  }
  return Object.fromEntries(sides.map((side) => [editKey(field, side), value]));
}

/** The level a strip's meter shows: its input's peak, or the louder side of a pair. */
export function stripLevel(sides, levels) {
  return Math.max(...sides.map((side) => levels[side] ?? 0));
}

/**
 * One strip per configured group: `{ index, id, name, tone, invalid, level, readout }`. `index` is
 * the group's position, which is where its level is in the mix.
 */
export function groupStrips(state, address) {
  const mix = viewMix(state, address);
  if (!mix) {
    return [];
  }
  return groupsWithTones(state).map(({ index, id, name, tone, invalid }) => ({
    index,
    id,
    name,
    tone,
    invalid,
    level: mix.groupLevels[index],
    readout: formatPercent(mix.groupLevels[index]),
  }));
}

/** The musician's master: `{ volume, cap, muted, readout }`, or null without that musician. */
export function masterStrip(state, address) {
  const mix = viewMix(state, address);
  if (!mix) {
    return null;
  }
  const level = mix.muted ? "muted" : formatPercent(mix.volumePercent);
  return {
    volume: mix.volumePercent,
    cap: mix.maxLevelPercent,
    muted: mix.muted,
    readout: `${level}${SEPARATOR}cap ${formatPercent(mix.maxLevelPercent)}`,
  };
}

/**
 * Why the mix area has no strips, or null when it has some: `{ kind }` with kind "disconnected"
 * (with `message`; the console cannot hear the engine), "connecting", "stopped", "starting",
 * "stopping", "error" (with `message`) or "no-musicians".
 */
export function mixNotice(state) {
  const { phase, error } = state.engine;
  const { connection } = state;
  if (connection.status === "failed") {
    return { kind: "disconnected", message: connection.error };
  }
  if (connection.status === "connecting" && phase === "idle") {
    return { kind: "connecting" };
  }
  if (phase === "idle" || phase === "stopped") {
    return { kind: "stopped" };
  }
  if (phase === "starting" || phase === "stopping") {
    return { kind: phase };
  }
  if (phase === "error") {
    return { kind: "error", message: error };
  }
  return state.musicians.length === 0 ? { kind: "no-musicians" } : null;
}

const COMMAND_FAILURES = Object.freeze({
  stop: "The engine could not be stopped",
  start: "The engine could not be started",
  switch: "The input device could not be switched",
});

/**
 * The sentences above the strips: `departed` when the open musician was removed and another one
 * opened, `error` when a change was refused, `command` when the engine refused a stop or a device
 * switch while it went on running. Each is null when there is nothing to say.
 */
export function mixMessages(state) {
  let command = null;
  if (state.commandError) {
    const { action, message } = state.commandError;
    command = `${COMMAND_FAILURES[action] ?? `The engine refused to ${action}`}: ${message}.`;
  }
  return { ...musicianMessages(state), command };
}

function musicianMessages(state) {
  let departed = null;
  const shown = musicianByAddress(state, state.selected);
  if (state.departed && shown) {
    departed = `${state.departed.label} was removed; showing ${musicianLabel(shown)}'s mix.`;
  }
  let error = null;
  if (state.mixError) {
    const musician = musicianByAddress(state, state.mixError.address);
    const name = musician ? musicianLabel(musician) : state.mixError.address;
    error = `A change to ${name}'s mix was not applied: ${state.mixError.message}.`;
  }
  return { departed, error };
}
