// The engineer's local mix: the engine's own mix for this computer, which no phone hears. The Mix
// screen opens it as its last tab ("Local mix", id LOCAL_MIX) and edits it with a musician's
// strips, without a level cap, a limit or locks; the Network and system screen chooses the output
// it plays on ("Local mix out"). Pure functions, no DOM.
//
// The output rules are the engine's: off by default ("None", the lowest load), always at 48 kHz,
// and only on an output the running driver offers (`output_devices`): under ASIO the input
// driver's own outputs, under WASAPI every output device. A choice is a stereo pair, named by its
// device and its first output, 0-based: "Volt 4 · outputs 3/4" is `{ device: "Volt 4",
// firstChannel: 2 }`. The status's `localOutput` says where it plays and how: `{ device,
// firstChannel, state: "off" | "running" | "failed", reason, underrunFrames, droppedFrames,
// notice }`, `notice` saying why the output the session keeps was not opened at start.

import { LOCAL_MIX } from "./console-state.js";
import { sentenceWithReason } from "./format.js";

export { LOCAL_MIX };

/** The select's value for no local output. */
export const NO_OUTPUT = "";

const NONE = Object.freeze({ value: NO_OUTPUT, label: "None (lowest load)" });

/** What the select says about the rules, under it. */
export const LOCAL_OUTPUT_HINT =
  "The local mix plays at 48 kHz on the pair you choose. Under ASIO only the input driver's own " +
  "outputs are offered; WASAPI offers every output device.";

/** An output in words: "Volt 4 · outputs 3/4". */
export function outputLabel(device, firstChannel) {
  return `${device} · outputs ${firstChannel + 1}/${firstChannel + 2}`;
}

/** The select's value for the pair of `device` that starts at `firstChannel`. */
export function outputValue(device, firstChannel) {
  return `${firstChannel}|${device}`;
}

/** The choice a select's value names, `{ device, firstChannel }`, or null for None. */
export function parseOutputValue(value) {
  const text = String(value ?? "");
  const bar = text.indexOf("|");
  if (bar <= 0) {
    return null;
  }
  const first = text.slice(0, bar);
  const device = text.slice(bar + 1);
  if (!/^\d+$/.test(first) || device === "") {
    return null;
  }
  return { device, firstChannel: Number(first) };
}

/**
 * The select's options: None first, then each whole stereo pair of every device listed (`[{ name,
 * channels }]`), outputs 1/2, 3/4 and so on. A device with fewer than two outputs offers none.
 */
export function outputOptions(devices) {
  const options = [NONE];
  for (const { name, channels } of devices ?? []) {
    for (let first = 0; first + 2 <= channels; first += 2) {
      options.push({ value: outputValue(name, first), label: outputLabel(name, first) });
    }
  }
  return options;
}

function count(value, noun) {
  const number = Number(value ?? 0);
  return `${number.toLocaleString("en-US")} ${noun}${number === 1 ? "" : "s"}`;
}

// The status's local output in words: `{ kind, text, alert }`; `alert` is said once, as an alert,
// when the output stopped on its own.
function outputState(output) {
  if (output?.state === "running") {
    const gaps =
      output.underrunFrames > 0 ? `${count(output.underrunFrames, "silent frame")} filled in` : "no gaps";
    const drops =
      output.droppedFrames > 0
        ? `${count(output.droppedFrames, "frame")} dropped to hold the delay`
        : "nothing dropped";
    return {
      kind: "running",
      text: `Playing on ${outputLabel(output.device, output.firstChannel)}: ${gaps}, ${drops}.`,
      alert: null,
    };
  }
  if (output?.state === "failed") {
    const lead = `${outputLabel(output.device, output.firstChannel)} stopped`;
    return {
      kind: "failed",
      text: "Stopped: the local mix plays nowhere.",
      alert: `${sentenceWithReason(lead, output.reason)} Choose it again, or another output.`,
    };
  }
  return { kind: "off", text: "Off: the local mix plays nowhere.", alert: null };
}

/**
 * The Network and system screen's "Local mix out": `{ options, value, disabled, state: { kind,
 * text, alert }, notice, error, hint }`. The value is the output in use (or the one that failed),
 * else None; an output in use that the engine does not offer now is still shown, marked so.
 * With no engine running there is nothing to choose from; until the outputs are listed the
 * select waits.
 */
export function localOutputView(state) {
  const status = state.engine.status;
  if (!status) {
    return {
      options: [NONE],
      value: NO_OUTPUT,
      disabled: true,
      state: {
        kind: "idle",
        text: "Start the engine to choose where the local mix plays.",
        alert: null,
      },
      notice: null,
      error: null,
      hint: LOCAL_OUTPUT_HINT,
    };
  }
  const output = status.localOutput ?? null;
  const { list, error } = state.outputs;
  let options = outputOptions(list ?? []);
  const value =
    typeof output?.device === "string" ? outputValue(output.device, output.firstChannel) : NO_OUTPUT;
  if (value !== NO_OUTPUT && !options.some((option) => option.value === value)) {
    options = [
      ...options,
      { value, label: `${outputLabel(output.device, output.firstChannel)} (not offered now)` },
    ];
  }
  let shown = outputState(output);
  if (list === null && !error) {
    shown = { ...shown, text: "Listing the outputs…" };
  }
  return {
    options,
    value,
    disabled: (list === null && !error) || Boolean(status.stopped),
    state: shown,
    notice: output?.notice ?? null,
    error: error ? sentenceWithReason("The outputs could not be listed", error) : null,
    hint: LOCAL_OUTPUT_HINT,
  };
}

// The local tab's word about where the local mix plays, as a link state shows it (a mark and a
// word, never a colour alone).
function localLink(output) {
  if (output?.state === "running") {
    return {
      level: "local",
      label: `This computer · ${outputLabel(output.device, output.firstChannel)}`,
      detail: "The local mix plays on this computer",
    };
  }
  if (output?.state === "failed") {
    return {
      level: "warning",
      label: "This computer · output stopped",
      detail: output.reason ?? "The output stopped",
    };
  }
  return {
    level: "local",
    label: "This computer · no output",
    detail: "Choose where it plays in Network and system",
  };
}

/**
 * The Mix screen's Local mix tab, `{ address: LOCAL_MIX, label, link, selected }` as a musician's
 * tab is, or null while there is no local mix (no engine runs).
 */
export function localTab(state) {
  if (!state.localMix) {
    return null;
  }
  return {
    address: LOCAL_MIX,
    label: "Local mix",
    link: localLink(state.engine.status?.localOutput ?? null),
    selected: state.selected === LOCAL_MIX,
  };
}

/**
 * What a change of the local output did, from the `set_local_output` answer `{ output, applied,
 * saved, saveError }`: playing there now (or nowhere), kept for the next start with no engine, or
 * playing but not kept.
 */
export function formatLocalOutputSet({ output, applied, saved, saveError }) {
  const where = output ? outputLabel(output.device, output.firstChannel) : null;
  if (!applied) {
    return where
      ? `${where} is kept: the local mix plays there from the next start.`
      : "No output is kept: the local mix plays nowhere from the next start.";
  }
  const done = where ? `The local mix plays on ${where}.` : "The local mix plays nowhere now.";
  return saved ? done : `${done} ${sentenceWithReason("It was not saved for the next start", saveError)}`;
}
