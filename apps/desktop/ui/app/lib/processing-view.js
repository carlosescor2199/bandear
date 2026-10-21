// The channel processing editor's pure logic: the equaliser graph's scales, grid and handles (drag,
// wheel and keys), the words every value is shown in, the sliders' positions, the compressor's
// static curve, the gain-reduction meters, the channel choices, the strip presets as listed and the
// A/B bypass. No DOM; the editor's screen (screens/processing-screen.js) and its organisms only
// draw what this works out and hand the engineer's gestures back to it.
//
// The graph is drawn in a box of `width` x `height` user units: frequency on a logarithmic scale
// from 20 Hz (left) to 20 kHz (right), gain from +18 dB (top) to -18 dB (bottom). Channels are
// numbered from 1, as the desk numbers them.

import { channelLabel, pairUnits } from "./channels.js";
import {
  EQ_HIGH_HZ,
  EQ_LOW_HZ,
  RANGES,
  STRIP_CHANNELS,
  defaultStrip,
  eqResponseOf,
} from "./processing.js";
import { processingHash } from "./router.js";

/** The gain the graph shows either side of 0 dB: the bands' own range. */
export const EQ_SPAN_DB = RANGES.bandGainDb[1];

/** The equaliser's bands by position, as the editor names them. */
export const BAND_NAMES = Object.freeze(["Low shelf", "Low peak", "High peak", "High shelf"]);

/** How much one wheel notch or Page Up multiplies Q by. */
const Q_STEP = 1.12;

const MINUS = "−";
const LOG_SPAN = Math.log(EQ_HIGH_HZ / EQ_LOW_HZ);

const clamp = (value, [low, high]) => Math.min(Math.max(value, low), high);

/** The x of `freqHz` on a graph `width` wide. */
export function freqToX(freqHz, width) {
  return (Math.log(clamp(freqHz, [EQ_LOW_HZ, EQ_HIGH_HZ]) / EQ_LOW_HZ) / LOG_SPAN) * width;
}

/** The frequency at `x` on a graph `width` wide, held to 20 Hz-20 kHz. */
export function xToFreq(x, width) {
  const fraction = clamp(x / width, [0, 1]);
  if (fraction === 0) {
    return EQ_LOW_HZ;
  }
  if (fraction === 1) {
    return EQ_HIGH_HZ;
  }
  return EQ_LOW_HZ * Math.exp(fraction * LOG_SPAN);
}

/** The y of `db` on a graph `height` tall, held to the scale's edges. */
export function gainToY(db, height) {
  const held = clamp(db, [-EQ_SPAN_DB, EQ_SPAN_DB]);
  return ((EQ_SPAN_DB - held) / (2 * EQ_SPAN_DB)) * height;
}

/** The gain at `y` on a graph `height` tall, held to ±18 dB. */
export function yToGain(y, height) {
  return clamp(EQ_SPAN_DB - (y / height) * 2 * EQ_SPAN_DB, [-EQ_SPAN_DB, EQ_SPAN_DB]);
}

// A number with at most `digits` decimals, trailing zeros dropped.
function trimmed(value, digits) {
  return String(Number(value.toFixed(digits)));
}

// A signed number with a real minus sign, never "-0".
function signed(text) {
  return text.startsWith("-") ? `${MINUS}${text.slice(1)}` : text;
}

/** "80 Hz", "2.5 kHz", "1 kHz". */
export function formatHz(hz) {
  const value = Number(hz) || 0;
  if (Math.round(value) < 1000) {
    return `${Math.round(value)} Hz`;
  }
  return `${trimmed(value / 1000, 1)} kHz`;
}

/** An equaliser gain: "+3.0 dB", "−2.0 dB", "0.0 dB". */
export function formatGainDb(db) {
  const text = (Number(db) || 0).toFixed(1);
  if (Number(text) === 0) {
    return "0.0 dB";
  }
  return Number(text) > 0 ? `+${text} dB` : `${signed(text)} dB`;
}

/** A level or amount in dB with up to one decimal: "−18 dB", "−2.5 dB", "6 dB". */
export function formatDb(db) {
  const text = trimmed(Number(db) || 0, 1);
  return `${text === "0" ? "0" : signed(text)} dB`;
}

/** A filter's Q: "0.71", "10.0". */
export function formatQ(q) {
  const value = Number(q) || 0;
  return value >= 10 ? value.toFixed(1) : value.toFixed(2);
}

/** A time: "0.5 ms", "150 ms", "1.5 s". */
export function formatMs(ms) {
  const value = Number(ms) || 0;
  if (value >= 1000) {
    return `${trimmed(value / 1000, 2)} s`;
  }
  return value < 10 ? `${trimmed(value, 2)} ms` : `${Math.round(value)} ms`;
}

/** A compressor ratio: "3:1", "2.5:1". */
export function formatRatio(ratio) {
  return `${trimmed(Number(ratio) || 1, 1)}:1`;
}

// One band in words for a handle's name: "Low peak, on, 400 Hz, +6.0 dB, Q 1.00".
function bandWords(name, band) {
  return `${name}, ${band.enabled ? "on" : "off"}, ${formatHz(band.frequencyHz)}, ${formatGainDb(
    band.gainDb,
  )}, Q ${formatQ(band.q)}`;
}

/**
 * The graph's handles: the high-pass filter's on the 0 dB line at its frequency, then each band's
 * at its frequency and gain. `{ target, name, x, y, enabled, label }`; `target` is "highPass" or the
 * band's position, which dragHandle, nudgeQ and handleKey take.
 */
export function bandHandles(eq, width, height) {
  const { highPass } = eq;
  return [
    {
      target: "highPass",
      name: "High-pass",
      x: freqToX(highPass.frequencyHz, width),
      y: height / 2,
      enabled: highPass.enabled,
      label: `High-pass, ${highPass.enabled ? "on" : "off"}, ${formatHz(highPass.frequencyHz)}`,
    },
    ...eq.bands.map((band, index) => ({
      target: index,
      name: BAND_NAMES[index],
      x: freqToX(band.frequencyHz, width),
      y: gainToY(band.gainDb, height),
      enabled: band.enabled,
      label: bandWords(BAND_NAMES[index], band),
    })),
  ];
}

// `eq` with `change` applied to the part `target` names (a copy; `eq` is left alone).
function changed(eq, target, change) {
  const next = structuredClone(eq);
  if (target === "highPass") {
    next.highPass = { ...next.highPass, ...change(next.highPass) };
  } else {
    next.bands[target] = { ...next.bands[target], ...change(next.bands[target]) };
  }
  return next;
}

const frequencyRange = (target) =>
  target === "highPass" ? RANGES.highPassFrequencyHz : RANGES.bandFrequencyHz;
const qRange = (target) => (target === "highPass" ? RANGES.highPassQ : RANGES.bandQ);

/**
 * `eq` with the handle `target` dragged to (`x`, `y`) on a graph `width` x `height`: a band takes
 * that frequency and gain, the high-pass filter only the frequency; either is turned on.
 */
export function dragHandle(eq, target, x, y, width, height) {
  const frequencyHz = clamp(xToFreq(x, width), frequencyRange(target));
  return changed(eq, target, () =>
    target === "highPass"
      ? { frequencyHz, enabled: true }
      : { frequencyHz, gainDb: yToGain(y, height), enabled: true },
  );
}

/** `eq` with the Q of `target` multiplied by 1.12 per step (`steps` may be negative), in range. */
export function nudgeQ(eq, target, steps) {
  return changed(eq, target, (part) => ({ q: clamp(part.q * Q_STEP ** steps, qRange(target)) }));
}

/**
 * `eq` after `key` on the handle `target`, or null when the key does nothing there. Left and Right
 * move the frequency a semitone (an octave with Shift), Up and Down the
 * gain 0.5 dB (3 dB with Shift), Page Up and Page Down Q, Home puts the gain back to 0 dB, Enter
 * and Space turn the part on or off.
 */
export function handleKey(eq, target, key, { shift = false } = {}) {
  const isBand = target !== "highPass";
  switch (key) {
    case "ArrowLeft":
    case "ArrowRight": {
      const factor = (shift ? 2 : 2 ** (1 / 12)) ** (key === "ArrowRight" ? 1 : -1);
      return changed(eq, target, (part) => ({
        frequencyHz: clamp(part.frequencyHz * factor, frequencyRange(target)),
      }));
    }
    case "ArrowUp":
    case "ArrowDown": {
      if (!isBand) {
        return null;
      }
      const step = (shift ? 3 : 0.5) * (key === "ArrowUp" ? 1 : -1);
      return changed(eq, target, (part) => ({
        gainDb: clamp(part.gainDb + step, RANGES.bandGainDb),
      }));
    }
    case "PageUp":
    case "PageDown":
      return nudgeQ(eq, target, (shift ? 4 : 1) * (key === "PageUp" ? 1 : -1));
    case "Home":
      return isBand ? changed(eq, target, () => ({ gainDb: 0 })) : null;
    case "Enter":
    case " ":
      return changed(eq, target, (part) => ({ enabled: !part.enabled }));
    default:
      return null;
  }
}

/** The SVG path through `points` (`[{ freqHz, db }]`) on a graph `width` x `height`. */
export function curvePath(points, width, height) {
  return (points ?? [])
    .map(
      (point, index) =>
        `${index === 0 ? "M" : "L"}${freqToX(point.freqHz, width).toFixed(1)} ${gainToY(
          point.db,
          height,
        ).toFixed(1)}`,
    )
    .join("");
}

const GRID_FREQS = [50, 100, 200, 500, 1000, 2000, 5000, 10000];
const GRID_GAINS = [12, 6, 0, -6, -12];

/** The graph's grid: `{ freqs: [{ x, label }], gains: [{ y, label }] }`. */
export function gridLines(width, height) {
  return {
    freqs: GRID_FREQS.map((hz) => ({ x: freqToX(hz, width), label: formatHz(hz) })),
    gains: GRID_GAINS.map((db) => ({
      y: gainToY(db, height),
      label: db > 0 ? `+${db}` : signed(String(db)),
    })),
  };
}

/** The equaliser in words, the graph's text alternative. */
export function curveDescription(eq) {
  const { highPass } = eq;
  const parts = [
    "Equaliser response.",
    highPass.enabled ? `High-pass on at ${formatHz(highPass.frequencyHz)}.` : "High-pass off.",
    ...eq.bands.map((band, index) =>
      band.enabled
        ? `${BAND_NAMES[index]} on: ${formatHz(band.frequencyHz)}, ${formatGainDb(band.gainDb)}, Q ${formatQ(band.q)}.`
        : `${BAND_NAMES[index]} off.`,
    ),
  ];
  return parts.join(" ");
}

function sameEq(a, b) {
  return a !== null && b !== null && JSON.stringify(a) === JSON.stringify(b);
}

/**
 * The points to draw for `eq`: the desk's (`desk`, worked out for the equaliser `asked`) while
 * that is still the equaliser shown, else `points` worked out here the same way, so a drag is
 * drawn at once and the desk's curve replaces it once it answers. `{ points, fromDesk }`.
 */
export function curveSource({ asked = null, eq, desk = null, points = 128 }) {
  if (desk && sameEq(asked, eq)) {
    return { points: desk, fromDesk: true };
  }
  return { points: eqResponseOf(eq, points), fromDesk: false };
}

/** The steps of a slider, from 0 to this. */
export const SLIDER_STEPS = 1000;

// A log scale needs positive ends: a range from 0 is shifted by 1 first.
const logOffset = ([low]) => (low <= 0 ? 1 - low : 0);

/**
 * The slider position (0-1,000) of `value` for `field` (`{ range, scale }`, scale "linear" or
 * "log"), held to the range.
 */
export function fieldPosition(field, value) {
  const [low, high] = field.range;
  const held = clamp(Number(value) || 0, field.range);
  let fraction;
  if (field.scale === "log") {
    const offset = logOffset(field.range);
    fraction = Math.log((held + offset) / (low + offset)) / Math.log((high + offset) / (low + offset));
  } else {
    fraction = (held - low) / (high - low);
  }
  return Math.round(fraction * SLIDER_STEPS);
}

/** The value of `field` at slider `position` (0-1,000). */
export function fieldValue(field, position) {
  const [low, high] = field.range;
  const fraction = clamp(Number(position) || 0, [0, SLIDER_STEPS]) / SLIDER_STEPS;
  if (field.scale === "log") {
    const offset = logOffset(field.range);
    return clamp((low + offset) * ((high + offset) / (low + offset)) ** fraction - offset, field.range);
  }
  return low + fraction * (high - low);
}

/** `settings` with the value at `path` ("compressor.thresholdDb", "eq.bands.1.q") set (a copy). */
export function withSetting(settings, path, value) {
  const next = structuredClone(settings);
  const keys = path.split(".");
  const last = keys.pop();
  let part = next;
  for (const key of keys) {
    part = part[key];
  }
  part[last] = value;
  return next;
}

/**
 * The compressor's static curve: the output level (dB) for an input level `inputDb`, with a soft
 * knee `kneeDb` wide around the threshold (as the desk's compressor), plus the makeup gain.
 */
export function compressorOutputDb({ thresholdDb, ratio, kneeDb, makeupDb = 0 }, inputDb) {
  const over = inputDb - thresholdDb;
  let output;
  if (2 * over < -kneeDb) {
    output = inputDb;
  } else if (kneeDb > 0 && 2 * Math.abs(over) <= kneeDb) {
    output = inputDb + ((1 / ratio - 1) * (over + kneeDb / 2) ** 2) / (2 * kneeDb);
  } else {
    output = thresholdDb + over / ratio;
  }
  return output + makeupDb;
}

/** The lowest level the transfer curve shows, on both axes. */
const TRANSFER_FLOOR_DB = -60;

/** The compressor's static curve as an SVG path in a `size` x `size` box, -60 to 0 dB each way. */
export function transferPath(compressor, size, steps = 60) {
  const toY = (db) => (clamp(db, [TRANSFER_FLOOR_DB, 0]) / TRANSFER_FLOOR_DB) * size;
  return Array.from({ length: steps + 1 }, (_, index) => {
    const inputDb = TRANSFER_FLOOR_DB * (1 - index / steps);
    const x = (index / steps) * size;
    return `${index === 0 ? "M" : "L"}${x.toFixed(1)} ${toY(compressorOutputDb(compressor, inputDb)).toFixed(1)}`;
  }).join("");
}

/** A gain-reduction meter: `{ fraction, text }` for `db` taken off on a scale `scaleDb` long. */
export function reductionView(db, scaleDb) {
  const value = Number.isFinite(db) && db >= 0.05 ? db : 0;
  return {
    fraction: clamp(value / scaleDb, [0, 1]),
    text: value === 0 ? "0.0 dB" : `${MINUS}${value.toFixed(1)} dB`,
  };
}

/**
 * The strips the editor can open, from a status: one per input of the device (the first 32), a
 * stereo pair once on its first side. `[{ channel, sides, label }]`: "CH 1 · Kick", "CH 3–4 · Keys".
 */
export function stripChoices(status) {
  if (!status?.channels) {
    return [];
  }
  const count = Math.min(status.sourceChannels ?? status.channels.length, STRIP_CHANNELS);
  const channels = status.channels.slice(0, count);
  return pairUnits(channels).map((unit) => {
    const first = channels[unit[0]];
    const sides = unit.map((index) => index + 1);
    const input = sides.length === 2 ? `CH ${sides[0]}–${sides[1]}` : `CH ${sides[0]}`;
    const name = channelLabel({ ...first, number: sides[0] });
    return {
      channel: sides[0],
      sides,
      label: name === `CH ${sides[0]}` ? input : `${input} · ${name}`,
    };
  });
}

/** The choice `step` places from the one holding `channel` (held at either end). */
export function stepChoice(choices, channel, step) {
  const at = choices.findIndex((choice) => choice.sides.includes(channel));
  if (at < 0) {
    return channel;
  }
  return choices[clamp(at + step, [0, choices.length - 1])].channel;
}

/** The strip presets as the editor lists them: `[{ name, label, factory }]`. */
export function presetOptions(presets) {
  return (presets ?? []).map(({ name, factory }) => ({
    name,
    label: factory ? `${name} (built-in)` : name,
    factory: Boolean(factory),
  }));
}

/**
 * The link to an input's processing editor that the Mix strips and the Channels table show.
 * `sides` are the strip's inputs (0-based, both sides of a stereo pair), `label` what the input is
 * called and `active` processingActive's answer (null: not read yet). A pair opens on its first
 * side. Answers `{ href, state: "on" | "off" | "unknown", word, text, ariaLabel }`: `word` the
 * table's ("On", "Off", "" while unknown) and `text` the strip's ("Processing on" or "Processing").
 */
export function processingLink({ sides, label, active }) {
  const first = Math.min(...sides) + 1;
  const state = active === null || active === undefined ? "unknown" : active ? "on" : "off";
  const said = { on: "on", off: "off", unknown: "not read yet" }[state];
  return {
    href: processingHash(first),
    state,
    word: { on: "On", off: "Off", unknown: "" }[state],
    text: state === "on" ? "Processing on" : "Processing",
    ariaLabel: `Processing of ${label}, ${said}`,
  };
}

/**
 * A/B: bypasses `channel`, whose strip is `strip`. `ab` maps a bypassed channel to the strip kept
 * for it; answers the next map and the strip to send (a neutral one).
 */
export function abBypass(ab, channel, strip) {
  return { ab: { ...ab, [channel]: structuredClone(strip) }, send: defaultStrip() };
}

/** A/B: ends `channel`'s bypass; answers the next map and the kept strip to send (null: none). */
export function abRestore(ab, channel) {
  const { [channel]: kept = null, ...rest } = ab;
  return { ab: rest, send: kept };
}

/**
 * A/B: the map once the desk's strip of `channel` (`{ active, pending }`, channelStrip) is known. A
 * bypassed strip is neutral at the desk; one the desk shows working, with no edit of the editor's
 * on its way, was changed elsewhere (a session load, another window), so the bypass ends there and
 * the kept strip is not sent back over it.
 */
export function abSettle(ab, channel, strip) {
  if (!(channel in ab) || !strip.active || strip.pending) {
    return ab;
  }
  const { [channel]: _dropped, ...rest } = ab;
  return rest;
}

/**
 * The editor's failure line: the latest failure wins. `sent(channel, outcome)` notes the answer to
 * an effect send of `channel` (a refusal, or `ok` to clear it); `text(view)` is the line for the
 * editor's view (`{ channel, outcome, error }`): a refused send of the channel shown, unless a
 * failure of the editor's own commands (or the strips' read error) came after it. A refused send is
 * forgotten once another channel is shown, so it never stands over a channel it was not about.
 */
export function createFailureLine() {
  let order = 0;
  let send = null;
  let other = { said: "", at: 0 };
  return {
    sent(channel, outcome) {
      order += 1;
      send = outcome?.ok ? null : { channel, message: outcome?.message ?? "", at: order };
    },
    text(view) {
      const said = view.outcome && !view.outcome.ok ? view.outcome.message : (view.error ?? "");
      if (said !== other.said) {
        order += 1;
        other = { said, at: order };
      }
      if (send && send.channel !== view.channel) {
        send = null;
      }
      return send && (!other.said || send.at > other.at) ? send.message : other.said;
    },
  };
}

// A slider of the editor: `{ key, label, range, scale, format }`, `key` its name in the strip part.
const field = (key, label, range, scale, format) => Object.freeze({ key, label, range, scale, format });

const formatPlainDb = (db) => formatDb(db);

/** The high-pass filter's and each band's sliders. */
export const HIGH_PASS_FIELDS = Object.freeze([
  field("frequencyHz", "Frequency", RANGES.highPassFrequencyHz, "log", formatHz),
  field("q", "Q", RANGES.highPassQ, "log", formatQ),
]);
export const BAND_FIELDS = Object.freeze([
  field("frequencyHz", "Frequency", RANGES.bandFrequencyHz, "log", formatHz),
  field("gainDb", "Gain", RANGES.bandGainDb, "linear", formatGainDb),
  field("q", "Q", RANGES.bandQ, "log", formatQ),
]);

/** The gate's sliders. */
export const GATE_FIELDS = Object.freeze([
  field("thresholdDb", "Threshold", RANGES.gateThresholdDb, "linear", formatPlainDb),
  field("rangeDb", "Range", RANGES.gateRangeDb, "linear", formatPlainDb),
  field("attackMs", "Attack", RANGES.gateAttackMs, "log", formatMs),
  field("holdMs", "Hold", RANGES.gateHoldMs, "log", formatMs),
  field("releaseMs", "Release", RANGES.gateReleaseMs, "log", formatMs),
]);

/** The compressor's sliders. */
export const COMPRESSOR_FIELDS = Object.freeze([
  field("thresholdDb", "Threshold", RANGES.compressorThresholdDb, "linear", formatPlainDb),
  field("ratio", "Ratio", RANGES.compressorRatio, "log", formatRatio),
  field("kneeDb", "Knee", RANGES.compressorKneeDb, "linear", formatPlainDb),
  field("attackMs", "Attack", RANGES.compressorAttackMs, "log", formatMs),
  field("releaseMs", "Release", RANGES.compressorReleaseMs, "log", formatMs),
  field("makeupDb", "Makeup", RANGES.compressorMakeupDb, "linear", formatPlainDb),
]);

/** The scales of the gain-reduction meters, in dB: the gate takes off up to its range. */
export const GATE_REDUCTION_SCALE_DB = 40;
export const COMPRESSOR_REDUCTION_SCALE_DB = 20;
