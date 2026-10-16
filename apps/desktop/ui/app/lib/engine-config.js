// The engine's start request and the audio choices it is built from. Pure functions, no DOM.
//
// The request is the one the first window sent (its `defaultConfig`): nobody configured (phones
// join on their own or are added by address), the default control port, no groups, the chosen
// driver and buffer. The driver is "auto" (the system's own) unless the engineer chose another:
// ASIO is never picked by default, and an ASIO request always names the driver the engineer
// picked (`asioDriver`): without one there is no request at all, since the engine would load the
// first ASIO driver it finds, a kernel-mode driver nobody chose.

export const HOSTS = Object.freeze(["auto", "wasapi", "asio"]);

const DEFAULT_CONTROL_PORT = 50001;

/** Why no ASIO start can be built: no driver was picked. */
export const ASIO_NEEDS_DRIVER =
  "ASIO is chosen but no ASIO driver is picked: pick one under Network and system.";

/**
 * The audio choices before the engineer makes any: the system's driver, 256 frames, no ASIO
 * driver picked.
 */
export const DEFAULT_AUDIO = Object.freeze({ host: "auto", bufferFrames: 256, asioDriver: null });

/** A driver the engine knows ("auto", "wasapi", "asio"); anything else is "auto". */
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

/** The audio choices with `change` applied; a field `change` leaves out keeps its value. */
export function audioChoice(current, change) {
  const pick = (field) => (field in change ? change[field] : current[field]);
  return {
    host: normalizeHost(pick("host")),
    bufferFrames: normalizeBuffer(pick("bufferFrames")),
    asioDriver: pick("asioDriver") || null,
  };
}

/**
 * The `start_engine` request for `audio`, optionally asking for an input device by name. Every
 * start goes through here. Under ASIO the device is the one named or else the picked driver, and
 * with neither the answer is `null`: no ASIO start without a driver the engineer picked.
 */
export function startRequest(audio = DEFAULT_AUDIO, { deviceFilter = null } = {}) {
  const host = normalizeHost(audio.host);
  const device = deviceFilter ?? (host === "asio" ? audio.asioDriver || null : null);
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
