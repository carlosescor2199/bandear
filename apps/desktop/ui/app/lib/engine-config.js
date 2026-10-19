// The engine's start request and the audio choices it is built from. Pure functions, no DOM.
//
// The request is the one the first window sent (its `defaultConfig`): nobody configured (phones
// join on their own or are added by address), the default control port, no groups, the chosen
// driver and buffer. The driver is "auto" (the system's own) unless the engineer chose another:
// ASIO is never picked by default, and an ASIO request always names the driver the engineer
// picked (`asioDriver`): without one there is no request at all, since the engine would load the
// first ASIO driver it finds, a kernel-mode driver nobody chose.
//
// The choices are `{ host, device, bufferFrames, asioDriver }`: `device` is the input picked under
// "auto" or "wasapi" (null for the system's default input) and `asioDriver` the one picked under
// "asio". A plain start reopens the input picked for the driver (rememberedDevice). The desk keeps
// the choices in its session, and the window starts from them (restoredAudio); when the input they
// name is gone, a start gives way to the system's default input on the same driver, or on the
// system's own when the choice was ASIO (fallbackAudio), and never on ASIO.
//
// "demo" is the demo source (demo-source.js): a generated band with no interface, its one device
// "Demo (no interface)". Only the engineer chooses it; nothing here ever picks it on its own.

export const HOSTS = Object.freeze(["auto", "wasapi", "asio", "demo"]);

const DEFAULT_CONTROL_PORT = 50001;

/** Why no ASIO start can be built: no driver was picked. */
export const ASIO_NEEDS_DRIVER =
  "ASIO is chosen but no ASIO driver is picked: pick one under Network and system.";

/**
 * The audio choices before the engineer makes any: the system's driver and its default input, 256
 * frames, no ASIO driver picked.
 */
export const DEFAULT_AUDIO = Object.freeze({
  host: "auto",
  device: null,
  bufferFrames: 256,
  asioDriver: null,
});

const AUDIO_FIELDS = Object.freeze(Object.keys(DEFAULT_AUDIO));

/** A driver the engine knows ("auto", "wasapi", "asio", "demo"); anything else is "auto". */
export function normalizeHost(host) {
  const value = typeof host === "string" ? host.trim().toLowerCase() : "";
  return HOSTS.includes(value) ? value : "auto";
}

/** A buffer size in whole frames, or `null` for the device's own. */
export function normalizeBuffer(frames) {
  if (frames === null || frames === undefined || frames === "") {
    return null;
  }
  const value = Number(frames);
  return Number.isInteger(value) && value > 0 ? value : null;
}

// A device or driver name, trimmed; null for none.
function normalizeName(name) {
  const value = typeof name === "string" ? name.trim() : "";
  return value === "" ? null : value;
}

/**
 * The audio choices with `change` applied; a field `change` leaves out keeps its value. An input
 * belongs to its driver, so a change of driver that names no input starts on the new driver's
 * default input.
 */
export function audioChoice(current, change) {
  const pick = (field) => (field in change ? change[field] : current[field]);
  const host = normalizeHost(pick("host"));
  const keepsDevice = "device" in change || host === normalizeHost(current.host);
  return {
    host,
    device: keepsDevice ? normalizeName(pick("device")) : null,
    bufferFrames: normalizeBuffer(pick("bufferFrames")),
    asioDriver: normalizeName(pick("asioDriver")),
  };
}

/**
 * The input a plain start of `audio` opens: the picked ASIO driver under ASIO, else the picked
 * input; null for the system's default input (or no ASIO driver picked).
 */
export function rememberedDevice(audio) {
  return normalizeHost(audio.host) === "asio"
    ? normalizeName(audio.asioDriver)
    : normalizeName(audio.device);
}

/**
 * The choices the desk's session kept (`audio_settings`), as every start takes them: nothing kept
 * is the defaults, a field left out keeps its default, and a choice that names ASIO without a
 * driver (an older or hand-edited file) is no ASIO at all but the system's own driver, so no ASIO
 * driver is ever loaded that the engineer did not pick.
 */
export function restoredAudio(saved) {
  if (!saved || typeof saved !== "object") {
    return DEFAULT_AUDIO;
  }
  const change = Object.fromEntries(
    AUDIO_FIELDS.filter((field) => field in saved).map((field) => [field, saved[field]]),
  );
  const audio = audioChoice(DEFAULT_AUDIO, { ...change, device: change.device ?? null });
  return audio.host === "asio" && !audio.asioDriver ? { ...audio, host: "auto" } : audio;
}

/**
 * The choices a start falls back to when the input `audio` names is gone: the system's default
 * input on the same driver, or on the system's own driver when the choice was ASIO, so a missing
 * input never leads to an ASIO driver. The buffer stays.
 */
export function fallbackAudio(audio) {
  const host = normalizeHost(audio.host);
  return { ...audio, host: host === "asio" ? "auto" : host, device: null };
}

/** What the console says when a start fell back because the input `audio` names was missing. */
export function fallbackNotice(audio) {
  const device = rememberedDevice(audio);
  if (normalizeHost(audio.host) === "asio") {
    return (
      `The saved ASIO driver ${device} was not found, so the engine started on the system's own ` +
      "driver and its default input, never on another ASIO driver. The saved driver is kept and " +
      "tried again at the next start."
    );
  }
  return (
    `The saved input ${device} was not found, so the engine started on the system's default ` +
    "input. The saved input is kept and tried again at the next start."
  );
}

/**
 * The `start_engine` request for `audio`, optionally asking for an input device by name. Every
 * start goes through here. Without a name it opens the input picked for the driver
 * (rememberedDevice): under ASIO the picked driver, and with none the answer is `null`: no ASIO
 * start without a driver the engineer picked.
 */
export function startRequest(audio = DEFAULT_AUDIO, { deviceFilter = null } = {}) {
  const host = normalizeHost(audio.host);
  const device = deviceFilter ?? rememberedDevice({ ...audio, host });
  if (host === "asio" && !device) {
    return null;
  }
  return {
    deviceFilter: device,
    targets: [],
    controlPort: DEFAULT_CONTROL_PORT,
    groups: [],
    host,
    bufferFrames: normalizeBuffer(audio.bufferFrames),
  };
}
