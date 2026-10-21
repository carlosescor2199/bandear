// Each input's channel processing as the desk answers it (see apps/desktop/src-tauri/src/commands/
// processing.rs): the strip's shape, its ranges, the equaliser's response and the desk's answers
// in words. No DOM.
//
// A strip is `{ eq: { highPass: { enabled, frequencyHz, q }, bands: [band x4] }, gate: { enabled,
// thresholdDb, rangeDb, attackMs, holdMs, releaseMs }, compressor: { enabled, thresholdDb, ratio,
// kneeDb, attackMs, releaseMs, makeupDb } }`, a band `{ enabled, frequencyHz, gainDb, q }` (a low
// shelf, two peaks and a high shelf, in that order). The desk holds every number in its range
// (RANGES) and answers what it keeps; a field left out takes its default. Channels are numbered
// from 1, as the desk numbers them: CH 1 is `1`. A linked pair is one strip: setting either side
// sets both, and the answer names both.

import { pairUnits } from "./channels.js";

/** The mixer's channels, each with its strip. */
export const STRIP_CHANNELS = 32;

/** The engine's sample rate, which the equaliser's filters are worked out at. */
const SAMPLE_RATE = 48000;

/** The points of an equaliser response when none is asked for, and its frequency span. */
export const EQ_RESPONSE_POINTS = 128;
export const EQ_LOW_HZ = 20;
export const EQ_HIGH_HZ = 20000;

/** Every number's range, `[low, high]`, as the desk holds it. */
export const RANGES = Object.freeze({
  highPassFrequencyHz: [20, 2000],
  highPassQ: [0.5, 2],
  bandFrequencyHz: [20, 20000],
  bandGainDb: [-18, 18],
  bandQ: [0.3, 10],
  gateThresholdDb: [-80, 0],
  gateRangeDb: [0, 80],
  gateAttackMs: [0.05, 50],
  gateHoldMs: [0, 2000],
  gateReleaseMs: [5, 4000],
  compressorThresholdDb: [-60, 0],
  compressorRatio: [1, 20],
  compressorKneeDb: [0, 24],
  compressorAttackMs: [0.1, 200],
  compressorReleaseMs: [10, 2000],
  compressorMakeupDb: [0, 24],
});

/** The shape of each equaliser band, by position. */
export const BAND_SHAPES = Object.freeze(["lowShelf", "peak", "peak", "highShelf"]);

const BUTTERWORTH_Q = Math.SQRT1_2;

/** A strip that does nothing, with the desk's defaults. */
export function defaultStrip() {
  const band = (frequencyHz, q) => ({ enabled: true, frequencyHz, gainDb: 0, q });
  return {
    eq: {
      highPass: { enabled: false, frequencyHz: 80, q: BUTTERWORTH_Q },
      bands: [band(100, BUTTERWORTH_Q), band(400, 1), band(2500, 1), band(8000, BUTTERWORTH_Q)],
    },
    gate: {
      enabled: false,
      thresholdDb: -50,
      rangeDb: 40,
      attackMs: 0.5,
      holdMs: 50,
      releaseMs: 150,
    },
    compressor: {
      enabled: false,
      thresholdDb: -18,
      ratio: 3,
      kneeDb: 6,
      attackMs: 10,
      releaseMs: 150,
      makeupDb: 0,
    },
    sends: { reverb: 0, delay: 0 },
  };
}

// `value` held in `range`; a value that is not a number takes `fallback`, as the desk does.
function within(value, [low, high], fallback) {
  const number = Number(value);
  if (value === null || value === undefined || Number.isNaN(number)) {
    return fallback;
  }
  return Math.min(Math.max(number, low), high);
}

/**
 * `settings` as the desk keeps them: every field it leaves out taking its default and every number
 * held in its range (a value that is not a number takes the default).
 */
export function clampStrip(settings = {}) {
  const fallback = defaultStrip();
  const eq = settings?.eq ?? {};
  const highPass = { ...fallback.eq.highPass, ...(eq.highPass ?? {}) };
  const gate = { ...fallback.gate, ...(settings?.gate ?? {}) };
  const compressor = { ...fallback.compressor, ...(settings?.compressor ?? {}) };
  return {
    eq: {
      highPass: {
        enabled: Boolean(highPass.enabled),
        frequencyHz: within(highPass.frequencyHz, RANGES.highPassFrequencyHz, 80),
        q: within(highPass.q, RANGES.highPassQ, BUTTERWORTH_Q),
      },
      bands: fallback.eq.bands.map((base, index) => {
        const band = { ...base, ...(eq.bands?.[index] ?? {}) };
        return {
          enabled: Boolean(band.enabled),
          frequencyHz: within(band.frequencyHz, RANGES.bandFrequencyHz, 1000),
          gainDb: within(band.gainDb, RANGES.bandGainDb, 0),
          q: within(band.q, RANGES.bandQ, 1),
        };
      }),
    },
    gate: {
      enabled: Boolean(gate.enabled),
      thresholdDb: within(gate.thresholdDb, RANGES.gateThresholdDb, -50),
      rangeDb: within(gate.rangeDb, RANGES.gateRangeDb, 40),
      attackMs: within(gate.attackMs, RANGES.gateAttackMs, 0.5),
      holdMs: within(gate.holdMs, RANGES.gateHoldMs, 50),
      releaseMs: within(gate.releaseMs, RANGES.gateReleaseMs, 150),
    },
    compressor: {
      enabled: Boolean(compressor.enabled),
      thresholdDb: within(compressor.thresholdDb, RANGES.compressorThresholdDb, -18),
      ratio: within(compressor.ratio, RANGES.compressorRatio, 3),
      kneeDb: within(compressor.kneeDb, RANGES.compressorKneeDb, 6),
      attackMs: within(compressor.attackMs, RANGES.compressorAttackMs, 10),
      releaseMs: within(compressor.releaseMs, RANGES.compressorReleaseMs, 150),
      makeupDb: within(compressor.makeupDb, RANGES.compressorMakeupDb, 0),
    },
    sends: {
      reverb: Math.round(within(settings?.sends?.reverb, [0, 100], 0)),
      delay: Math.round(within(settings?.sends?.delay, [0, 100], 0)),
    },
  };
}

/** True when the equaliser changes nothing: the high-pass off and every band off or at 0 dB. */
export function isNeutralEq(eq) {
  const kept = clampStrip({ eq }).eq;
  return !kept.highPass.enabled && kept.bands.every((band) => !band.enabled || band.gainDb === 0);
}

/** True when the strip changes nothing, as the desk decides it: the audio then skips it. */
export function isNeutralStrip(settings) {
  const kept = clampStrip(settings);
  const gateNeutral = !kept.gate.enabled || kept.gate.rangeDb === 0;
  const compressorNeutral =
    !kept.compressor.enabled || (kept.compressor.ratio === 1 && kept.compressor.makeupDb === 0);
  return isNeutralEq(kept.eq) && gateNeutral && compressorNeutral;
}

/** True when the channel keeps nothing: a neutral strip that sends to no effect (the desk keeps
 * every other channel in the session). */
export function isUntouchedStrip(settings) {
  const kept = clampStrip(settings);
  return isNeutralStrip(kept) && kept.sends.reverb === 0 && kept.sends.delay === 0;
}

// The cookbook biquad of `shape` at 48 kHz, normalised: `[b0, b1, b2, a1, a2]`, as the engine
// works it out.
function biquad(shape, frequencyHz, q, gainDb) {
  const w0 = (2 * Math.PI * frequencyHz) / SAMPLE_RATE;
  const cos = Math.cos(w0);
  const alpha = Math.sin(w0) / (2 * q);
  const a = 10 ** (gainDb / 40);
  const shelf = 2 * Math.sqrt(a) * alpha;
  let terms;
  switch (shape) {
    case "highPass":
      terms = [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2, 1 + alpha, -2 * cos, 1 - alpha];
      break;
    case "peak":
      terms = [1 + alpha * a, -2 * cos, 1 - alpha * a, 1 + alpha / a, -2 * cos, 1 - alpha / a];
      break;
    case "lowShelf":
      terms = [
        a * (a + 1 - (a - 1) * cos + shelf),
        2 * a * (a - 1 - (a + 1) * cos),
        a * (a + 1 - (a - 1) * cos - shelf),
        a + 1 + (a - 1) * cos + shelf,
        -2 * (a - 1 + (a + 1) * cos),
        a + 1 + (a - 1) * cos - shelf,
      ];
      break;
    default:
      terms = [
        a * (a + 1 + (a - 1) * cos + shelf),
        -2 * a * (a - 1 + (a + 1) * cos),
        a * (a + 1 + (a - 1) * cos - shelf),
        a + 1 - (a - 1) * cos + shelf,
        2 * (a - 1 - (a + 1) * cos),
        a + 1 - (a - 1) * cos - shelf,
      ];
  }
  const [b0, b1, b2, a0, a1, a2] = terms;
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}

// The gain of one biquad at `frequencyHz`, in dB.
function biquadDb([b0, b1, b2, a1, a2], frequencyHz) {
  const w = (2 * Math.PI * frequencyHz) / SAMPLE_RATE;
  const numRe = b0 + b1 * Math.cos(w) + b2 * Math.cos(2 * w);
  const numIm = b1 * Math.sin(w) + b2 * Math.sin(2 * w);
  const denRe = 1 + a1 * Math.cos(w) + a2 * Math.cos(2 * w);
  const denIm = a1 * Math.sin(w) + a2 * Math.sin(2 * w);
  const power = (numRe ** 2 + numIm ** 2) / (denRe ** 2 + denIm ** 2);
  return 10 * Math.log10(Math.max(power, 1e-30));
}

/**
 * The gain of the equaliser `eq` at `frequencyHz`, in dB, the high-pass filter included, worked
 * out as the desk works it out: what `eq_response` answers, for the fake bridge and a preview.
 */
export function eqMagnitudeDb(eq, frequencyHz) {
  const kept = clampStrip({ eq }).eq;
  let db = 0;
  if (kept.highPass.enabled) {
    db += biquadDb(biquad("highPass", kept.highPass.frequencyHz, kept.highPass.q, 0), frequencyHz);
  }
  kept.bands.forEach((band, index) => {
    if (band.enabled && band.gainDb !== 0) {
      const filter = biquad(BAND_SHAPES[index], band.frequencyHz, band.q, band.gainDb);
      db += biquadDb(filter, frequencyHz);
    }
  });
  return db;
}

/**
 * The equaliser's response, `[{ freqHz, db }]`: `points` gains (128 by default, held to 2-1,024)
 * spaced evenly on a log scale from 20 Hz to 20 kHz, both ends included, as `eq_response` answers.
 */
export function eqResponseOf(eq, points = EQ_RESPONSE_POINTS) {
  const count = Math.min(Math.max(Math.round(Number(points) || EQ_RESPONSE_POINTS), 2), 1024);
  const span = EQ_HIGH_HZ / EQ_LOW_HZ;
  return Array.from({ length: count }, (_, index) => {
    const freqHz = EQ_LOW_HZ * span ** (index / (count - 1));
    return { freqHz, db: eqMagnitudeDb(eq, freqHz) };
  });
}

/** "CH 3", "CH 3 and CH 4", "CH 1, CH 2 and CH 5". */
export function channelList(channels) {
  const names = channels.map((channel) => `CH ${channel}`);
  if (names.length <= 1) {
    return names.join("");
  }
  return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

/**
 * The desk's answer to a processing change (`set_channel_processing`, `reset_channel_processing`,
 * `copy_channel_processing`, `apply_strip_preset`) in words: `{ ok, message }`. Only `set` is
 * done; `invalid` and `notFound` changed nothing.
 */
export function formatProcessingSet(answer) {
  const ok = answer?.outcome === "set";
  const fallback = ok ? "The processing was changed." : "The processing was not changed.";
  return { ok, message: answer?.message || fallback };
}

/**
 * The desk's answer to a strip preset change (`save_strip_preset`, `delete_strip_preset`) in
 * words: `{ ok, message }`. `saved`, `replaced` and `deleted` are done; `invalid`, `factory` and
 * `notFound` changed nothing.
 */
export function formatStripPresetsSet(answer) {
  const ok = ["saved", "replaced", "deleted"].includes(answer?.outcome);
  const fallback = ok ? "The strip presets were changed." : "The strip presets were not changed.";
  return { ok, message: answer?.message || fallback };
}

/**
 * `channels` (1-based) with the other side of each one's stereo pair among `table` (a status's
 * channels in channel order), each once, in the order met: the strips a change to them touches.
 */
export function stripUnitChannels(table, channels) {
  const units = pairUnits(table ?? []);
  const touched = new Set();
  for (const channel of channels) {
    touched.add(channel);
    for (const index of units.find((unit) => unit.includes(channel - 1)) ?? []) {
      touched.add(index + 1);
    }
  }
  return [...touched];
}
