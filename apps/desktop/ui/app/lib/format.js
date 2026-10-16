// Text the console shows for engine values. Pure functions, no DOM.

import { clampLevel } from "./values.js";

const MINUS = "−";
const SEPARATOR = " · ";

/**
 * `lead` and the reason after a colon, as one sentence: "lead: reason." A reason that ends with
 * full stops, or no reason at all (null, empty, blank), still gives one sentence with one.
 */
export function sentenceWithReason(lead, reason) {
  const text = String(reason ?? "")
    .trim()
    .replace(/\.+$/, "");
  return text ? `${lead}: ${text}.` : `${lead}.`;
}

/**
 * What a group edit did, from the `set_groups` answer `{ groups, applied, saved, saveError }`:
 * live on every phone, saved for the next start (no engine was running), or live but not kept.
 */
export function formatGroupsSet({ applied, saved, saveError }) {
  if (!applied) {
    return "Groups saved; they apply at the next start.";
  }
  return saved
    ? "Groups applied to every phone."
    : sentenceWithReason("Groups applied to every phone, but not saved", saveError);
}

/**
 * What a channel edit did, from the `set_channels` answer `{ channels, applied, saved, saveError }`,
 * in the words of formatGroupsSet.
 */
export function formatChannelsSet({ applied, saved, saveError }) {
  if (!applied) {
    return "Channel details saved; they apply at the next start.";
  }
  return saved
    ? "Channel details applied to every phone."
    : sentenceWithReason("Channel details applied to every phone, but not saved", saveError);
}

/**
 * What a change of the engineer's controls did, from the `set_engineer_controls` answer `{ applied,
 * saved, saveError }`: in force on the phone and saved, saved for the next start (no engine was
 * running), or in force but not kept. With `name`, the sentence names the musician.
 */
export function formatEngineerControlsSet({ applied, saved, saveError }, name = null) {
  const subject = name ? `${name}'s limit and locks` : "Limit and locks";
  if (!applied) {
    return `${subject} saved; they apply at the next start.`;
  }
  return saved
    ? `${subject} apply now and are saved.`
    : sentenceWithReason(`${subject} apply now, but were not saved`, saveError);
}

/** "85%". */
export function formatPercent(level) {
  return `${clampLevel(level)}%`;
}

/** "−9 dBFS", "0 dBFS", "−∞ dBFS", with a true minus sign. */
export function formatDb(db) {
  if (db === -Infinity) {
    return `${MINUS}∞ dBFS`;
  }
  const rounded = Math.round(db);
  return rounded < 0 ? `${MINUS}${-rounded} dBFS` : `${Math.abs(rounded)} dBFS`;
}

/** "48 kHz", "44.1 kHz". */
export function formatSampleRate(hertz) {
  return `${Number.parseFloat((hertz / 1000).toFixed(1))} kHz`;
}

/** The engine names sample formats as the audio library does ("I24", "F32"); this says "24-bit". */
export function formatSampleFormat(format) {
  const match = /^([IUF])(\d+)$/.exec(format);
  if (!match) {
    return format;
  }
  const [, kind, bits] = match;
  if (kind === "F") {
    return `${bits}-bit float`;
  }
  return kind === "U" ? `${bits}-bit unsigned` : `${bits}-bit`;
}

/** The engine's buffer description ("Fixed(256)", "device default") as the top bar shows it. */
export function formatBuffer(bufferSize) {
  const match = /^Fixed\((\d+)\)$/.exec(bufferSize);
  if (match) {
    return match[1];
  }
  return bufferSize === "device default" ? "default buffer" : bufferSize;
}

/** "48 kHz · 24-bit · 256" for the engine's capture format. */
export function formatCaptureFormat({ sampleRate, sampleFormat, bufferSize }) {
  return [
    formatSampleRate(sampleRate),
    formatSampleFormat(sampleFormat),
    formatBuffer(bufferSize),
  ].join(SEPARATOR);
}

const LINK_LEVELS = new Set(["good", "warning", "bad", "unknown"]);
const LINK_LABELS = { good: "Good", warning: "Warning", bad: "Poor", unknown: "Unknown" };

/**
 * A musician's link as the console names it: `{ level, label, detail }`.
 *
 * `level` is the server's classification ("good", "warning", "bad", "unknown"). The rates come only
 * with a fresh report from the phone, so a bad link without them is a phone whose reports stopped
 * (the link is lost), and a bad link that receives no packets is a phone that hears nothing.
 * Underruns are called glitches, as the design does.
 */
export function linkState(musician) {
  const level = LINK_LEVELS.has(musician.linkHealth) ? musician.linkHealth : "unknown";
  const rates = [
    musician.underrunsPerSecond,
    musician.lossPercent,
    musician.receiveTimeoutsPerSecond,
  ];
  if (!rates.every((rate) => typeof rate === "number")) {
    if (level === "bad") {
      return { level, label: "Link lost", detail: "Reports from this phone stopped" };
    }
    if (!musician.controlConnected) {
      return {
        level: "unknown",
        label: "Not connected",
        detail: "The app on this phone is not connected",
      };
    }
    return { level: "unknown", label: "No reports", detail: "No reports from this phone yet" };
  }
  if (level === "bad" && musician.packetsPerSecond === 0) {
    return { level, label: "No audio", detail: "This phone receives no audio" };
  }
  const [glitches, loss, timeouts] = rates;
  return {
    level,
    label: LINK_LABELS[level],
    detail: [
      `Loss ${loss.toFixed(2)}%`,
      `Glitches ${glitches.toFixed(1)}/s`,
      `Timeouts ${timeouts.toFixed(1)}/s`,
    ].join(SEPARATOR),
  };
}

/** True when a musician's link is a warning or bad, which the top bar points out. */
export function needsAttention(musician) {
  const { level } = linkState(musician);
  return level === "warning" || level === "bad";
}

/** The musician's name, or the phone's IP when there is none. */
export function musicianLabel(musician) {
  if (musician.name) {
    return musician.name;
  }
  const { address } = musician;
  if (address.startsWith("[")) {
    return address.slice(1, address.indexOf("]"));
  }
  const colon = address.lastIndexOf(":");
  return colon === -1 ? address : address.slice(0, colon);
}
