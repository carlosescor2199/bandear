// The engineer's channel details and the rules the engine keeps for them. Pure functions, no DOM.
//
// A status channel is `{ number, name, kind, hidden, link, group, groupId, level }`: `name` is
// empty when the input has none, `kind` is one of CHANNEL_KINDS, a hidden input is heard in no mix,
// and `link` (on the first input of a pair only) makes it and the next input one stereo strip. The
// details the desk sends (`set_channels`) and keeps are `{ name, kind, hidden, link }`, in channel
// order: the first entry is CH 1.

import { clampLevel } from "./values.js";

/** The engine's instrument kinds, in the order a picker offers them; "other" is the default. */
export const CHANNEL_KINDS = Object.freeze([
  "voice",
  "guitar",
  "bass",
  "keys",
  "drums",
  "percussion",
  "horns",
  "strings",
  "playback",
  "click",
  "other",
]);

/** The longest channel name the engine takes, in characters. */
export const MAX_CHANNEL_NAME_LENGTH = 32;

/** The engine mixes at most this many inputs, so it takes details for at most this many. */
const MAX_CHANNELS = 32;

/** The details of an input the engineer said nothing about. */
export function defaultChannel() {
  return { name: "", kind: "other", hidden: false, link: false };
}

/** What the console calls a status channel: its name, or "CH 3" when it has none. */
export function channelLabel(channel) {
  const name = String(channel.name ?? "").trim();
  return name || `CH ${channel.number}`;
}

/**
 * The two pans a stereo pair's balance writes: balance 0 is hard left, 50 the centre (the full
 * width, pans 0 and 100) and 100 hard right. The first pan is `max(0, 2b - 100)`, the second
 * `min(100, 2b)`, the rule every client of the engine uses.
 */
export function balanceToPans(balance) {
  const level = clampLevel(balance);
  return [Math.max(0, 2 * level - 100), Math.min(100, 2 * level)];
}

/** The balance a pair's two pans show: their mean, which reads back what balanceToPans wrote. */
export function pansToBalance(first, second) {
  return clampLevel((first + second) / 2);
}

/**
 * Refuses the details the engine refuses (its `check_channels`), with an Error naming the input: a
 * kind it does not know, more than 32 entries, a name longer than 32 characters, a pair whose
 * second side is not listed, an input in two pairs, a pair with one side hidden.
 */
export function checkChannels(channels) {
  if (channels.length > MAX_CHANNELS) {
    throw new Error(
      `channel details are supported for at most ${MAX_CHANNELS} channels, received ${channels.length}`,
    );
  }
  channels.forEach((channel, index) => {
    const number = index + 1;
    if (!CHANNEL_KINDS.includes(channel.kind)) {
      throw new Error(`CH ${number} has the instrument kind \`${channel.kind}\`, which is none of ${CHANNEL_KINDS.join(", ")}`);
    }
    if ([...channel.name].length > MAX_CHANNEL_NAME_LENGTH) {
      throw new Error(`the name of CH ${number} is longer than ${MAX_CHANNEL_NAME_LENGTH} characters`);
    }
    if (!channel.link) {
      return;
    }
    const next = channels[index + 1];
    if (!next) {
      throw new Error(`CH ${number} is linked to the next channel, but it is the last one`);
    }
    if (next.link) {
      throw new Error(
        `CH ${number + 1} cannot be in two stereo pairs (CH ${number}-${number + 1} and CH ${number + 1}-${number + 2})`,
      );
    }
    if (Boolean(next.hidden) !== Boolean(channel.hidden)) {
      throw new Error(`CH ${number} and CH ${number + 1} are one stereo pair: hide or show both`);
    }
  });
}

/**
 * The details for a device with `count` inputs, as the engine fits them: kept for the inputs the
 * device has, the defaults for inputs with none, dropped past the last input, and the last input
 * never linked (its pair would be gone). The details given are left as they were.
 */
export function fitChannels(channels, count) {
  const fitted = Array.from({ length: count }, (_, index) => ({
    ...defaultChannel(),
    ...(channels[index] ?? {}),
  }));
  if (fitted.length > 0) {
    fitted[fitted.length - 1].link = false;
  }
  return fitted;
}

const PAIRED_TABLES = Object.freeze(["channelGains", "channelMuted", "channelSolo"]);

/**
 * `merged` with both sides of every linked pair in `channels` at one level, mute and solo, as the
 * engine keeps them after any change from `before`: a side the change moved carries the other with
 * it, and when both sides changed to different values the first channel wins. Pans are left as
 * they are: a pair's balance writes both (see balanceToPans).
 */
export function keepPairsEqual(before, merged, channels) {
  const kept = { ...merged };
  for (const field of PAIRED_TABLES) {
    kept[field] = [...merged[field]];
  }
  channels.forEach((channel, first) => {
    const second = first + 1;
    if (!channel.link || second >= MAX_CHANNELS) {
      return;
    }
    for (const field of PAIRED_TABLES) {
      const table = kept[field];
      if (table[second] !== before[field][second] && table[first] === before[field][first]) {
        table[first] = table[second];
      } else {
        table[second] = table[first];
      }
    }
  });
  return kept;
}
