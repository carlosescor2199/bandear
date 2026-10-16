// The engine's start request and the audio choices it is built from. Pure functions, no DOM.
//
// The request is the one the old window sent (main.js `defaultConfig`): nobody configured (phones
// join on their own or are added by address), the default control port, no groups, the chosen
// driver and buffer. The driver is "auto" (the system's own) unless the engineer chose another:
// ASIO is never picked by default.

export const HOSTS = Object.freeze(["auto", "wasapi", "asio"]);

const DEFAULT_CONTROL_PORT = 50001;

/** The audio choices before the engineer makes any: the system's driver, 256 frames. */
export const DEFAULT_AUDIO = Object.freeze({ host: "auto", bufferFrames: 256 });

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
  return {
    host: normalizeHost("host" in change ? change.host : current.host),
    bufferFrames: normalizeBuffer("bufferFrames" in change ? change.bufferFrames : current.bufferFrames),
  };
}

/** The `start_engine` request for `audio`, optionally asking for an input device by name. */
export function startRequest(audio = DEFAULT_AUDIO, { deviceFilter = null } = {}) {
  return {
    deviceFilter,
    targets: [],
    controlPort: DEFAULT_CONTROL_PORT,
    groups: [],
    host: normalizeHost(audio.host),
    bufferFrames: normalizeBuffer(audio.bufferFrames),
  };
}
