// The Channels and groups screen (board 3 of the design): what it shows and the edits it makes.
// Pure functions, no DOM.
//
// The rows and the group cards are worked out from the console state, as mix-view.js does for the
// Mix screen. Every edit is a function of the whole list the desk keeps: the channel details in
// channel order (`[{ name, kind, hidden, link }]`, the first entry CH 1, entries past the device
// included) or the groups (`[{ id, name, channels }]` with 1-based channels and no id for a new
// group). An edit returns a new list and leaves the one given as it was; a list the desk would
// refuse throws an Error that says why, so nothing is sent.

import {
  CHANNEL_KINDS,
  KIND_LABELS,
  channelLabel,
  checkChannels,
  defaultChannel,
  pairUnits,
} from "./channels.js";
import { MAX_GROUPS } from "./mix.js";
import { groupTone } from "./values.js";

/** The instrument picker's options, in the engine's order: `{ value, label }`. */
export const KIND_OPTIONS = Object.freeze(
  CHANNEL_KINDS.map((value) => Object.freeze({ value, label: KIND_LABELS[value] })),
);

function input(index) {
  return `CH ${index + 1}`;
}

// The status's groups with the tone each one's chip shares on every screen.
function tonedGroups(status) {
  return (status?.groups ?? []).map((group, index) => ({ ...group, tone: groupTone(index) }));
}

// The status's inputs: as many as the device has.
function deviceChannels(status) {
  return status ? status.channels.slice(0, status.sourceChannels) : [];
}

/**
 * The stereo cell of input `index` (0-based) among a status's `channels`:
 * `{ linked, first, partner, text, label, available, reason }`. A linked input names the other
 * side (`partner`) and the pair's first input (`first`, where the link is kept); either side can
 * undo it. An input on its own can pair with the next one only when that one exists, is in no
 * other pair and is shown or hidden alike; otherwise `available` is false and `reason` says why.
 */
export function linkChoice(channels, index) {
  const unit = pairUnits(channels).find((candidate) => candidate.includes(index));
  if (unit.length === 2) {
    const [first, second] = unit;
    const partner = index === first ? second : first;
    return {
      linked: true,
      first,
      partner,
      text: `Linked with ${input(partner)}`,
      label: `Stereo pair ${input(first)} and ${input(second)}`,
      available: true,
      reason: null,
    };
  }
  const next = channels[index + 1];
  const choice = {
    linked: false,
    first: index,
    partner: next ? index + 1 : null,
    text: "Mono",
    label: `Stereo pair ${input(index)} and ${next ? input(index + 1) : "the next input"}`,
    available: false,
    reason: null,
  };
  if (!next) {
    choice.reason = `${input(index)} is the last input, so there is no next input to pair it with.`;
  } else if (pairUnits(channels).some((candidate) => candidate[0] === index + 1 && candidate.length === 2)) {
    choice.reason = `${input(index + 1)} is already paired with ${input(index + 2)}.`;
  } else if (Boolean(next.hidden) !== Boolean(channels[index].hidden)) {
    const [hidden, shown] = next.hidden ? [index + 1, index] : [index, index + 1];
    choice.reason = `${input(hidden)} is hidden and ${input(shown)} is shown: show or hide both before pairing them.`;
  } else {
    choice.available = true;
  }
  return choice;
}

/**
 * One row per input of the device: `{ index, number, input, name, label, kind, kindLabel, hidden,
 * stereo, group }`. `name` is the engineer's (empty when none), `label` what every screen calls
 * the input, `stereo` its linkChoice and `group` the group it is in, found by id, or null.
 */
export function channelRows(state) {
  const status = state.engine.status;
  const channels = deviceChannels(status);
  const groups = tonedGroups(status);
  return channels.map((channel, index) => {
    const group = channel.groupId ? groups.find((candidate) => candidate.id === channel.groupId) : null;
    const kind = KIND_LABELS[channel.kind] ? channel.kind : "other";
    return {
      index,
      number: channel.number,
      input: `CH ${channel.number}`,
      name: String(channel.name ?? ""),
      label: channelLabel(channel),
      kind,
      kindLabel: KIND_LABELS[kind],
      hidden: Boolean(channel.hidden),
      stereo: linkChoice(channels, index),
      group: group
        ? { id: group.id, name: group.name, tone: group.tone, invalid: group.invalid }
        : null,
    };
  });
}

function detailOf(entry) {
  return {
    name: String(entry?.name ?? ""),
    kind: entry?.kind ?? "other",
    hidden: Boolean(entry?.hidden),
    link: Boolean(entry?.link),
  };
}

/** The channel details a status shows, as the desk sends them; none without a status. */
export function channelTableOf(status) {
  return deviceChannels(status).map(detailOf);
}

/** The groups a status shows, as the desk sends them (1-based channels); none without a status. */
export function groupDefinitionsOf(status) {
  return (status?.groups ?? []).map(({ id, name, channels }) => ({
    id,
    name,
    channels: channels.map((channel) => channel + 1),
  }));
}

// A copy of `table` with at least `count` entries and room for input `index`.
function editable(table, count, index) {
  const length = Math.max(table.length, count, index + 1);
  return Array.from({ length }, (_, entry) => detailOf(table[entry] ?? defaultChannel()));
}

function checked(table) {
  checkChannels(table);
  return table;
}

function unitOf(table, index) {
  return pairUnits(table).find((unit) => unit.includes(index));
}

/** `table` with input `index` named `name` (trimmed; empty for none), for a device of `count`. */
export function renameChannel(table, index, name, count) {
  const next = editable(table, count, index);
  next[index].name = String(name ?? "").trim();
  return checked(next);
}

/** `table` with input `index` of instrument `kind`. */
export function setChannelKind(table, index, kind, count) {
  const next = editable(table, count, index);
  next[index].kind = kind;
  return checked(next);
}

/** `table` with input `index` hidden or shown: both sides of a pair go together. */
export function setChannelHidden(table, index, hidden, count) {
  const next = editable(table, count, index);
  for (const side of unitOf(next, index)) {
    next[side].hidden = Boolean(hidden);
  }
  return checked(next);
}

/**
 * `table` with input `index` paired with the next input (`linked`), or with the pair it is in,
 * on either side, undone.
 */
export function setChannelLink(table, index, linked, count) {
  const next = editable(table, count, index);
  const unit = unitOf(next, index);
  if (!linked) {
    next[unit[0]].link = false;
  } else if (unit.length === 1) {
    next[index].link = true;
  }
  return checked(next);
}

/** `groups` with a new, empty group at the end, named "Group 3" or the next free number. */
export function addGroup(groups) {
  if (groups.length >= MAX_GROUPS) {
    throw new Error(`there can be at most ${MAX_GROUPS} groups`);
  }
  const taken = new Set(groups.map((group) => group.name.trim().toLowerCase()));
  let number = groups.length + 1;
  while (taken.has(`group ${number}`)) {
    number += 1;
  }
  return [...copies(groups), { name: `Group ${number}`, channels: [] }];
}

function copies(groups) {
  return groups.map((group) => ({ ...group, channels: [...group.channels] }));
}

function requireGroup(groups, id) {
  if (!groups.some((group) => group.id === id)) {
    throw new Error("that group is no longer there");
  }
}

/** `groups` with group `id` called `name` (trimmed, never empty). */
export function renameGroup(groups, id, name) {
  requireGroup(groups, id);
  const trimmed = String(name ?? "").trim();
  if (!trimmed) {
    throw new Error("a group needs a name");
  }
  return copies(groups).map((group) => (group.id === id ? { ...group, name: trimmed } : group));
}

/**
 * `groups` with the inputs `numbers` (1-based: one input, or both sides of a pair) in group `id`
 * (`member`) or out of it. An input is in one group at most, so joining one group leaves any other.
 */
export function setGroupMembers(groups, id, numbers, member) {
  requireGroup(groups, id);
  const moving = new Set(numbers);
  return copies(groups).map((group) => {
    const kept = group.channels.filter((channel) => !moving.has(channel));
    if (group.id === id && member) {
      return { ...group, channels: [...kept, ...numbers].sort((a, b) => a - b) };
    }
    return group.id === id || member ? { ...group, channels: kept } : group;
  });
}

/** `groups` without group `id`. */
export function deleteGroup(groups, id) {
  requireGroup(groups, id);
  return copies(groups).filter((group) => group.id !== id);
}

function unitLabel(channels, unit) {
  const label = channelLabel(channels[unit[0]]);
  return unit.length === 2 ? `${label} (stereo)` : label;
}

function choiceLabel(channels, unit) {
  const range = unit.length === 2 ? `CH ${unit[0] + 1}-${unit[1] + 1}` : input(unit[0]);
  const name = String(channels[unit[0]].name ?? "").trim();
  return name ? `${range} ${name}` : range;
}

/**
 * One card per group, in the engine's order: `{ id, name, tone, invalid, members, count, choices }`.
 * `members` names the inputs in it (a pair once, an input the device lacks as such) and `choices`
 * offers every input of the device, a pair once, as `{ key, numbers, label, checked, owner,
 * hidden }`, where `owner` names the other group an input is in (choosing it moves it).
 */
export function groupCards(state) {
  const status = state.engine.status;
  const channels = deviceChannels(status);
  const units = pairUnits(channels);
  const groups = tonedGroups(status);
  return groups.map((group) => {
    const inGroup = (unit) => unit.some((index) => group.channels.includes(index));
    const members = units.filter(inGroup).map((unit) => unitLabel(channels, unit));
    for (const index of group.channels) {
      if (index >= channels.length) {
        members.push(`${input(index)} (not on this device)`);
      }
    }
    const choices = units.map((unit) => {
      const owner = groups.find(
        (other) => other.id !== group.id && unit.some((index) => other.channels.includes(index)),
      );
      return {
        key: String(unit[0] + 1),
        numbers: unit.map((index) => index + 1),
        label: choiceLabel(channels, unit),
        checked: inGroup(unit),
        owner: owner?.name ?? null,
        hidden: Boolean(channels[unit[0]].hidden),
      };
    });
    return {
      id: group.id,
      name: group.name,
      tone: group.tone,
      invalid: Boolean(group.invalid),
      members,
      count: `${members.length} ${members.length === 1 ? "channel" : "channels"}`,
      choices,
    };
  });
}

/** Whether another group can be made: the engine takes at most 16. */
export function canAddGroup(state) {
  return (state.engine.status?.groups ?? []).length < MAX_GROUPS;
}
