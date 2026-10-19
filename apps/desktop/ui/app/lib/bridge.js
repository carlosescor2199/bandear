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
  /**
   * About 30 a second: `{ run, channels: [peak], musicians: { address: { left, right } }, local:
   * { left, right } }`, `local` the peak of the engineer's local mix.
   */
  meters: "meters",
  /** About 4 a second: the `engine_status` snapshot, with its `run`. */
  status: "status",
  /**
   * At once: `{ run, address, origin: "phone" | "desktop", mix }` for a musician's mix, and
   * `{ run, id: "local", origin: "desktop", mix }` for the engineer's local mix.
   */
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
     * Changes the engineer's local mix: `mix` as for setMix, without `maxLevelPercent` (the local
     * mix has no level cap); resolves to the whole mix, which the desk keeps for the next start.
     */
    setLocalMix: (mix) => bridge.invoke("set_local_mix", { mix }),
    /**
     * The output devices the running engine offers for the local mix, `[{ name, channels }]`:
     * under ASIO the input driver's own outputs, under WASAPI every output device. It rejects with
     * no engine running.
     */
    outputDevices: () => bridge.invoke("output_devices"),
    /**
     * Plays the local mix on `output`, `{ device, firstChannel }` (the pair's first output,
     * 0-based), or nowhere with null. Resolves to `{ output, localOutput, applied, saved, saveError
     * }` (see formatLocalOutputSet); with no engine it is kept for the next start. An output the
     * engine does not offer rejects, and the one in use keeps playing.
     */
    setLocalOutput: (output) => bridge.invoke("set_local_output", { output }),
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
    /**
     * The engineer's mix presets in the order they were saved, `[{ id, name, mix: { channels:
     * [{ gain, pan, muted, solo }], groups: { <groupId>: level } } }]` (see mixes.js).
     */
    presets: () => bridge.invoke("presets"),
    /**
     * Saves the mix of the musician at `address` as a new preset called `name` (each input's level,
     * pan, mute and solo and the group levels by id; never the master, the cap or the engineer's
     * controls). Resolves to `{ preset, presets }`. It rejects with no engine running and for a
     * name that is empty, too long or taken (see checkPresetName), and nothing is kept.
     */
    savePreset: (address, name) => bridge.invoke("save_preset", { address, name }),
    /** Renames the preset `id`, as savePreset checks names; resolves to `{ preset, presets }`. */
    renamePreset: (id, name) => bridge.invoke("rename_preset", { id, name }),
    /** Deletes the preset `id`; resolves to `{ preset: null, presets }`. */
    deletePreset: (id) => bridge.invoke("delete_preset", { id }),
    /**
     * Puts the preset `id` on the mix of every musician `musicians` names (addresses), through the
     * running engine; resolves to `{ mixes: [{ address, mix }] }`. It rejects with no engine, for a
     * preset that is not kept and for a musician whose mix follows another's, and then nobody's mix
     * changed.
     */
    applyPreset: (id, musicians) => bridge.invoke("apply_preset", { id, musicians }),
    /**
     * Copies the mix of the musician at `from` onto every musician `to` names: channels, pans,
     * mutes, solos and group levels, never the master, the cap or the engineer's controls. Resolves
     * to `{ mixes }` and rejects as applyPreset does.
     */
    copyMix: (from, to) => bridge.invoke("copy_mix", { from, to }),
    /**
     * Makes the mix of the musician at `address` follow `leader`, `{ address, clientId, name }` (the
     * leader's row, and the name the follower's phone is told), or stops it with null; `clientId` is
     * the follower's row's. Resolves to `{ address, leader, name, keptFor, applied, saved,
     * saveError, mix }`: with an engine running the follower's mix takes the leader's at once and
     * follows it, and their phone is held off its channels and groups; with none it is kept for the
     * next start. A mirror on oneself or in a circle rejects, and nothing changed.
     */
    setMirror: (address, leader, clientId = null) =>
      bridge.invoke("set_mirror", { address, clientId, leader }),
    /**
     * What went wrong or changed when the desk opened its session (a damaged part set aside, a
     * session from a newer MixLink left alone, labels imported), or null.
     */
    sessionNotice: () => bridge.invoke("session_notice"),
    /**
     * The engineer's saved sessions, the newest first: `[{ id, name, savedAt, mixes, problem }]`
     * (see sessions.js); `problem` says why one cannot be loaded.
     */
    sessions: () => bridge.invoke("sessions"),
    /**
     * Saves the desk's whole session, and every musician's mix while an engine runs, as `name`.
     * Resolves to `{ session, engineRunning }`; a name that is empty, too long or taken rejects.
     */
    saveSession: (name) => bridge.invoke("save_session", { name }),
    /** Renames the saved session `id`; resolves to it as listed. */
    renameSession: (id, name) => bridge.invoke("rename_session", { id, name }),
    /** Deletes the saved session `id`. */
    deleteSession: (id) => bridge.invoke("delete_session", { id }),
    /**
     * Loads the saved session `id` in place of the desk's and, with an engine running, applies it
     * at once: groups and channel details, then limits, locks and mirrors, then each saved mix
     * pushed to the musician it was saved for when they are here. Resolves to `{ session,
     * applied, saved, saveError, mixes: [{ address, follows, mix }], skipped: [{ key, label,
     * reason, error }], refused, damaged, audio, audioLoaded, audioSkipped, presets }` (see
     * formatSessionLoaded). Its audio input applies at the next start. A saved session the desk
     * cannot use rejects, and nothing changed.
     */
    loadSession: (id) => bridge.invoke("load_session", { id }),
    /**
     * How the desk records takes (see recording.js): `{ folder, defaultFolder, folderIsDefault,
     * format, includeLocal, armed, notice }`, the folder whole, the inputs 1-based.
     */
    recordingSettings: () => bridge.invoke("recording_settings"),
    /**
     * Keeps `settings`, the whole `{ folder, format, includeLocal, armed }` (the folder null for
     * the default; see settingsRequest), and resolves to them as recordingSettings answers. A
     * folder that is not a whole path, goes up with `..` or is in MixLink's settings folder, an
     * unknown format or an input out of 1-255 rejects, and nothing changed.
     */
    setRecordingSettings: (settings) => bridge.invoke("set_recording_settings", { settings }),
    /**
     * Starts recording a take called `takeName` with the kept settings; `utcOffsetMinutes` (the
     * window's offset from UTC) names its folder in local time. Resolves to `{ recording, folder,
     * armed, skipped, includeLocal, format }`; with no engine, a take recording, nothing armed on
     * the device and no local mix, or a folder that cannot be made, it rejects.
     */
    startRecording: (takeName, utcOffsetMinutes = null) =>
      bridge.invoke("start_recording", { takeName, utcOffsetMinutes }),
    /**
     * Stops the take: resolves to `{ finishing: false, take }`, or `{ finishing: true, message }`
     * while its files are still being finished (ask again). It rejects when nothing records.
     */
    stopRecording: () => bridge.invoke("stop_recording"),
    /** Marks the take being recorded; resolves to `{ frame, seconds, name }`. */
    addMarker: (name) => bridge.invoke("add_marker", { name }),
    /**
     * What a take with the kept settings would take on the disk: `{ folder, bytesPerSecond,
     * freeBytes, minutesLeft, problem }`, `problem` saying why the rate is unknown.
     */
    recordEstimate: () => bridge.invoke("record_estimate"),
    /**
     * The takes in the recordings folder, the newest first: `{ folder, takes, unreadable, missing
     * }` (see recording.js for a take's shape).
     */
    listTakes: () => bridge.invoke("list_takes"),
    /** Opens a take folder (as a take lists it) in the system's file manager. */
    openTakeFolder: (folder) => bridge.invoke("open_take_folder", { folder }),
    /**
     * Plays the take in `takeFolder` (as a take lists it) in place of the live inputs, from
     * `startFrame`, looping when `looping`. Resolves to the soundcheck (see soundcheck.js); a take
     * that does not fit the device, a take recording or no engine is refused. A take's Play is this.
     */
    startSoundcheck: (takeFolder, startFrame = 0, looping = false) =>
      bridge.invoke("start_soundcheck", { takeFolder, startFrame, looping }),
    /** Pauses the soundcheck's take; resolves to the soundcheck. */
    pauseSoundcheck: () => bridge.invoke("pause_soundcheck"),
    /** Plays the soundcheck's take on (from its start when it stopped at its end). */
    resumeSoundcheck: () => bridge.invoke("resume_soundcheck"),
    /** Moves the soundcheck's take to `frame`. */
    seekSoundcheck: (frame) => bridge.invoke("seek_soundcheck", { frame }),
    /** Whether the soundcheck's take starts again at its end. */
    setSoundcheckLoop: (looping) => bridge.invoke("set_soundcheck_loop", { looping }),
    /** Stops the soundcheck and goes back to the live inputs. */
    stopSoundcheck: () => bridge.invoke("stop_soundcheck"),
    onMeters: (handler) => bridge.listen(EVENTS.meters, handler),
    onStatus: (handler) => bridge.listen(EVENTS.status, handler),
    onMixChanged: (handler) => bridge.listen(EVENTS.mixChanged, handler),
  });
}
