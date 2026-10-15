// The only module that touches Tauri. Screens talk to the engine through `createEngineApi`, which
// takes any bridge: the Tauri one in the app, the fake one (fake-bridge.js) in tests, the gallery
// and a browser preview.
//
// A bridge is `{ invoke(command, args) -> Promise, listen(event, handler) -> Promise<unlisten> }`.
// `listen` hands the handler the event's payload, not Tauri's event envelope. A command that fails
// rejects with the command's error text.

/**
 * The engine events the desktop emits (apps/desktop/src-tauri/src/events.rs). Every payload names
 * the engine run it belongs to; see `isLiveRun` in console-state.js.
 */
export const EVENTS = Object.freeze({
  /** About 30 a second: `{ run, channels: [peak], musicians: { address: { left, right } } }`. */
  meters: "meters",
  /** About 4 a second: the `engine_status` snapshot, with its `run`. */
  status: "status",
  /** At once: `{ run, address, origin: "phone" | "desktop", mix }`. */
  mixChanged: "mix-changed",
});

/** The bridge over `window.__TAURI__`, which `withGlobalTauri` provides. */
export function createTauriBridge(tauri = globalThis.__TAURI__) {
  const invoke = tauri?.core?.invoke;
  const listen = tauri?.event?.listen;
  if (typeof invoke !== "function" || typeof listen !== "function") {
    throw new Error("the Tauri globals are not available: open the console in the MixLink app");
  }
  return {
    invoke: (command, args) => invoke(command, args),
    listen: (event, handler) => listen(event, (message) => handler(message.payload)),
  };
}

/**
 * The desktop commands (apps/desktop/src-tauri/src/lib.rs) with the argument names they take.
 * Every call returns the command's promise.
 */
export function createEngineApi(bridge) {
  return Object.freeze({
    /**
     * Whether an engine runs: `{ state: "notRunning", run }`, or the `EngineStatusDto` with its
     * `run` and `state: "running"`. It rejects only on a real failure (a poisoned lock).
     */
    status: () => bridge.invoke("engine_status"),
    /**
     * `config` is the `StartRequest`: deviceFilter, targets, controlPort, groups, host,
     * bufferFrames. Resolves to `{ run, deviceName, captureFormat }`.
     */
    start: (config = {}) => bridge.invoke("start_engine", { config }),
    stop: () => bridge.invoke("stop_engine"),
    /** `host` is "auto", "asio" or "wasapi"; none means auto. */
    listInputDevices: (host = null) => bridge.invoke("list_input_devices", { host }),
    switchDevice: (device) => bridge.invoke("switch_device", { device }),
    addTarget: (address) => bridge.invoke("add_target", { address }),
    removeTarget: (address) => bridge.invoke("remove_target", { address }),
    setMusicianName: (address, name) => bridge.invoke("set_musician_name", { address, name }),
    /** `mix` is a partial `set_mix` request (see buildMixRequest); resolves to the whole mix. */
    setMix: (address, mix) => bridge.invoke("set_mix", { address, mix }),
    onMeters: (handler) => bridge.listen(EVENTS.meters, handler),
    onStatus: (handler) => bridge.listen(EVENTS.status, handler),
    onMixChanged: (handler) => bridge.listen(EVENTS.mixChanged, handler),
  });
}
