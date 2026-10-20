// The demo source: a generated band the engine plays in place of an audio interface, so the app
// runs and streams to phones on a machine with no interface. Pure functions, no DOM.
//
// It is a driver of its own ("demo") with one device, "Demo (no interface)", chosen only by the
// engineer: the window never picks it on its own, "auto" is never the demo, and choosing it or
// leaving it lists and loads no other driver (ASIO included). While it plays, the top bar says so
// (demoChip) and the window's title says "Demo source" (windowTitle in network-view.js). A take
// can be recorded from it like from any input; a soundcheck needs an interface and is refused.

/** The demo source's driver, as the window and the desk name it. */
export const DEMO_HOST = "demo";

/** The demo source's one device. */
export const DEMO_DEVICE = "Demo (no interface)";

/** The demo source's inputs: one generated voice each. */
export const DEMO_CHANNELS = 8;

/** The voices, in input order. */
export const DEMO_VOICES = Object.freeze([
  "Kick",
  "Snare",
  "Hi-hat",
  "Bass",
  "Keys",
  "Pad",
  "Vocal",
  "Room",
]);

/** The devices `list_input_devices` offers for the demo driver: its one device. */
export function demoDevices() {
  return [{ name: DEMO_DEVICE, channels: DEMO_CHANNELS }];
}

/** True while the running engine plays the demo source rather than an interface. */
export function isDemoRunning(state) {
  const status = state?.engine?.status;
  return Boolean(status && !status.stopped && status.demo);
}

/**
 * The top bar's badge while the demo source plays, `{ kind, word, time, detail, label, href }`
 * for <ml-rec-chip>, or null when an interface (or nothing) runs. It links to the audio setup,
 * where an interface is chosen.
 */
export function demoChip(state) {
  if (!isDemoRunning(state)) {
    return null;
  }
  return {
    kind: "demo",
    word: "Demo source",
    time: "",
    detail: "— no interface in use",
    label:
      "Demo source — no interface in use: the engine plays a generated band. Open Network and " +
      "system to choose an interface",
    href: "#/network",
  };
}
