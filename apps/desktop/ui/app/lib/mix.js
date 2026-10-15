// A musician's mix: reading it from the engine's payloads, showing local edits over it, and turning
// those edits into a `set_mix` request. Pure functions, no DOM.
//
// A mix is `{ channelGains, pans, channelMuted, channelSolo, groupLevels, volumePercent,
// maxLevelPercent, muted }`, the camelCase `MixDto` of the desktop's JSON contract. The tables
// always hold the engine's full 32 channel and 16 group entries.
//
// A local edit is keyed by field, plus the entry for a table: "volumePercent", "channelGains.3".
// The edits waiting for the engine's answer are a plain object of key to value.

import { clampLevel } from "./values.js";

export const MAX_CHANNELS = 32;
export const MAX_GROUPS = 16;

const SCALARS = Object.freeze(["volumePercent", "maxLevelPercent", "muted"]);
const TABLES = Object.freeze({
  channelGains: MAX_CHANNELS,
  pans: MAX_CHANNELS,
  channelMuted: MAX_CHANNELS,
  channelSolo: MAX_CHANNELS,
  groupLevels: MAX_GROUPS,
});
const FLAGS = new Set(["muted", "channelMuted", "channelSolo"]);
const FIELDS = new Set([...SCALARS, ...Object.keys(TABLES)]);

/** The names of a mix's fields, as they appear flat on a status row. */
export const MIX_FIELDS = Object.freeze([...FIELDS]);

/** The mix the engine starts every musician with: unity levels, even channels left, odd right. */
export function defaultMix() {
  return {
    channelGains: new Array(MAX_CHANNELS).fill(100),
    pans: Array.from({ length: MAX_CHANNELS }, (_, index) => (index % 2 === 0 ? 0 : 100)),
    channelMuted: new Array(MAX_CHANNELS).fill(false),
    channelSolo: new Array(MAX_CHANNELS).fill(false),
    groupLevels: new Array(MAX_GROUPS).fill(100),
    volumePercent: 100,
    maxLevelPercent: 100,
    muted: false,
  };
}

/**
 * The mix in a payload: a status row (where the mix sits flat beside the other fields) or the
 * `mix` of a `mix-changed` event. The tables are copied, so the mix owns them.
 */
export function mixFromDto(source) {
  const mix = {};
  for (const field of SCALARS) {
    mix[field] = source[field];
  }
  for (const field of Object.keys(TABLES)) {
    mix[field] = Array.isArray(source[field]) ? [...source[field]] : [];
  }
  return mix;
}

/** The key of one edit: `editKey("volumePercent")`, `editKey("channelGains", 3)`. */
export function editKey(field, index = null) {
  const key = index === null ? field : `${field}.${index}`;
  parseEditKey(key);
  return key;
}

/**
 * `{ field, index }` for an edit key; `index` is `null` for a whole-mix field. A key the engine
 * would not accept throws rather than being sent and silently changing nothing.
 */
export function parseEditKey(key) {
  const [field, entry, ...rest] = String(key).split(".");
  if (rest.length > 0 || !FIELDS.has(field)) {
    throw new Error(`unknown mix edit: ${key}`);
  }
  if (SCALARS.includes(field)) {
    if (entry !== undefined) {
      throw new Error(`mix edit ${key}: ${field} has no entries`);
    }
    return { field, index: null };
  }
  const index = /^\d+$/.test(entry ?? "") ? Number(entry) : NaN;
  if (!(index < TABLES[field])) {
    throw new Error(`mix edit ${key}: ${field} needs an entry from 0 to ${TABLES[field] - 1}`);
  }
  return { field, index };
}

// A flag as a boolean, a level as the whole 0-100 number the engine's integer fields take.
function cleanValue(field, value) {
  return FLAGS.has(field) ? Boolean(value) : clampLevel(value);
}

/** The mix as it shows with `edits` on top. `mix` is left as it was. */
export function applyEdits(mix, edits) {
  const view = { ...mix };
  for (const [key, value] of Object.entries(edits ?? {})) {
    const { field, index } = parseEditKey(key);
    if (index === null) {
      view[field] = value;
    } else {
      if (view[field] === mix[field]) {
        view[field] = [...mix[field]];
      }
      view[field][index] = value;
    }
  }
  return view;
}

/**
 * The `set_mix` request that carries `edits`, or `null` when there are none.
 *
 * Only edited fields are sent. The engine replaces a table from its first entry, so a change to one
 * channel sends the table from channel 0 up to that channel, the earlier entries as `mix` (with the
 * other edits) shows them. Levels go out as whole numbers from 0 to 100, which the engine's integer
 * fields require.
 */
export function buildMixRequest(mix, edits) {
  const keys = Object.keys(edits ?? {});
  if (keys.length === 0) {
    return null;
  }
  const view = applyEdits(mix, edits);
  const reach = {};
  for (const key of keys) {
    const { field, index } = parseEditKey(key);
    reach[field] = index === null ? null : Math.max(reach[field] ?? 0, index);
  }
  const request = {};
  for (const [field, index] of Object.entries(reach)) {
    request[field] =
      index === null
        ? cleanValue(field, view[field])
        : view[field].slice(0, index + 1).map((value) => cleanValue(field, value));
  }
  return request;
}

/**
 * What the engine stores when it merges a `set_mix` request into `mix`: each table is replaced
 * from its first entry and keeps the rest, levels are clamped to 0-100, absent fields keep their
 * values, and an unknown field or a table longer than the mix is refused. It mirrors the engine's
 * `MixUpdate::merge_into` so the fake bridge and the tests answer as the engine does.
 */
export function mergeMixUpdate(mix, update) {
  for (const field of Object.keys(update)) {
    if (!FIELDS.has(field)) {
      throw new Error(`unknown field \`${field}\` in the mix update`);
    }
  }
  const merged = mixFromDto(mix);
  for (const field of SCALARS) {
    if (update[field] !== undefined) {
      merged[field] = cleanValue(field, update[field]);
    }
  }
  for (const [field, size] of Object.entries(TABLES)) {
    const values = update[field];
    if (values === undefined) {
      continue;
    }
    if (values.length > size) {
      throw new Error(`${field} has ${values.length} entries; the mix holds ${size}`);
    }
    values.forEach((value, index) => {
      merged[field][index] = cleanValue(field, value);
    });
  }
  return merged;
}

/**
 * The edits still waiting after the engine answered a request that carried `sent`: an edit is
 * settled when its value is still the one sent, and kept when it changed since or was not sent.
 */
export function settleEdits(edits, sent) {
  const remaining = {};
  for (const [key, value] of Object.entries(edits ?? {})) {
    if (!(key in sent) || sent[key] !== value) {
      remaining[key] = value;
    }
  }
  return remaining;
}
