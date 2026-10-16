// A musician's mix: reading it from the engine's payloads, showing local edits over it, and turning
// those edits into a `set_mix` request (and the engineer's into a `set_engineer_controls` one).
// Pure functions, no DOM.
//
// A mix is `{ revision, channelGains, pans, channelMuted, channelSolo, groupLevels,
// volumePercent, maxLevelPercent, muted, engineerLimitPercent, lockedChannels }`, the camelCase
// `MixDto` of the desktop's JSON contract. The tables always hold the engine's full 32 channel and
// 16 group entries. `revision` is the engine's version of the stored mix: every change has a higher
// one, so of two copies the higher revision is the newer. It is never edited or sent.
//
// `engineerLimitPercent` and `lockedChannels` are the engineer's controls (ENGINEER_FIELDS): a cap
// on what the musician hears apart from the phone's own `maxLevelPercent` (the engine plays at most
// the lower of the two), and the channels the musician's phone cannot change. They travel with the
// mix and its revision, but only `set_engineer_controls` changes them, never `set_mix`.
//
// A local edit is keyed by field, plus the entry for a table: "volumePercent", "channelGains.3",
// "lockedChannels.0". The edits waiting for the engine's answer are a plain object of key to value.

import { clampLevel } from "./values.js";

export const MAX_CHANNELS = 32;
export const MAX_GROUPS = 16;

const MIX_SCALARS = Object.freeze(["volumePercent", "maxLevelPercent", "muted"]);
const MIX_TABLES = Object.freeze({
  channelGains: MAX_CHANNELS,
  pans: MAX_CHANNELS,
  channelMuted: MAX_CHANNELS,
  channelSolo: MAX_CHANNELS,
  groupLevels: MAX_GROUPS,
});
const SCALARS = Object.freeze([...MIX_SCALARS, "engineerLimitPercent"]);
const TABLES = Object.freeze({ ...MIX_TABLES, lockedChannels: MAX_CHANNELS });
const FLAGS = new Set(["muted", "channelMuted", "channelSolo", "lockedChannels"]);
const FIELDS = new Set([...SCALARS, ...Object.keys(TABLES)]);
/** What `set_mix` takes: every field but the engineer's controls. */
const MIX_UPDATE_FIELDS = new Set([...MIX_SCALARS, ...Object.keys(MIX_TABLES)]);

/** The engineer's controls in a mix, which only `set_engineer_controls` changes. */
export const ENGINEER_FIELDS = Object.freeze(["engineerLimitPercent", "lockedChannels"]);

/** The names of a mix's fields, as they appear flat on a status row. */
export const MIX_FIELDS = Object.freeze(["revision", ...FIELDS]);

/**
 * The mix the engine starts every musician with: unity levels, even channels left, odd right, no
 * limit from the engineer and nothing locked.
 */
export function defaultMix() {
  return {
    revision: 0,
    channelGains: new Array(MAX_CHANNELS).fill(100),
    pans: Array.from({ length: MAX_CHANNELS }, (_, index) => (index % 2 === 0 ? 0 : 100)),
    channelMuted: new Array(MAX_CHANNELS).fill(false),
    channelSolo: new Array(MAX_CHANNELS).fill(false),
    groupLevels: new Array(MAX_GROUPS).fill(100),
    volumePercent: 100,
    maxLevelPercent: 100,
    muted: false,
    engineerLimitPercent: 100,
    lockedChannels: new Array(MAX_CHANNELS).fill(false),
  };
}

/**
 * The mix in a payload: a status row (where the mix sits flat beside the other fields) or the
 * `mix` of a `mix-changed` event. The tables are copied, so the mix owns them. A payload from a desk
 * that predates the engineer's controls reads as no limit and nothing locked.
 */
export function mixFromDto(source) {
  // A payload without a revision reads as the oldest copy there is.
  const mix = { revision: Number.isInteger(source.revision) ? source.revision : 0 };
  for (const field of MIX_SCALARS) {
    mix[field] = source[field];
  }
  for (const field of Object.keys(MIX_TABLES)) {
    mix[field] = Array.isArray(source[field]) ? [...source[field]] : [];
  }
  mix.engineerLimitPercent = Number.isFinite(source.engineerLimitPercent)
    ? source.engineerLimitPercent
    : 100;
  mix.lockedChannels = Array.isArray(source.lockedChannels)
    ? [...source.lockedChannels]
    : new Array(MAX_CHANNELS).fill(false);
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

/** True for an edit of the engineer's controls (ENGINEER_FIELDS), which `set_mix` never carries. */
export function isEngineerEdit(key) {
  return ENGINEER_FIELDS.includes(parseEditKey(key).field);
}

/**
 * `edits` in two: `mix`, the edits `set_mix` carries, and `engineer`, the edits of the engineer's
 * controls that `set_engineer_controls` carries.
 */
export function splitEdits(edits) {
  const mix = {};
  const engineer = {};
  for (const [key, value] of Object.entries(edits ?? {})) {
    (isEngineerEdit(key) ? engineer : mix)[key] = value;
  }
  return { mix, engineer };
}

// The request fields `edits` reach, as the engine replaces them: a scalar on its own, a table from
// its first entry up to the furthest edited one, values as `mix` with the edits shows them.
function reachedFields(mix, edits) {
  const view = applyEdits(mix, edits);
  const reach = {};
  for (const key of Object.keys(edits)) {
    const { field, index } = parseEditKey(key);
    reach[field] = index === null ? null : Math.max(reach[field] ?? 0, index);
  }
  const fields = {};
  for (const [field, index] of Object.entries(reach)) {
    fields[field] =
      index === null
        ? cleanValue(field, view[field])
        : view[field].slice(0, index + 1).map((value) => cleanValue(field, value));
  }
  return fields;
}

/**
 * The `set_mix` request that carries `edits`, or `null` when there are none. Edits of the
 * engineer's controls are not mix edits and are left out (see buildEngineerRequest).
 *
 * Only edited fields are sent. The engine replaces a table from its first entry, so a change to one
 * channel sends the table from channel 0 up to that channel, the earlier entries as `mix` (with the
 * other edits) shows them. Levels go out as whole numbers from 0 to 100, which the engine's integer
 * fields require.
 */
export function buildMixRequest(mix, edits) {
  const { mix: mixEdits } = splitEdits(edits);
  return Object.keys(mixEdits).length === 0 ? null : reachedFields(mix, mixEdits);
}

/**
 * The `set_engineer_controls` request that carries the engineer's edits among `edits` (the others
 * are left out), or `null` when there are none: `{ limitPercent?, lockedChannels? }`, the locks
 * from CH 1 up to the furthest edited channel, as `mix` with the edits shows them.
 */
export function buildEngineerRequest(mix, edits) {
  const { engineer } = splitEdits(edits);
  if (Object.keys(engineer).length === 0) {
    return null;
  }
  const fields = reachedFields(mix, engineer);
  const request = {};
  if (fields.engineerLimitPercent !== undefined) {
    request.limitPercent = fields.engineerLimitPercent;
  }
  if (fields.lockedChannels !== undefined) {
    request.lockedChannels = fields.lockedChannels;
  }
  return request;
}

const ENGINEER_REQUEST_FIELDS = Object.freeze(["limitPercent", "lockedChannels"]);

/**
 * The engineer's controls once a `set_engineer_controls` request is merged into `mix`, as the
 * engine merges it: `{ engineerLimitPercent, lockedChannels }`, the limit clamped to 0-100 and each
 * listed lock replacing the channel's from CH 1 on. An unknown field or a lock list longer than the
 * mix is refused. Both sides of a pair locking together is keepPairsEqual's (channels.js).
 */
export function mergeEngineerUpdate(mix, update) {
  for (const field of Object.keys(update ?? {})) {
    if (!ENGINEER_REQUEST_FIELDS.includes(field)) {
      throw new Error(
        `unknown field \`${field}\`, expected one of \`${ENGINEER_REQUEST_FIELDS.join("`, `")}\``,
      );
    }
  }
  const locked = [...mix.lockedChannels];
  const listed = update?.lockedChannels ?? [];
  if (listed.length > MAX_CHANNELS) {
    throw new Error(`locked accepts at most ${MAX_CHANNELS} flags, received ${listed.length}`);
  }
  listed.forEach((value, index) => {
    locked[index] = Boolean(value);
  });
  return {
    engineerLimitPercent:
      update?.limitPercent === undefined ? mix.engineerLimitPercent : clampLevel(update.limitPercent),
    lockedChannels: locked,
  };
}

/**
 * What the engine stores when it merges a `set_mix` request into `mix`: each table is replaced
 * from its first entry and keeps the rest, levels are clamped to 0-100, absent fields keep their
 * values, and an unknown field (the engineer's controls included) or a table longer than the mix
 * is refused. It mirrors the engine's `MixUpdate::merge_into` so the fake bridge and the tests
 * answer as the engine does.
 */
export function mergeMixUpdate(mix, update) {
  for (const field of Object.keys(update)) {
    if (!MIX_UPDATE_FIELDS.has(field)) {
      throw new Error(`unknown field \`${field}\` in the mix update`);
    }
  }
  const merged = mixFromDto(mix);
  for (const field of MIX_SCALARS) {
    if (update[field] !== undefined) {
      merged[field] = cleanValue(field, update[field]);
    }
  }
  for (const [field, size] of Object.entries(MIX_TABLES)) {
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
