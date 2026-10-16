// The only module that touches Tauri. Screens talk to the engine through `createEngineApi`, which
// takes any bridge: the Tauri one in the app, the fake one (fake-bridge.js) in tests, the gallery
// and a browser preview.
//
// A bridge is `{ invoke(command, args) -> Promise, listen(event, handler) -> Promise<unlisten>,
// setTitle(title) -> Promise }`.
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
    /** Titles the app's window (the capability `core:window:allow-set-title` allows it). */
    setTitle: async (title) => tauri.window?.getCurrentWindow?.().setTitle(title),
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
    /**
     * Replaces the groups: the whole list in its order, `[{ id, name, channels }]` with 1-based
     * channels and no `id` for a new group. Resolves to `{ groups, applied, saved, saveError }`:
     * the groups each with its id, whether a running engine took them (every musician's group
     * levels follow their groups by id) and whether they were saved; with no engine running they
     * are saved and apply at the next start (see `formatGroupsSet`). A refused list rejects, and
     * nothing changed.
     */
    setGroups: (groups) => bridge.invoke("set_groups", { groups }),
    /**
     * Replaces the channel details: the whole list in channel order (the first entry is CH 1),
     * `[{ name, kind, hidden, link }]`, any field left out taking its default (see channels.js).
     * Resolves to `{ channels, applied, saved, saveError }` as setGroups does (see
     * `formatChannelsSet`): a running engine sends them to every phone at once, silences hidden
     * inputs in every mix and pans a new pair apart in every mix. A refused list rejects, and
     * nothing changed.
     */
    setChannels: (channels) => bridge.invoke("set_channels", { channels }),
    /**
     * The whole channel table the desk keeps, `[{ name, kind, hidden, link }]` in channel order:
     * the details of inputs the current device lacks included, which a status leaves out. A
     * channel edit is built on it and sent back whole with setChannels. With an engine running it
     * is the engine's table, else the session's.
     */
    channelTable: () => bridge.invoke("channel_table"),
    /**
     * Sets what the engineer allows one musician beyond their mix: `controls` is `{ limitPercent?,
     * lockedChannels? }` (see buildEngineerRequest), the limit 0-100 and the locks from CH 1 on;
     * `clientId` is the row's client id when it has one, so a desk with no engine keeps them for
     * the phone, the key the next engine resolves first. Resolves to `{ address, limitPercent,
     * lockedChannels, keptFor, applied, saved, saveError, mix }`: the musician's phone has them now
     * and is held to them (`applied`), they are kept for its phone or its address (`keptFor`), and
     * `mix` is the mix now stored (null with no engine, when they apply at the next start). A
     * refused change rejects, and nothing changed.
     */
    setEngineerControls: (address, controls, clientId = null) =>
      bridge.invoke("set_engineer_controls", { address, controls, clientId }),
    /**
     * The audio input the desk last ran with, `{ host, device, bufferFrames, asioDriver }`, or null
     * before any was kept; read as kept (see restoredAudio in engine-config.js).
     */
    audioSettings: () => bridge.invoke("audio_settings"),
    /**
     * Keeps `settings`, `{ host, device, bufferFrames, asioDriver }`, for every later start, once
     * the running engine started or switched with them. Resolves to them as kept. It rejects with no
     * engine running, for an unknown driver and for ASIO without a picked driver, and nothing is
     * kept.
     */
    setAudioSettings: (settings) => bridge.invoke("set_audio_settings", { settings }),
    onMeters: (handler) => bridge.listen(EVENTS.meters, handler),
    onStatus: (handler) => bridge.listen(EVENTS.status, handler),
    onMixChanged: (handler) => bridge.listen(EVENTS.mixChanged, handler),
  });
}
