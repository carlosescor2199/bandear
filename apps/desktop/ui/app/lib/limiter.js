// The master limiter on every mix, as the master strip shows and changes it. No DOM.
//
// Each mix (a musician's, or the engineer's local mix) ends with a peak limiter after its sum and
// before the clamp at the mix's ceiling (the lower of the phone's level cap and the engineer's
// limit). Off by default. Its ceiling is -24 to 0 dBFS: at 0, the default, it limits right at the
// mix's own ceiling, so the clamp never cuts a peak; lower, it limits below it. Its release is 1 to
// 1,000 ms (50 by default). It adds no latency. Engineer-only: no phone sets it.
//
// The desk answers a change with `{ musicianKey, outcome, settings, saved, saveError, message }`
// (formatLimiterSet). A status row carries its mix's `limiter` and the status `localLimiter`; the
// meters carry each mix's `limiterReductionDb` (limiterReduction).

import { LOCAL_MIX, musicianByAddress } from "./console-state.js";
import { masterStrip } from "./mix-view.js";
import { fieldPosition, fieldValue, formatMs, reductionView } from "./processing-view.js";
import { sentenceWithReason } from "./format.js";

/** The limiter's ranges, as the engine holds them. */
export const LIMITER_RANGES = Object.freeze({
  ceilingDb: Object.freeze([-24, 0]),
  releaseMs: Object.freeze([1, 1000]),
});

/** The gain reduction the master strip's meter shows at full scale, in dB. */
export const LIMITER_REDUCTION_SCALE_DB = 12;

/** The limiter every mix starts with: off, its ceiling at 0 dBFS, a 50 ms release. */
export function defaultLimiter() {
  return { enabled: false, ceilingDb: 0, releaseMs: 50 };
}

const clamp = (value, [low, high]) => Math.min(Math.max(value, low), high);
const number = (value, fallback) => (Number.isFinite(Number(value)) && value !== null ? Number(value) : fallback);

/** `settings` held in range, a missing or unreadable number taking the default. */
export function clampLimiter(settings) {
  const fallback = defaultLimiter();
  return {
    enabled: Boolean(settings?.enabled),
    ceilingDb: clamp(number(settings?.ceilingDb, fallback.ceilingDb), LIMITER_RANGES.ceilingDb),
    releaseMs: clamp(number(settings?.releaseMs, fallback.releaseMs), LIMITER_RANGES.releaseMs),
  };
}

/** Whether two limiters are the same settings. */
export function sameLimiter(one, other) {
  return (
    Boolean(one?.enabled) === Boolean(other?.enabled) &&
    one?.ceilingDb === other?.ceilingDb &&
    one?.releaseMs === other?.releaseMs
  );
}

/**
 * The limiter of the mix `address` names (a musician's address, or LOCAL_MIX): the change still on
 * its way, else what the status reports, else the default. `{ settings, pending }`.
 */
export function mixLimiter(state, address) {
  const waiting = state.limiterPending?.[address];
  if (waiting) {
    return { settings: waiting, pending: true };
  }
  const reported =
    address === LOCAL_MIX
      ? state.engine.status?.localLimiter
      : musicianByAddress(state, address)?.limiter;
  return { settings: clampLimiter(reported ?? defaultLimiter()), pending: false };
}

/** How far the limiter of the mix `selected` turns it down now, from a `meters` payload, in dB. */
export function limiterReduction(meters, selected) {
  const output = selected === LOCAL_MIX ? meters?.local : meters?.musicians?.[selected];
  const db = Number(output?.limiterReductionDb);
  return Number.isFinite(db) && db > 0 ? db : 0;
}

/** A ceiling in words: "0 dBFS", "−6.5 dBFS". */
export function formatCeiling(db) {
  const value = Math.round((Number(db) || 0) * 10) / 10;
  if (value === 0) {
    return "0 dBFS";
  }
  const text = Number.isInteger(value) ? String(Math.abs(value)) : Math.abs(value).toFixed(1);
  return `${value < 0 ? "−" : ""}${text} dBFS`;
}

/** The ceiling's slider: 0.5 dB steps from -24 to 0. */
export const CEILING_FIELD = Object.freeze({
  key: "ceilingDb",
  label: "Ceiling",
  range: LIMITER_RANGES.ceilingDb,
  scale: "linear",
  format: formatCeiling,
});

/** The release's slider: a log scale from 1 to 1,000 ms. */
export const RELEASE_FIELD = Object.freeze({
  key: "releaseMs",
  label: "Release",
  range: LIMITER_RANGES.releaseMs,
  scale: "log",
  format: formatMs,
});

/** The slider position (0-1,000) of `field` at `value`. */
export function limiterPosition(field, value) {
  return fieldPosition(field, value);
}

/** The value of `field` at slider `position`, rounded as the strip shows it (0.5 dB, 1 ms). */
export function limiterValue(field, position) {
  const value = fieldValue(field, position);
  return field.key === "ceilingDb" ? Math.round(value * 2) / 2 : Math.max(1, Math.round(value));
}

const FIELDS = Object.freeze({ ceilingDb: CEILING_FIELD, releaseMs: RELEASE_FIELD });
// The smallest change each slider makes, as the strip rounds it.
const UNITS = Object.freeze({ ceilingDb: 0.5, releaseMs: 1 });

/**
 * The whole limiter after a control of the master strip changed: `field` "enabled" with `value`
 * true or false, or "ceilingDb" / "releaseMs" with `value` the slider's position (0-1,000). A
 * position that moved but rounds back to the same value (an arrow key: one step of 1,000) moves it
 * by one unit, 0.5 dB or 1 ms, its way, so the keyboard always gets somewhere. Held in range.
 */
export function limiterChange(settings, field, value) {
  const held = clampLimiter(settings);
  if (field === "enabled") {
    return { ...held, enabled: Boolean(value) };
  }
  const definition = FIELDS[field];
  if (!definition) {
    return held;
  }
  const from = limiterPosition(definition, held[field]);
  let next = limiterValue(definition, value);
  if (next === held[field] && value !== from) {
    next = held[field] + Math.sign(value - from) * UNITS[field];
  }
  return clampLimiter({ ...held, [field]: next });
}

/**
 * The master strip's limiter for the mix `address` (a musician's address, or LOCAL_MIX): its view
 * (limiterView) with `pending` while a change is on its way, or null when there is no such mix.
 * The gain reduction comes from the meters, apart (limiterReduction).
 */
export function masterLimiter(state, address) {
  const strip = masterStrip(state, address);
  if (!strip) {
    return null;
  }
  const { settings, pending } = mixLimiter(state, address);
  const ceilingPercent = strip.cap === null ? null : Math.min(strip.cap, strip.limit);
  return { ...limiterView(settings, 0, ceilingPercent), pending };
}

/**
 * What the master strip's limiter shows: `{ enabled, ceilingDb, releaseMs, ceiling, release,
 * state, note, reduction: { fraction, text }, ceilingPosition, releasePosition }`. `ceilingPercent`
 * is the mix's own ceiling (the lower of the cap and the limit, null for the local mix, which has
 * neither): the note says that the lower of the two ceilings wins. The positions are the sliders'
 * (0-1,000).
 */
export function limiterView(settings, reductionDb = 0, ceilingPercent = null) {
  const held = clampLimiter(settings);
  const reduction = reductionView(held.enabled ? reductionDb : 0, LIMITER_REDUCTION_SCALE_DB);
  const ceiling = formatCeiling(held.ceilingDb);
  const mixCeiling = ceilingPercent === null ? "the mix's full scale" : `the mix's ceiling (${ceilingPercent}%)`;
  return {
    ...held,
    ceiling,
    release: formatMs(held.releaseMs),
    state: held.enabled ? "Limiter on" : "Limiter off",
    note: held.enabled
      ? `Peaks are held at ${ceiling} or ${mixCeiling}, whichever is lower. No added latency.`
      : "Off: the mix is only clipped at its ceiling.",
    reduction,
    ceilingPosition: limiterPosition(CEILING_FIELD, held.ceilingDb),
    releasePosition: limiterPosition(RELEASE_FIELD, held.releaseMs),
  };
}

/**
 * The desk's answer to a limiter change, in words: `{ ok, message }`. `name` is whose mix it is
 * ("Ana", or "The local mix").
 */
export function formatLimiterSet(answer, name) {
  if (answer?.outcome !== "set") {
    return { ok: false, message: answer?.message ?? `${name}'s limiter was not set.` };
  }
  const settings = clampLimiter(answer.settings);
  const whose = name === "The local mix" ? "The local mix's" : `${name}'s`;
  let message = settings.enabled
    ? `${whose} limiter is on: peaks are held at ${formatCeiling(settings.ceilingDb)}, released over ${formatMs(settings.releaseMs)}.`
    : `${whose} limiter is off.`;
  if (answer.saveError) {
    message += ` ${sentenceWithReason("It was not saved for the next start", answer.saveError)}`;
  }
  return { ok: true, message };
}
