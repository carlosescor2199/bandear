// Presets, copies and mirrors of musicians' mixes: what the Mix screen offers and how it words the
// desk's answers, and the rules the engine keeps, which the fake bridge keeps too. Pure functions,
// no DOM.
//
// A preset is `{ id, name, mix: { channels: [{ gain, pan, muted, solo }], groups: { <groupId>:
// level } } }`: each input's values in channel order (the first entry is CH 1) and the group levels
// by group id, never a musician's master volume, level cap or the engineer's controls. A copy and a
// mirror carry the same values from one live mix to another, every channel and group in place. A
// status row's `mirror` is `{ leader, leaderAddress, name }` when its musician's mix follows
// another's: the leader's key, their address (null while they are away) and the name the
// follower's phone is told.

import { keepPairsEqual } from "./channels.js";
import { musicianByAddress } from "./console-state.js";
import { musicianLabel, sentenceWithReason } from "./format.js";

/** The longest preset name the desk keeps, in characters. */
export const MAX_PRESET_NAME_LENGTH = 40;

const SHARED_TABLES = Object.freeze(["channelGains", "pans", "channelMuted", "channelSolo", "groupLevels"]);

/**
 * `name` trimmed, when it can name a preset among `presets` (the one with the id `renaming` aside),
 * else an Error, as the desk refuses it: no name, more than MAX_PRESET_NAME_LENGTH characters, or
 * another preset's name in any case.
 */
export function checkPresetName(name, presets, renaming = null) {
  const trimmed = String(name ?? "").trim();
  if (!trimmed) {
    throw new Error("a preset needs a name");
  }
  if ([...trimmed].length > MAX_PRESET_NAME_LENGTH) {
    throw new Error(`a preset name is at most ${MAX_PRESET_NAME_LENGTH} characters`);
  }
  const lower = trimmed.toLowerCase();
  if (presets.some((preset) => preset.id !== renaming && preset.name.toLowerCase() === lower)) {
    throw new Error(`a preset named "${trimmed}" already exists`);
  }
  return trimmed;
}

/**
 * What a preset keeps of `mix` on the engine of `status`: each of the device's inputs, and every
 * group level by the group's id.
 */
export function presetOf(mix, status) {
  const count = Math.min(status.sourceChannels, mix.channelGains.length);
  return {
    channels: Array.from({ length: count }, (_, index) => ({
      gain: mix.channelGains[index],
      pan: mix.pans[index],
      muted: mix.channelMuted[index],
      solo: mix.channelSolo[index],
    })),
    groups: Object.fromEntries(
      (status.groups ?? []).map((group, position) => [group.id, mix.groupLevels[position]]),
    ),
  };
}

/**
 * `mix` with `preset` (its `mix`) on it, as the engine puts it there: each kept input up to the
 * mix's channels, and the level of every live group (`groups`, in their order) the preset names by
 * id; an input or group the preset does not name keeps its value, and so do the master, the cap and
 * the engineer's controls. Both sides of a pair in `details` take the first side's level, mute and
 * solo (its pans stay the preset's).
 */
export function withPreset(mix, preset, groups, details) {
  const next = structuredClone(mix);
  const channels = preset.channels.slice(0, next.channelGains.length);
  channels.forEach((channel, index) => {
    next.channelGains[index] = channel.gain;
    next.pans[index] = channel.pan;
    next.channelMuted[index] = Boolean(channel.muted);
    next.channelSolo[index] = Boolean(channel.solo);
  });
  details.forEach((detail, first) => {
    if (detail.link && first + 1 < channels.length) {
      for (const field of ["channelGains", "channelMuted", "channelSolo"]) {
        next[field][first + 1] = next[field][first];
      }
    }
  });
  groups.forEach((group, position) => {
    if (Object.hasOwn(preset.groups, group.id)) {
      next.groupLevels[position] = preset.groups[group.id];
    }
  });
  return keepPairsEqual(mix, next, details);
}

/**
 * `mix` with `leader`'s channels and group levels, in place, as a copy and a mirror carry them: the
 * master, the cap and the engineer's controls stay `mix`'s.
 */
export function withShared(mix, leader) {
  const next = { ...mix };
  for (const field of SHARED_TABLES) {
    next[field] = [...leader[field]];
  }
  return next;
}

/** True when `mix` already holds `leader`'s channels and group levels. */
export function sameShared(mix, leader) {
  return SHARED_TABLES.every(
    (field) => JSON.stringify(mix[field]) === JSON.stringify(leader[field]),
  );
}

/**
 * True when the musician at `follower` following the one at `leader` would close a circle among
 * `musicians` (status rows): the leader already follows the follower, directly or through others.
 * Following oneself is a circle too.
 */
export function closesCircle(musicians, follower, leader) {
  const seen = new Set();
  let current = leader;
  while (current && !seen.has(current)) {
    if (current === follower) {
      return true;
    }
    seen.add(current);
    const row = musicians.find((musician) => musician.address === current);
    current = row?.mirror?.leaderAddress ?? null;
  }
  return false;
}

/** "Bass", "Bass and Keys", "Bass, Keys and Drums". */
export function joinNames(names) {
  if (names.length <= 1) {
    return names.join("");
  }
  return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

function labelOf(state, address) {
  const musician = musicianByAddress(state, address);
  return musician ? musicianLabel(musician) : address;
}

/**
 * The Mirror control for `address`'s mix: `{ value, options: [{ value, label }], note }`. `value`
 * is "" for none, the leader's address, or "away" while the leader is not here. Every other
 * musician is offered, except one whose mix follows this one's, directly or through others (a
 * circle is never offered). `note` says in words whom the mix follows, or null.
 */
export function mirrorChoice(state, address) {
  const musician = musicianByAddress(state, address);
  const mirror = musician?.mirror ?? null;
  const options = [{ value: "", label: "Off" }];
  for (const other of state.musicians) {
    if (other.address !== address && !closesCircle(state.musicians, address, other.address)) {
      options.push({ value: other.address, label: musicianLabel(other) });
    }
  }
  if (!mirror) {
    return { value: "", options, note: null };
  }
  const name = musician ? musicianLabel(musician) : address;
  if (mirror.leaderAddress === null) {
    options.push({ value: "away", label: `${mirror.name} (not here)` });
    return {
      value: "away",
      options,
      note:
        `${name} follows ${mirror.name}'s mix. ${mirror.name} is not here, so ${name}'s mix stays ` +
        `as ${mirror.name} left it until they are back.`,
    };
  }
  return {
    value: mirror.leaderAddress,
    options,
    note: `${name} follows ${labelOf(state, mirror.leaderAddress)}'s mix: its channels and groups move with it, its master stays its own.`,
  };
}

/**
 * The musicians a change from `address`'s tools can reach, in the status's order: `{ address,
 * label, disabled, note }`. With `includeSelf` the open musician is first ("this mix"). A musician
 * whose mix follows another's cannot take a preset or a copy (the engine refuses it: stop the
 * mirror first), so it is offered disabled with a note saying whom it follows.
 */
export function mixTargets(state, address, { includeSelf = false } = {}) {
  return state.musicians
    .filter((musician) => includeSelf || musician.address !== address)
    .map((musician) => {
      const following = musician.mirror ? `follows ${musician.mirror.name}` : null;
      let note = following;
      if (musician.address === address) {
        note = following ? `this mix, ${following}` : "this mix";
      }
      return {
        address: musician.address,
        label: musicianLabel(musician),
        disabled: Boolean(musician.mirror),
        note,
      };
    })
    .sort((one, other) => Number(other.address === address) - Number(one.address === address));
}

/**
 * What the Mix screen's tools show for `address`: `{ name, mirror, copyTargets, presetTargets,
 * presets, canUse }`. `presets` is `[{ id, name }]`, or null until the desk's list was read;
 * `canUse` is false while no engine runs (presets are saved from, and put on, mixes the engine
 * plays; a mirror can still be kept for the next start).
 */
export function mixToolsView(state, address) {
  return {
    name: labelOf(state, address),
    mirror: mirrorChoice(state, address),
    copyTargets: mixTargets(state, address),
    presetTargets: mixTargets(state, address, { includeSelf: true }),
    presets: state.presets?.map(({ id, name }) => ({ id, name })) ?? null,
    canUse: state.engine.status !== null,
  };
}

/**
 * What a mirror change did, from the `set_mirror` answer `{ leader, applied, saved, saveError }`,
 * naming the follower and the leader: followed now, stopped, kept for the next start, or done but
 * not kept.
 */
export function formatMirrorSet({ leader, applied, saved, saveError }, follower, leaderName) {
  let done;
  if (leader === null) {
    done = applied
      ? `${follower} no longer follows another mix; their mix stays as it is`
      : `${follower} will follow no other mix from the next start`;
  } else {
    done = applied
      ? `${follower} follows ${leaderName}'s mix now`
      : `${follower} will follow ${leaderName}'s mix from the next start`;
  }
  if (saved) {
    return `${done}.`;
  }
  return sentenceWithReason(`${done}, but this was not saved`, saveError);
}

/** What a copy did: "Copied Ana's mix to Bass and Keys: channels, pans, mutes, solos and groups." */
export function formatCopied(from, names) {
  return `Copied ${from}'s mix to ${joinNames(names)}: channels, pans, mutes, solos and group levels.`;
}

/** What a preset put on musicians did: 'Applied "Verse" to Bass and Keys.' */
export function formatPresetApplied(preset, names) {
  return `Applied "${preset}" to ${joinNames(names)}.`;
}
