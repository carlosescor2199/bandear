// The shared reverb and delay, as the console shows and changes them. No DOM.
//
// The desk has one reverb and one delay on two buses. Every input sends to each at its own level
// (0-100%, kept with its strip: `settings.sends = { reverb, delay }`), after its strip and before
// any fader; every mix (each musician's and the Local mix) returns each at its own level (0-100%),
// scaled by its master volume. The returns are engineer-only: no phone sets them. The effects add
// no latency to the dry mix.
//
// The desk answers:
// - a send with the processing answer `{ outcome, channels, message }` (formatProcessingSet);
// - `set_effect` with `{ outcome, effect, effects, saved, saveError, message }` (formatEffectSet);
// - a return with `{ musicianKey, outcome, effect, returns, saved, saveError, message }`
//   (formatReturnSet).
// The status carries `effects`, `localFxReturns` and each row's `fxReturns`.

import { LOCAL_MIX, musicianByAddress } from "./console-state.js";
import { sentenceWithReason } from "./format.js";

/** The two effects, in the desk's order. */
export const EFFECTS = Object.freeze(["reverb", "delay"]);

/** An effect's name in words, capitalised. */
export function effectLabel(effect) {
  return effect === "delay" ? "Delay" : "Reverb";
}

/** The ranges the desk holds each setting to. */
export const EFFECT_RANGES = Object.freeze({
  sizePercent: Object.freeze([0, 100]),
  dampingPercent: Object.freeze([0, 100]),
  preDelayMs: Object.freeze([0, 200]),
  widthPercent: Object.freeze([0, 100]),
  timeMs: Object.freeze([1, 2000]),
  tempoBpm: Object.freeze([30, 300]),
  feedbackPercent: Object.freeze([0, 90]),
  highCutHz: Object.freeze([1000, 20000]),
});

/** The note values the delay follows at a tempo, each with its length in beats and its words. */
export const DELAY_NOTES = Object.freeze([
  Object.freeze({ value: "half", beats: 2, label: "1/2" }),
  Object.freeze({ value: "quarter", beats: 1, label: "1/4" }),
  Object.freeze({ value: "dottedEighth", beats: 0.75, label: "Dotted 1/8" }),
  Object.freeze({ value: "quarterTriplet", beats: 2 / 3, label: "1/4 triplet" }),
  Object.freeze({ value: "eighth", beats: 0.5, label: "1/8" }),
  Object.freeze({ value: "eighthTriplet", beats: 1 / 3, label: "1/8 triplet" }),
  Object.freeze({ value: "sixteenth", beats: 0.25, label: "1/16" }),
]);

/** Both effects' defaults, as the desk starts them. */
export function defaultEffects() {
  return {
    reverb: { sizePercent: 50, dampingPercent: 40, preDelayMs: 10, widthPercent: 100 },
    delay: {
      timeMs: 375,
      tempoBpm: 0,
      note: "quarter",
      feedbackPercent: 35,
      highCutHz: 6000,
      pingPong: false,
    },
  };
}

// `value` held in `range`; one that is not a number takes `fallback`.
function within(value, [low, high], fallback) {
  const number = Number(value);
  if (value === null || value === undefined || value === "" || Number.isNaN(number)) {
    return fallback;
  }
  return Math.min(Math.max(number, low), high);
}

/** The reverb's settings held in range, a missing or unreadable number taking the default. */
export function clampReverb(settings) {
  const fallback = defaultEffects().reverb;
  return Object.fromEntries(
    Object.entries(fallback).map(([key, base]) => [key, within(settings?.[key], EFFECT_RANGES[key], base)]),
  );
}

/** A tempo as the desk keeps it: 0 (none) under its range, held to 300 bpm. */
export function clampTempo(value) {
  const number = Number(value);
  if (value === null || value === undefined || value === "" || !Number.isFinite(number)) {
    return 0;
  }
  return number < EFFECT_RANGES.tempoBpm[0] ? 0 : Math.min(number, EFFECT_RANGES.tempoBpm[1]);
}

/** The delay's settings held in range; an unknown note is a quarter. */
export function clampDelay(settings) {
  const fallback = defaultEffects().delay;
  const note = DELAY_NOTES.some((entry) => entry.value === settings?.note) ? settings.note : fallback.note;
  return {
    timeMs: within(settings?.timeMs, EFFECT_RANGES.timeMs, fallback.timeMs),
    tempoBpm: clampTempo(settings?.tempoBpm),
    note,
    feedbackPercent: within(settings?.feedbackPercent, EFFECT_RANGES.feedbackPercent, fallback.feedbackPercent),
    highCutHz: within(settings?.highCutHz, EFFECT_RANGES.highCutHz, fallback.highCutHz),
    pingPong: Boolean(settings?.pingPong),
  };
}

/** Both effects held in range. */
export function clampEffects(effects) {
  return { reverb: clampReverb(effects?.reverb), delay: clampDelay(effects?.delay) };
}

/** The time the delay repeats at, in ms: the note at the tempo when one is set, held to 1-2,000. */
export function delayTimeMs(delay) {
  const held = clampDelay(delay);
  const time =
    held.tempoBpm > 0
      ? (60000 / held.tempoBpm) * DELAY_NOTES.find((entry) => entry.value === held.note).beats
      : held.timeMs;
  return Math.min(Math.max(time, EFFECT_RANGES.timeMs[0]), EFFECT_RANGES.timeMs[1]);
}

/** A level as the desk keeps it: a whole number 0-100. */
export function clampLevel(value) {
  return Math.round(within(value, [0, 100], 0));
}

/** Levels `{ reverb, delay }` held to 0-100. */
export function clampLevels(levels) {
  return { reverb: clampLevel(levels?.reverb), delay: clampLevel(levels?.delay) };
}

/** A percentage in words: "40%", "Off" at 0. */
export function formatLevel(level) {
  const value = clampLevel(level);
  return value === 0 ? "Off" : `${value}%`;
}

/** A time in words: "375 ms", "1.25 s". */
export function formatTime(ms) {
  const value = Number(ms) || 0;
  return value >= 1000 ? `${Math.round(value / 10) / 100} s` : `${Math.round(value)} ms`;
}

/** A frequency in words: "6 kHz", "12.5 kHz". */
export function formatKhz(hz) {
  return `${Math.round((Number(hz) || 0) / 100) / 10} kHz`;
}

/**
 * The tempo typed in the delay's tempo field: `{ ok: true, tempoBpm }` (0 for an empty field, no
 * tempo) or `{ ok: false, message }` for text that is not a tempo in range.
 */
export function readTempo(text) {
  const trimmed = String(text ?? "").trim();
  if (trimmed === "") {
    return { ok: true, tempoBpm: 0 };
  }
  const number = Number(trimmed.replace(",", "."));
  const [low, high] = EFFECT_RANGES.tempoBpm;
  if (!Number.isFinite(number) || number < low || number > high) {
    return { ok: false, message: `A tempo is ${low} to ${high} bpm, or empty for none.` };
  }
  return { ok: true, tempoBpm: Math.round(number * 10) / 10 };
}

/**
 * What the effects panel shows: both effects (the change on its way over the status's) with every
 * value in words. `{ reverb: { ...settings, size, damping, preDelay, width }, delay: { ...settings,
 * time, timeInForce, feedback, highCut, synced, noteLabel }, running }`.
 */
export function effectsView(state) {
  const status = state.engine?.status;
  const effects = clampEffects({
    reverb: state.effectsPending?.reverb ?? status?.effects?.reverb,
    delay: state.effectsPending?.delay ?? status?.effects?.delay,
  });
  const delayTime = delayTimeMs(effects.delay);
  const synced = effects.delay.tempoBpm > 0;
  const note = DELAY_NOTES.find((entry) => entry.value === effects.delay.note);
  return {
    running: state.engine?.phase === "running" && Boolean(status),
    reverb: {
      ...effects.reverb,
      size: `${Math.round(effects.reverb.sizePercent)}%`,
      damping: `${Math.round(effects.reverb.dampingPercent)}%`,
      preDelay: formatTime(effects.reverb.preDelayMs),
      width: `${Math.round(effects.reverb.widthPercent)}%`,
    },
    delay: {
      ...effects.delay,
      time: formatTime(effects.delay.timeMs),
      timeInForce: synced ? `${formatTime(delayTime)} (${note.label} at ${effects.delay.tempoBpm} bpm)` : formatTime(delayTime),
      feedback: `${Math.round(effects.delay.feedbackPercent)}%`,
      highCut: formatKhz(effects.delay.highCutHz),
      synced,
      noteLabel: note.label,
    },
  };
}

/** The sends of channel `channel` (1-based): the change on its way, else the strip's, else none. */
export function channelSends(state, channel) {
  const strip = state.processing?.find((entry) => entry.channel === channel)?.settings;
  const kept = clampLevels(strip?.sends);
  const waiting = state.sendsPending?.[channel] ?? {};
  return { ...kept, ...waiting };
}

/**
 * The return levels of the mix `address` (a musician's address, or LOCAL_MIX): the change on its
 * way, else what the status reports, else none. `{ levels, pending }`.
 */
export function mixReturns(state, address) {
  const waiting = state.returnsPending?.[address];
  const reported =
    address === LOCAL_MIX ? state.engine?.status?.localFxReturns : musicianByAddress(state, address)?.fxReturns;
  const levels = { ...clampLevels(reported), ...(waiting ?? {}) };
  return { levels, pending: Boolean(waiting) };
}

/** What a master strip shows of its mix's returns: one row per effect, `{ effect, label, level, text }`. */
export function returnsView(state, address) {
  const { levels, pending } = mixReturns(state, address);
  return {
    pending,
    rows: EFFECTS.map((effect) => ({
      effect,
      label: `${effectLabel(effect)} return`,
      level: levels[effect],
      text: formatLevel(levels[effect]),
    })),
  };
}

/** The desk's answer to an effect change, in words: `{ ok, message }`. */
export function formatEffectSet(answer) {
  if (answer?.outcome !== "set") {
    return { ok: false, message: answer?.message ?? "The effect was not set." };
  }
  let message = answer.message ?? `The ${answer.effect} is set.`;
  if (answer.saveError) {
    message += ` ${sentenceWithReason("It was not saved for the next start", answer.saveError)}`;
  }
  return { ok: true, message };
}

/** The desk's answer to a return change for `name`'s mix ("Ana", or "The local mix"), in words. */
export function formatReturnSet(answer, name) {
  if (answer?.outcome !== "set") {
    return { ok: false, message: answer?.message ?? `${name}'s return was not set.` };
  }
  const levels = clampLevels(answer.returns);
  const level = levels[answer.effect] ?? 0;
  const whose = name === "The local mix" ? "The local mix" : `${name}'s mix`;
  let message =
    level === 0 ? `${whose} returns no ${answer.effect}.` : `${whose} returns the ${answer.effect} at ${level}%.`;
  if (answer.saveError) {
    message += ` ${sentenceWithReason("It was not saved for the next start", answer.saveError)}`;
  }
  return { ok: true, message };
}
