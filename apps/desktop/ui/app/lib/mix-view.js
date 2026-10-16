// What the Mix screen shows, worked out from the console state. Pure functions, no DOM: the
// screen's organisms only render these plain objects.

import { musicianByAddress, musiciansNeedingAttention, viewMix } from "./console-state.js";
import { formatCaptureFormat, formatPercent, linkState, musicianLabel } from "./format.js";
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
 * One strip per input of the device, for `address`'s mix as it shows (with local edits):
 * `{ index, label, group, gain, pan, muted, solo, tone, readout }`. Channels have no names yet,
 * so a strip is called by its number. A muted strip reads "muted", not only a duller fill.
 */
export function channelStrips(state, address) {
  const mix = viewMix(state, address);
  const status = state.engine.status;
  if (!mix || !status) {
    return [];
  }
  const groups = groupsWithTones(state);
  return status.channels.slice(0, status.sourceChannels).map((channel, index) => {
    const group = groups.find((candidate) => candidate.name === channel.group);
    const muted = mix.channelMuted[index];
    return {
      index,
      label: `CH ${channel.number}`,
      group: group ? { name: group.name, tone: group.tone, invalid: group.invalid } : null,
      gain: mix.channelGains[index],
      pan: mix.pans[index],
      muted,
      solo: mix.channelSolo[index],
      tone: muted ? "muted" : "default",
      readout: muted ? "muted" : formatPercent(mix.channelGains[index]),
    };
  });
}

/** One strip per configured group: `{ index, name, tone, invalid, level, readout }`. */
export function groupStrips(state, address) {
  const mix = viewMix(state, address);
  if (!mix) {
    return [];
  }
  return groupsWithTones(state).map(({ index, name, tone, invalid }) => ({
    index,
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
