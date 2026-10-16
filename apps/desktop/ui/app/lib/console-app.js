// The console's controller: the one place that talks to the engine through a bridge and keeps
// the store. It lives as long as the window; screens come and go and use it. No DOM.
//
// - On creation it listens to the engine's events, reads the audio choices the desk's session
//   keeps (`audio_settings`, until a read succeeds), then asks for the engine's state. When the
//   answer is that no engine runs (`state: "notRunning"`, an answer and not an error) it starts one
//   with the start request the old window sent, built on those choices (see engine-config.js),
//   unless `autoStart` is false. A failed read of the state, and a failed listen, is the console's
//   connection error, and so is a failed read of the choices when it would start: nothing starts
//   on the defaults in place of a kept input. `reconnect` tries again, one attempt at a time.
// - The audio choices (driver, input, buffer, picked ASIO driver) are applied only once an engine
//   started or switched with them, and only then kept in the session (`set_audio_settings`), so
//   every later start reopens them: Start, Try again, the automatic start, a start after a stop
//   and a buffer change. A plain start whose input is gone (an interface left unplugged: not listed
//   for its driver any more) starts on the default input instead, never on ASIO, says so, and keeps
//   the choices for the next start; it saves nothing.
// - `start` and `stop` drive the engine and its phase. A start, a failed start and a confirmed
//   stop end the current run, so late events of an ended engine are ignored (see
//   console-state.js). A refused stop or an unreadable status after a start ends nothing: the
//   engine may still run, so its run stays live, a refused stop is shown, and the status is read
//   again.
// - `edit` records local mix edits and sends them with `set_mix` through a per-musician
//   coalescer: one request in flight per musician, the latest edits always sent, each answer
//   settling the edits it carried and offering its mix by revision. Edits of the engineer's limit
//   and locks (ENGINEER_FIELDS) go the same way through their own coalescer to
//   `set_engineer_controls`; an answer that did not both apply and save is kept to be said.
// - Meters skip the store: `onMeters` listeners get each `meters` payload of the live run.
// - The Network and system screen's commands: list devices, apply a driver or buffer (a
//   restart; ASIO only on a picked driver), switch the device (live under WASAPI, a restart
//   otherwise), add, remove and rename musicians. The window's title follows the device.
// - The Channels and groups screen's edits (`editChannels`, `editGroups`): each replaces the whole
//   list the desk keeps, so they go out one at a time. A channel edit is built on the whole table
//   read afresh (`channel_table`: the details of inputs the device lacks included, which a status
//   leaves out); a group edit on the list the last answer gave, or on the status's (a status lists
//   every group). Without either the desk does not know the groups, and replacing them blindly
//   would lose what is kept, so the edit is refused. Each answers `{ ok, message }`.
// - The Mix screen's tools: presets (read once connected, `loadPresets` again), save, rename,
//   delete and apply them, copy a mix onto others, and start or stop a mirror. Each answers `{ ok,
//   message }`, the desk's answer in words, and reads the status after it.

import { createEngineApi } from "./bridge.js";
import { createCoalescer } from "./coalesce.js";
import {
  actions,
  consoleReducer,
  initialState,
  isLiveRun,
  musicianByAddress,
  pendingEdits,
} from "./console-state.js";
import { groupDefinitions, groupDefinitionsOf } from "./channels-view.js";
import {
  ASIO_NEEDS_DRIVER,
  audioChoice,
  fallbackAudio,
  fallbackNotice,
  rememberedDevice,
  restoredAudio,
  startRequest,
} from "./engine-config.js";
import { formatChannelsSet, formatGroupsSet, musicianLabel, sentenceWithReason } from "./format.js";
import { buildEngineerRequest, buildMixRequest, splitEdits } from "./mix.js";
import { formatCopied, formatMirrorSet, formatPresetApplied } from "./mixes.js";
import { switchMethod, windowTitle } from "./network-view.js";
import { createStore } from "./store.js";

// After a start whose status could not be read: how often and how far apart to ask again.
const STATUS_RETRIES = 3;
const STATUS_RETRY_MS = 500;

function message(error) {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The controller over `bridge`. `autoStart` starts an engine when none runs; `statusRetryMs` is the
 * wait between status reads after a start whose status could not be read.
 */
export function createConsoleApp({ bridge, autoStart = true, statusRetryMs = STATUS_RETRY_MS }) {
  const api = createEngineApi(bridge);
  const store = createStore(consoleReducer, initialState);
  const meterListeners = new Set();
  const unlisteners = [];
  let disposed = false;

  // Sends `address`'s pending mix edits as they are when the send goes out. The coalescer calls it
  // at most once at a time per musician, and again after an answer when edits are still waiting.
  async function sendMix(address) {
    const state = store.getState();
    const musician = musicianByAddress(state, address);
    const sent = splitEdits(pendingEdits(state, address)).mix;
    if (!musician || Object.keys(sent).length === 0) {
      return;
    }
    try {
      const answer = await api.setMix(address, buildMixRequest(musician.mix, sent));
      store.dispatch(actions.mixApplied(address, sent, answer));
    } catch (error) {
      store.dispatch(actions.mixFailed(address, sent, error));
    }
  }

  // Sends `address`'s pending edits of the engineer's controls, as sendMix sends the mix's.
  async function sendEngineer(address) {
    const state = store.getState();
    const musician = musicianByAddress(state, address);
    const sent = splitEdits(pendingEdits(state, address)).engineer;
    if (!musician || Object.keys(sent).length === 0) {
      return;
    }
    try {
      const outcome = await api.setEngineerControls(
        address,
        buildEngineerRequest(musician.mix, sent),
        musician.clientId ?? null,
      );
      store.dispatch(actions.engineerApplied(address, sent, outcome));
    } catch (error) {
      store.dispatch(actions.mixFailed(address, sent, error));
    }
  }

  const sender = createCoalescer(sendMix);
  const engineerSender = createCoalescer(sendEngineer);

  // Puts the engine's answer to `engine_status` in the store: its status when it runs, idle when
  // it says no engine runs.
  function takeEngineState(answer) {
    if (answer.state === "notRunning") {
      store.dispatch(actions.engine("idle"));
    } else {
      store.dispatch(actions.status(answer));
    }
  }

  // Reads the engine's state into the store. Resolves to null, or to the error when it could
  // not be read, which tells nothing about the engine.
  async function readStatus() {
    try {
      takeEngineState(await api.status());
      return null;
    } catch (error) {
      return error;
    }
  }

  // After a start, the status the pump sends fills the view; when none has come yet, ask again a
  // few times. When it never comes, say so, with Try again, rather than stay on "starting".
  function fillAfterStart(lastError, attempt = 1) {
    if (disposed || store.getState().engine.phase !== "starting") {
      return;
    }
    if (attempt > STATUS_RETRIES) {
      store.dispatch(
        actions.connectionFailed(
          `The engine started, but its status could not be read: ${message(lastError)}`,
        ),
      );
      return;
    }
    const timer = setTimeout(async () => {
      if (disposed || store.getState().engine.phase !== "starting") {
        return;
      }
      const error = await readStatus();
      if (error) {
        fillAfterStart(error, attempt + 1);
      }
    }, statusRetryMs);
    timer.unref?.();
  }

  // The audio choices are known once read from the session, or once the engineer made one (which
  // a later read must not replace).
  let choiceKnown = false;
  // The applied choices differ from what the session keeps: kept for the next start while nothing
  // ran them, or not saved when they ran. The next start that runs them saves them.
  let choiceUnsaved = false;

  // Reads the audio choices the desk's session keeps, until a read succeeds: every start builds on
  // them. Resolves to null once they are known, or to the error of a failed read, which the next
  // connect or start tries again. While they are unknown nothing starts on the defaults, so a
  // passing failure never opens the default input in place of the kept one.
  async function readChoice() {
    if (choiceKnown) {
      return null;
    }
    try {
      const saved = await api.audioSettings();
      if (!choiceKnown) {
        store.dispatch(actions.audio(restoredAudio(saved)));
        choiceKnown = true;
      }
      return null;
    } catch (error) {
      return error;
    }
  }

  // Why nothing started while the kept choices could not be read, and the way past it: an input
  // the engineer chooses in Network and system starts without them (and is kept from then on).
  function choiceUnread(error) {
    const why = sentenceWithReason(
      "The saved audio input could not be read, so nothing was started",
      message(error),
    );
    return `${why} Choose an input in Network and system to start without it.`;
  }

  // Keeps the applied choices, which an engine now runs with, in the session for every later
  // start. A failure is said, and the next start that runs them tries again.
  async function keepChoice() {
    choiceKnown = true;
    try {
      await api.setAudioSettings(store.getState().audio);
      choiceUnsaved = false;
      if (store.getState().audioNotice?.kind === "unsaved") {
        store.dispatch(actions.audioNotice(null));
      }
    } catch (error) {
      choiceUnsaved = true;
      store.dispatch(
        actions.audioNotice({
          kind: "unsaved",
          message: sentenceWithReason(
            "The audio input runs but was not saved for the next start",
            message(error),
          ),
        }),
      );
    }
  }

  // The start request on the default input (fallbackAudio: never ASIO) when the input `request`
  // asked for is no longer listed for its driver, matched as the engine matches it; null when it
  // is listed (the start failed for another reason), when none was asked for, or when the devices
  // cannot be listed.
  async function defaultInputRequest(audio, request) {
    if (request.deviceFilter === null) {
      return null;
    }
    let listed;
    try {
      listed = await api.listInputDevices(request.host);
    } catch {
      return null;
    }
    const sought = request.deviceFilter.toLowerCase();
    const present = listed.some((device) => device.name.toLowerCase().includes(sought));
    return present ? null : startRequest(fallbackAudio(audio));
  }

  // Starts (or restarts) the engine with `audio`, the applied choices unless given, on
  // `deviceFilter` when named, else on the input the choices name. Every start is built by
  // startRequest, which never builds an ASIO start without a picked driver. With `orDefault`, a
  // start whose input is gone starts on the default input instead and says so (`audioNotice`
  // "fallback"); the choices stay as they are. Answers `{ ok: true }` or `{ ok: false, error }`.
  async function start({
    deviceFilter = null,
    audio = store.getState().audio,
    orDefault = false,
  } = {}) {
    const request = startRequest(audio, { deviceFilter });
    if (!request) {
      // Nothing reaches the engine, so an engine that runs keeps running.
      const running = store.getState().engine.status !== null;
      store.dispatch(
        running
          ? actions.commandFailed("start", ASIO_NEEDS_DRIVER)
          : actions.engine("error", ASIO_NEEDS_DRIVER),
      );
      return { ok: false, error: ASIO_NEEDS_DRIVER };
    }
    store.dispatch(actions.engine("starting"));
    let fellBack = false;
    try {
      await api.start(request);
    } catch (error) {
      // A failed start has stopped the engine it replaced and started none.
      const fallback = orDefault ? await defaultInputRequest(audio, request) : null;
      if (!fallback) {
        store.dispatch(actions.engine("error", message(error)));
        return { ok: false, error: message(error) };
      }
      try {
        await api.start(fallback);
      } catch (fallbackError) {
        const reason = sentenceWithReason(
          `${rememberedDevice(audio)} was not found, and the system's default input did not ` +
            "start either",
          message(fallbackError),
        );
        store.dispatch(actions.engine("error", reason));
        return { ok: false, error: reason };
      }
      fellBack = true;
    }
    if (fellBack) {
      store.dispatch(actions.audioNotice({ kind: "fallback", message: fallbackNotice(audio) }));
    }
    // The engine runs now: failing to read its status must not end its run.
    const error = await readStatus();
    if (error) {
      fillAfterStart(error);
    }
    return { ok: true };
  }

  // A start of the applied choices as they are (Start, Try again, the automatic start): a missing
  // input gives way to the default input, and choices not saved yet are saved once they run.
  // Choices that could not be read yet are read first; while that fails nothing starts.
  async function startChoice() {
    const unread = await readChoice();
    if (unread) {
      const reason = choiceUnread(unread);
      // Nothing reaches the engine, so an engine that runs keeps running.
      const running = store.getState().engine.status !== null;
      store.dispatch(
        running
          ? actions.commandFailed("start", reason.replace(/\.$/, ""))
          : actions.engine("error", reason),
      );
      return { ok: false, error: reason };
    }
    const result = await start({ orDefault: true });
    const fellBack = store.getState().audioNotice?.kind === "fallback";
    if (result.ok && choiceUnsaved && !fellBack) {
      await keepChoice();
    }
    return result;
  }

  async function stop() {
    store.dispatch(actions.engine("stopping"));
    try {
      await api.stop();
    } catch (error) {
      // Only a confirmed stop ends the run: the engine may still be running, so ask it.
      store.dispatch(actions.commandFailed("stop", message(error)));
      await readStatus();
      return;
    }
    store.dispatch(actions.engine("idle"));
  }

  // An event that arrives after `dispose` (a listener still being registered) is dropped.
  function heard(handler) {
    return (payload) => {
      if (!disposed) {
        handler(payload);
      }
    };
  }

  // Registers the three event listeners, all or none: a partial set is taken down again.
  async function listen() {
    const results = await Promise.allSettled([
      api.onStatus(heard((status) => store.dispatch(actions.status(status)))),
      api.onMixChanged(heard((change) => store.dispatch(actions.mixChanged(change)))),
      api.onMeters(
        heard((meters) => {
          if (isLiveRun(store.getState(), meters.run)) {
            for (const listener of [...meterListeners]) {
              listener(meters);
            }
          }
        }),
      ),
    ]);
    const registered = results.filter(({ status }) => status === "fulfilled");
    const refused = results.find(({ status }) => status === "rejected");
    if (refused) {
      for (const { value: unlisten } of registered) {
        unlisten();
      }
      throw refused.reason;
    }
    unlisteners.push(...registered.map(({ value }) => value));
  }

  // Listens to the engine (once) and loads its state. Never rejects: a failure is shown as the
  // console's connection error, and `reconnect` tries again.
  async function runConnect() {
    store.dispatch(actions.connecting());
    try {
      if (unlisteners.length === 0) {
        await listen();
      }
    } catch (error) {
      store.dispatch(actions.connectionFailed(`Cannot hear the engine: ${message(error)}`));
      return;
    }
    if (disposed) {
      return;
    }
    // Before any start, so the automatic start reopens the input the engineer chose last.
    const unread = await readChoice();
    let answer;
    try {
      answer = await api.status();
    } catch (error) {
      store.dispatch(actions.connectionFailed(message(error)));
      return;
    }
    store.dispatch(actions.connected());
    // The presets live in the desk's session, engine or not; a failed read leaves them unknown
    // until the Mix tools read them again.
    if (store.getState().presets === null) {
      await loadPresets();
    }
    if (answer.state === "notRunning" && autoStart) {
      if (unread) {
        // Never the defaults in place of a kept input that could not be read: Try again reads it
        // again and starts.
        store.dispatch(actions.connectionFailed(choiceUnread(unread)));
        return;
      }
      await startChoice();
    } else {
      takeEngineState(answer);
    }
  }

  async function loadDevices(host = store.getState().audio.host) {
    try {
      store.dispatch(actions.devices(host, await api.listInputDevices(host)));
    } catch (error) {
      store.dispatch(actions.devices(host, [], message(error)));
    }
  }

  // Applies a driver or buffer change by restarting the engine, as a DAW does: on the device in
  // use with `keepDevice`, on `deviceFilter` when named, else on the input the choices name for
  // the driver (a new driver: its default input). ASIO loads a kernel-mode driver, so it only ever
  // starts on a driver the engineer picked, which stays the applied driver for every later start;
  // a device named under another driver becomes the applied input. The choices are applied, and
  // kept in the session, only once the engine started with them: a failed start keeps the previous
  // ones. While the engine runs on the default input because the saved one is missing, a buffer
  // change (`keepDevice`) is kept for the next start instead, which tries the saved input again.
  // Answers like `start`.
  async function applyAudio(change, { deviceFilter = null, keepDevice = false } = {}) {
    const state = store.getState();
    if (keepDevice && state.audioNotice?.kind === "fallback") {
      store.dispatch(actions.audio(change));
      choiceUnsaved = true;
      return { ok: true };
    }
    const device = keepDevice ? (state.engine.status?.deviceName ?? null) : deviceFilter;
    let next = audioChoice(state.audio, change);
    if (next.host === "asio" && device) {
      next = audioChoice(next, { asioDriver: device });
    } else if (deviceFilter) {
      next = audioChoice(next, { device: deviceFilter });
    }
    const result = await start({ deviceFilter: device, audio: next });
    if (result.ok) {
      store.dispatch(actions.audio(next));
      await keepChoice();
    }
    return result;
  }

  // Switches the input device: live under WASAPI, by a restart on that device otherwise (under
  // ASIO the device picked becomes the applied driver). The input switched to becomes the applied
  // one, kept for every later start; a refused switch changes nothing.
  async function switchDevice(name) {
    if (switchMethod(store.getState().audio) === "restart") {
      await applyAudio({}, { deviceFilter: name });
      return;
    }
    try {
      await api.switchDevice(name);
    } catch (error) {
      // The engine keeps the device it had, and the choices stay as they were.
      store.dispatch(actions.commandFailed("switch", message(error)));
      await readStatus();
      return;
    }
    store.dispatch(actions.commandDone("switch"));
    store.dispatch(actions.audio({ device: name }));
    if (store.getState().audioNotice?.kind === "fallback") {
      // The engine runs on an input the engineer chose now.
      store.dispatch(actions.audioNotice(null));
    }
    await keepChoice();
    await readStatus();
  }

  // Runs a musician command, then reloads the status so the table follows at once. Answers
  // `{ ok: true }` or `{ ok: false, message }` with the engine's own words.
  async function musicianCommand(run) {
    try {
      await run();
    } catch (error) {
      return { ok: false, message: message(error) };
    }
    await readStatus();
    return { ok: true };
  }

  // The groups as the last answer to a group edit gave them; null until one answered.
  let keptGroups = null;
  // Desk edits, one at a time: each starts once the one before it was answered.
  let editing = Promise.resolve();

  // Replaces one of the lists the desk keeps with `change(list)`. `kind` says where the whole list
  // comes from (null when the desk does not know it), how to send it, what to keep of the answer
  // and how to word it.
  function editList(kind, change) {
    const run = async () => {
      try {
        const base = await kind.base();
        if (base === null) {
          return { ok: false, message: sentenceWithReason(kind.refused, kind.unknown) };
        }
        const outcome = await kind.send(change(base));
        kind.answered(outcome);
        await readStatus();
        return { ok: true, message: kind.format(outcome) };
      } catch (error) {
        return { ok: false, message: sentenceWithReason(kind.refused, message(error)) };
      }
    };
    const result = editing.then(run);
    editing = result;
    return result;
  }

  // The channel details: always the whole table the desk keeps, read afresh, never the status's
  // part of it (which leaves out the inputs the device lacks).
  const CHANNEL_LIST = Object.freeze({
    base: () => api.channelTable(),
    send: (channels) => api.setChannels(channels),
    answered: () => {},
    format: formatChannelsSet,
    refused: "The channel details were not changed",
  });
  // The groups: a status lists every group, so the last answer's list, or the status's.
  const GROUP_LIST = Object.freeze({
    base: () => {
      const status = store.getState().engine.status;
      return keptGroups ?? (status ? groupDefinitionsOf(status) : null);
    },
    send: (groups) => api.setGroups(groups),
    // An answer numbers inputs from 1, as the request did; a status from 0.
    answered: (outcome) => {
      keptGroups = groupDefinitions(outcome.groups, 1);
    },
    format: formatGroupsSet,
    refused: "The groups were not changed",
    unknown: "no engine is running, so this desk does not know them yet; start the engine first",
  });

  // What the console calls the musician at `address`: their name, else their IP.
  function labelAt(address) {
    const musician = musicianByAddress(store.getState(), address);
    return musician ? musicianLabel(musician) : address;
  }

  // Reads the desk's presets into the store. Resolves to null, or to the error of a failed read
  // (the presets stay as they were known, or unknown).
  async function loadPresets() {
    try {
      store.dispatch(actions.presets(await api.presets()));
      return null;
    } catch (error) {
      return error;
    }
  }

  // Runs one of the Mix tools' commands: `run` resolves to the outcome in words, and the status is
  // read after it. A refusal is said after `refused`. Answers `{ ok, message }`.
  async function mixCommand(run, refused) {
    try {
      const said = await run();
      await readStatus();
      return { ok: true, message: said };
    } catch (error) {
      return { ok: false, message: sentenceWithReason(refused, message(error)) };
    }
  }

  // The name of the preset `id` as the console knows it, or a plain word for one it does not.
  function presetName(id) {
    return store.getState().presets?.find((preset) => preset.id === id)?.name ?? "the preset";
  }

  // The window's title follows the input device, as the old window's did.
  let title = windowTitle(initialState);
  store.subscribe((state) => {
    const next = windowTitle(state);
    if (next !== title && bridge.setTitle) {
      title = next;
      Promise.resolve()
        .then(() => bridge.setTitle(next))
        .catch((error) => console.warn("could not update the window title", error));
    }
  });

  // One connection attempt at a time: a second call while one is on its way joins it, so two
  // quick Try again presses can never listen twice.
  let connecting = null;
  function connect() {
    connecting ??= runConnect().finally(() => {
      connecting = null;
    });
    return connecting;
  }

  const ready = connect();

  return {
    store,
    /** Resolves once the first status is in, the first start has been tried, or it failed. */
    ready,
    /** Connects again after a failure: listens if it does not yet, and reloads the status. */
    reconnect: connect,
    /** Starts the engine on the applied choices (Start, Try again); see startChoice. */
    start: startChoice,
    stop,
    loadDevices,
    applyAudio,
    /**
     * Keeps an audio choice for the next start without restarting anything; the session keeps it
     * once a start runs it.
     */
    setAudio(change) {
      store.dispatch(actions.audio(change));
      choiceKnown = true;
      choiceUnsaved = true;
    },
    switchDevice,
    addTarget: (address) => musicianCommand(() => api.addTarget(address)),
    removeTarget: (address) => musicianCommand(() => api.removeTarget(address)),
    rename: (address, name) => musicianCommand(() => api.setMusicianName(address, name)),
    /**
     * Replaces the channel details with `change(details)`, a pure edit of the whole table in
     * channel order as `channel_table` reads it (see channels-view.js). Answers `{ ok, message }`:
     * the outcome in words (formatChannelsSet), or why nothing changed.
     */
    editChannels: (change) => editList(CHANNEL_LIST, change),
    /** Replaces the groups with `change(groups)`, as editChannels does (see formatGroupsSet). */
    editGroups: (change) => editList(GROUP_LIST, change),
    /** Reads the desk's presets again; resolves to null, or to the error of a failed read. */
    loadPresets,
    /** Saves `address`'s mix as a new preset called `name`; answers `{ ok, message }`. */
    savePreset: (address, name) =>
      mixCommand(async () => {
        const outcome = await api.savePreset(address, name);
        store.dispatch(actions.presets(outcome.presets));
        return `Saved ${labelAt(address)}'s mix as "${outcome.preset.name}".`;
      }, "The preset was not saved"),
    /** Renames the preset `id` to `name`; answers `{ ok, message }`. */
    renamePreset: (id, name) =>
      mixCommand(async () => {
        const before = presetName(id);
        const outcome = await api.renamePreset(id, name);
        store.dispatch(actions.presets(outcome.presets));
        return `Renamed "${before}" to "${outcome.preset.name}".`;
      }, "The preset was not renamed"),
    /** Deletes the preset `id`; answers `{ ok, message }`. */
    deletePreset: (id) =>
      mixCommand(async () => {
        const name = presetName(id);
        const outcome = await api.deletePreset(id);
        store.dispatch(actions.presets(outcome.presets));
        return `Deleted the preset "${name}".`;
      }, "The preset was not deleted"),
    /** Puts the preset `id` on the mixes of `addresses`; answers `{ ok, message }`. */
    applyPreset: (id, addresses) =>
      mixCommand(async () => {
        const outcome = await api.applyPreset(id, addresses);
        return formatPresetApplied(
          presetName(id),
          outcome.mixes.map(({ address }) => labelAt(address)),
        );
      }, "The preset was not applied"),
    /** Copies `from`'s mix onto the mixes of `to`; answers `{ ok, message }`. */
    copyMix: (from, to) =>
      mixCommand(async () => {
        const outcome = await api.copyMix(from, to);
        return formatCopied(
          labelAt(from),
          outcome.mixes.map(({ address }) => labelAt(address)),
        );
      }, "The mix was not copied"),
    /**
     * Makes `address`'s mix follow the mix of the musician at `leaderAddress`, or stops it with
     * null; the leader's phone's client id and the name this console shows for them go with it, so
     * the follower's phone says "Following <name>'s mix". Answers `{ ok, message }`.
     */
    setMirror: (address, leaderAddress) =>
      mixCommand(async () => {
        const state = store.getState();
        const follower = musicianByAddress(state, address);
        const leader = leaderAddress ? musicianByAddress(state, leaderAddress) : null;
        if (leaderAddress && !leader) {
          throw new Error(`no musician is listed at ${leaderAddress}`);
        }
        const request = leader
          ? { address: leader.address, clientId: leader.clientId ?? null, name: musicianLabel(leader) }
          : null;
        const outcome = await api.setMirror(address, request, follower?.clientId ?? null);
        return formatMirrorSet(outcome, labelAt(address), leader ? musicianLabel(leader) : null);
      }, "The mirror was not changed"),
    select(address) {
      store.dispatch(actions.select(address));
    },
    /**
     * Local edits to `address`'s mix, keyed as editKey (mix.js) makes them: mix edits go out with
     * `set_mix`, edits of the engineer's limit and locks with `set_engineer_controls`.
     */
    edit(address, edits) {
      const before = store.getState();
      if (store.dispatch(actions.mixEdit(address, edits)) === before) {
        return;
      }
      const { mix, engineer } = splitEdits(edits);
      if (Object.keys(mix).length > 0) {
        sender.push(address, null);
      }
      if (Object.keys(engineer).length > 0) {
        engineerSender.push(address, null);
      }
    },
    /** Calls `listener` with every `meters` payload of the live run; returns the unsubscribe. */
    onMeters(listener) {
      meterListeners.add(listener);
      return () => meterListeners.delete(listener);
    },
    /** Stops listening to the engine; the engine itself keeps running. */
    dispose() {
      disposed = true;
      meterListeners.clear();
      for (const unlisten of unlisteners.splice(0)) {
        unlisten();
      }
    },
  };
}
