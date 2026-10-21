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
//   `set_engineer_controls`; an answer that did not both apply and save is kept to be said. The
//   engineer's local mix (LOCAL_MIX) goes the same way with `set_local_mix`.
// - The local mix's output (the Network and system screen's "Local mix out"): `loadOutputs` lists
//   the outputs the running engine offers (an answer for a run that has ended is not taken),
//   `setLocalOutput` plays the local mix on one or none, one choice at a time with the last one
//   winning, and answers `{ ok, message, superseded }` (formatLocalOutputSet), reading the status
//   after it.
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
// - Named sessions (the Network and system screen's Session panel): the saved sessions and what the
//   desk said when it opened its session (both read once connected, `loadSessions` again), save,
//   rename, delete and load. A load waits for the channel and group edits on their way and holds
//   the waiting mix edits back while it loads: once it loaded they are dropped, and if it was
//   refused they are kept and go out. From its answer on the groups are the loaded ones (the last
//   group answer is forgotten), the presets are the loaded ones and, when its audio input was
//   taken, the next start opens it (the engine is never restarted). `unsavedEdits()` counts the
//   edits a load would drop, so the panel can ask first. Each answers `{ ok, message }` and reads
//   the list again.
// - Mix backups (the Session panel too; see mix-backups-view.js): the backups of the musicians'
//   mixes the desk takes as they change (read once connected, `loadMixBackups` again), and
//   `restoreMixBackup(stamp)`, which puts one back on the musicians who are connected now, in turn
//   with the desk edits and loads. Both answer `{ ok, message }`; a restore reads the status and the
//   backups after it.
// - A phone's playback buffer (the Network and system screen; see playback-buffer.js):
//   `setMusicianBuffer(musicianKey, mode, ms)` sets Auto or a fixed length, in turn with the desk
//   edits and loads (a load replaces the buffers), answers `{ ok, message }` (formatBufferSet) and
//   reads the status after it.
// - A musician's packet redundancy (the Network and system screen; see redundancy.js):
//   `setMusicianRedundancy(musicianKey, on, delayBlocks)` turns it on or off, in turn with the desk
//   edits and loads (a load replaces the settings), answers `{ ok, message }` (formatRedundancySet)
//   and reads the status after it.
// - The master limiter on every mix (the master strip; see limiter.js): `editMixLimiter(address,
//   settings)` for a musician's address or LOCAL_MIX, shown at once over the status's (mixLimiter)
//   and sent through a per-mix coalescer (one request in flight per mix, the latest change always
//   sent); answers `{ ok, message }` (formatLimiterSet) once that mix's changes settled, and reads
//   the status after each answer.
// - The shared effects (see effects.js): `editChannelSend(channel, effect, level)`,
//   `editEffect(effect, settings)` and `editMixReturn(address, effect, level)`, each shown at once
//   over what the desk keeps and sent through its own coalescer (per channel and effect, per effect,
//   per mix and effect); each answers `{ ok, message }` once settled.
// - Each input's channel processing (see processing.js): every channel's strip (read once
//   connected and after a loaded session, `loadProcessing` again), `editProcessing(channel,
//   settings)` through a per-channel coalescer (one request in flight per channel, the latest edit
//   always sent, the edit shown until its answer settles it), and `resetChannelProcessing`,
//   `copyChannelProcessing` and `applyStripPreset`, in turn with the desk edits and loads and after
//   the strip edits of the channels they touch. The strip presets (read once connected,
//   `loadStripPresets` again) change with `saveStripPreset` and `deleteStripPreset`, and
//   `loadEqResponse(channel, eq)` keeps the equaliser's curve for a channel. Each answers `{ ok,
//   message }`. A load holds the strip edits back like the mix edits, and drops them once loaded.
// - Recording takes (the Record screen; see recording.js): the recording settings (read once
//   connected, `loadRecordingSettings` again) change with `setRecordingSettings(change)`, one change
//   at a time and each built on the one before it, so two quick toggles both land; a refusal leaves
//   the settings as they were and the next change builds on those. `startRecording(name)` starts a
//   take with them, its folder named in the window's local time; `stopRecording()` stops it and,
//   while its files are still being finished, asks again a few times a short wait apart before
//   saying so; `addMarker(name)` marks it; `refreshTakes()` lists the takes, `refreshEstimate()`
//   reads what a take would take on the disk (once connected, when an engine starts and after
//   every settings change), and
//   `openTakeFolder(folder)` opens one in the file manager. Each command answers `{ ok, message }`
//   (and keeps it as the store's `recordingOutcome`) and reads the status after it. While a take
//   records, a device switch and a driver change are refused in words before anything reaches the
//   engine, which refuses a switch too: a take never mixes two devices.
// - The virtual soundcheck (the Record screen; see soundcheck.js): `startSoundcheck(folder, {
//   startFrame, looping })` plays a take in place of the live inputs (a take's Play is this; there
//   is no separate playback), `pauseSoundcheck`, `resumeSoundcheck`, `seekSoundcheck(frame)` (one
//   at a time, the latest winning), `setSoundcheckLoop(looping)` and `stopSoundcheck()` (back to
//   the live inputs). Each answers `{ ok, message, soundcheck }`, shows the answer on the status at
//   once and says the outcome as the store's `recordingOutcome` ("soundcheck"). While a soundcheck
//   plays, a recording, a device switch and a driver change are refused in words, and while a take
//   records no soundcheck starts; the engine refuses each too.

import { createEngineApi } from "./bridge.js";
import { createCoalescer } from "./coalesce.js";
import {
  LOCAL_MIX,
  actions,
  consoleReducer,
  initialState,
  isLiveRun,
  isRecordingNow,
  isSoundcheckNow,
  mixOf,
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
import { formatLocalOutputSet } from "./local-mix.js";
import {
  RECORDING_BLOCKS_SWITCH,
  formatDuration,
  formatStarted,
  formatStopped,
  settingsRequest,
} from "./recording.js";
import {
  RECORDING_BLOCKS_SOUNDCHECK,
  SOUNDCHECK_BLOCKS_RECORDING,
  SOUNDCHECK_BLOCKS_SWITCH,
  SOUNDCHECK_REFUSED,
  formatSoundcheckDone,
} from "./soundcheck.js";
import { buildEngineerRequest, buildMixRequest, splitEdits } from "./mix.js";
import { formatCopied, formatMirrorSet, formatPresetApplied } from "./mixes.js";
import { switchMethod, windowTitle } from "./network-view.js";
import { formatMixRestored } from "./mix-backups-view.js";
import { clampLimiter, formatLimiterSet } from "./limiter.js";
import { clampEffects, clampLevel, clampLevels, formatEffectSet, formatReturnSet } from "./effects.js";
import { musicianKey } from "./link-health-view.js";
import { formatBufferSet, nameForKey } from "./playback-buffer.js";
import { formatProcessingSet, formatStripPresetsSet, stripUnitChannels } from "./processing.js";
import { blockMsOf, formatRedundancySet } from "./redundancy.js";
import { formatSessionLoaded, formatSessionSaved, skippedName } from "./sessions.js";
import { createStore } from "./store.js";

// After a start whose status could not be read: how often and how far apart to ask again.
const STATUS_RETRIES = 3;
const STATUS_RETRY_MS = 500;

// A stop whose take's files are still being finished: how often and how far apart to ask again.
const STOP_RETRIES = 5;
const STOP_RETRY_MS = 300;

function message(error) {
  return error instanceof Error ? error.message : String(error);
}

function wait(ms) {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    timer.unref?.();
  });
}

/**
 * The controller over `bridge`. `autoStart` starts an engine when none runs; `statusRetryMs` is the
 * wait between status reads after a start whose status could not be read; `stopRetries` and
 * `stopRetryMs` how often and how far apart a stop is asked again while the take's files are still
 * being finished.
 */
export function createConsoleApp({
  bridge,
  autoStart = true,
  statusRetryMs = STATUS_RETRY_MS,
  stopRetries = STOP_RETRIES,
  stopRetryMs = STOP_RETRY_MS,
}) {
  const api = createEngineApi(bridge);
  const store = createStore(consoleReducer, initialState);
  const meterListeners = new Set();
  const unlisteners = [];
  let disposed = false;

  // While a saved session is loading, the waiting edits stay where they are: none goes out over
  // the loaded session; they are dropped once it loaded, and sent as usual if it was refused.
  let loading = false;
  // Resolves once the load running now is over, its held edits dropped or sent; null when none runs.
  let loadEnded = null;

  // Sends `address`'s pending mix edits as they are when the send goes out: a musician's with
  // `set_mix`, the local mix's (LOCAL_MIX) with `set_local_mix`. The coalescer calls it at most
  // once at a time per mix, and again after an answer when edits are still waiting.
  async function sendMix(address) {
    const state = store.getState();
    const mix = mixOf(state, address);
    const sent = splitEdits(pendingEdits(state, address)).mix;
    if (loading || !mix || Object.keys(sent).length === 0) {
      return;
    }
    try {
      const request = buildMixRequest(mix, sent);
      const answer =
        address === LOCAL_MIX ? await api.setLocalMix(request) : await api.setMix(address, request);
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
    if (loading || !musician || Object.keys(sent).length === 0) {
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

  // The last answer to a strip edit of each channel, in words, for editProcessing's promise.
  const processingSaid = new Map();

  // Sends `channel`'s strip edit still waiting with `set_channel_processing`, as sendMix sends a
  // mix: at most one at a time per channel, the latest edit always sent.
  async function sendProcessing(channel) {
    const sent = store.getState().processingPending[channel];
    if (loading || !sent) {
      return;
    }
    try {
      const answer = await api.setChannelProcessing(channel, sent);
      const said = formatProcessingSet(answer);
      processingSaid.set(channel, said);
      if (said.ok) {
        store.dispatch(actions.processingApplied(channel, sent, answer));
        await readStripsIfUnknown();
      } else {
        store.dispatch(actions.processingFailed(channel, sent, said.message));
      }
    } catch (error) {
      const said = {
        ok: false,
        message: sentenceWithReason(`CH ${channel}'s processing was not changed`, message(error)),
      };
      processingSaid.set(channel, said);
      store.dispatch(actions.processingFailed(channel, sent, said.message));
    }
  }

  const processingSender = createCoalescer(sendProcessing);

  // Reads every strip once a change was answered while they were unknown (never read yet, or not
  // read after a load): the answer names only the strips it changed.
  async function readStripsIfUnknown() {
    if (store.getState().processing === null) {
      await loadProcessing();
    }
  }

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
    // The run before it took its estimate with it; this engine's device decides the tracks.
    await refreshEstimate();
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
    // until the Mix tools read them again. So do the saved sessions and the desk's word about its
    // session, which the Session panel shows.
    if (store.getState().presets === null) {
      await loadPresets();
    }
    // So are each channel's processing and the strip presets.
    if (store.getState().processing === null) {
      await loadProcessing();
    }
    if (store.getState().stripPresets === null) {
      await loadStripPresets();
    }
    if (store.getState().sessions.list === null) {
      await loadSessions();
    }
    await readSessionNotice();
    // So are the backups of the musicians' mixes.
    if (store.getState().mixBackups.list === null) {
      await loadMixBackups();
    }
    // The recording settings are the desk's too; a failed read leaves them unknown until the Record
    // screen reads them again.
    if (store.getState().recordingSettings === null) {
      await loadRecordingSettings();
    }
    if (answer.state === "notRunning" && autoStart) {
      if (unread) {
        // Never the defaults in place of a kept input that could not be read: Try again reads it
        // again and starts.
        store.dispatch(actions.connectionFailed(choiceUnread(unread)));
        return;
      }
      // A start that runs reads the disk estimate once its engine runs.
      await startChoice();
    } else {
      takeEngineState(answer);
      // What a take would take on the disk, or why that is not known, for the engine found.
      await refreshEstimate();
    }
  }

  async function loadDevices(host = store.getState().audio.host) {
    try {
      store.dispatch(actions.devices(host, await api.listInputDevices(host)));
    } catch (error) {
      store.dispatch(actions.devices(host, [], message(error)));
    }
  }

  // Lists the outputs the running engine offers for the local mix. Resolves to null, or to the
  // error of a failed listing (with no engine running there is nothing to list), which is kept to
  // be said. The answer names the run it was asked for, so one that comes back after another
  // engine started is not taken (see console-state.js).
  async function loadOutputs() {
    const { run } = store.getState().engine;
    try {
      store.dispatch(actions.outputs(await api.outputDevices(), run));
      return null;
    } catch (error) {
      store.dispatch(actions.outputsFailed(message(error), run));
      return error;
    }
  }

  // Output choices go out one at a time: a choice waits for the one in flight, and a choice
  // overtaken while it waits never goes out. `outputChoice` counts them, so each answer knows
  // whether a newer choice came after it.
  let outputLane = Promise.resolve();
  let outputChoice = 0;

  // Plays the local mix on `output` (`{ device, firstChannel }`) or nowhere with null, then reads
  // the status, which says where it plays. Answers `{ ok, message, superseded }`; `superseded`
  // says a newer choice came after this one (it may not even have gone out), so only the last
  // choice's outcome is worth saying. Every choice settles with an answer: a refusal, an answer
  // that cannot be read and a status that cannot be read are said in `message`, and none of them
  // ever stops a later choice from going out.
  function setLocalOutput(output) {
    const choice = ++outputChoice;
    const overtaken = () => choice !== outputChoice;
    const send = async () => {
      if (overtaken()) {
        return { ok: true, message: null, superseded: true };
      }
      let answer;
      try {
        answer = await api.setLocalOutput(output);
      } catch (error) {
        const refused = sentenceWithReason("The local mix output was not changed", message(error));
        return settleOutput({ ok: false, message: refused }, overtaken);
      }
      let words;
      try {
        words = formatLocalOutputSet(answer);
      } catch (error) {
        words = sentenceWithReason(
          "The local mix output was changed, but the desk's answer could not be read",
          message(error),
        );
      }
      return settleOutput({ ok: true, message: words }, overtaken);
    };
    // The lane itself never stays rejected: whatever happens to one choice, the next goes out.
    const answer = outputLane.then(send).catch((error) => ({
      ok: false,
      message: sentenceWithReason("The local mix output may not have changed", message(error)),
      superseded: overtaken(),
    }));
    outputLane = answer;
    return answer;
  }

  // Reads the status after an output choice and finishes its answer: a status that cannot be read
  // is said after the outcome, and `superseded` is worked out last.
  async function settleOutput(result, overtaken) {
    let unread;
    try {
      unread = await readStatus();
    } catch (error) {
      unread = error;
    }
    const unknown = unread
      ? sentenceWithReason("Where it plays now could not be read", message(unread))
      : null;
    const words = unknown ? `${result.message} ${unknown}` : result.message;
    return { ...result, message: words, superseded: overtaken() };
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
    if (isRecordingNow(state)) {
      // A restart would end the take, and a new driver or input would change its source.
      store.dispatch(actions.commandFailed("switch", RECORDING_BLOCKS_SWITCH));
      return { ok: false, error: RECORDING_BLOCKS_SWITCH };
    }
    if (isSoundcheckNow(state)) {
      // A restart would drop the take playing in place of the inputs, and the engine refuses a
      // switch under it.
      store.dispatch(actions.commandFailed("switch", SOUNDCHECK_BLOCKS_SWITCH));
      return { ok: false, error: SOUNDCHECK_BLOCKS_SWITCH };
    }
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
    if (isRecordingNow(store.getState())) {
      store.dispatch(actions.commandFailed("switch", RECORDING_BLOCKS_SWITCH));
      return;
    }
    if (isSoundcheckNow(store.getState())) {
      store.dispatch(actions.commandFailed("switch", SOUNDCHECK_BLOCKS_SWITCH));
      return;
    }
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

  // The groups as the last answer to a group edit gave them; null until one answered, and again
  // once a saved session was loaded (its groups are the desk's then).
  let keptGroups = null;
  // Desk edits and session loads, one at a time: each starts once the one before it was answered.
  let editing = Promise.resolve();
  // The channel and group edits asked for and not answered yet.
  let editsOnTheirWay = 0;

  // Runs `run` after every desk edit and load asked for before it, and keeps the next one waiting
  // for it. `run` never rejects.
  function inTurn(run) {
    const result = editing.then(run);
    editing = result;
    return result;
  }

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
      } finally {
        editsOnTheirWay -= 1;
      }
    };
    editsOnTheirWay += 1;
    return inTurn(run);
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

  // Reads the saved sessions into the store. Resolves to null, or to the error of a failed read
  // (the list stays as it was known, or unknown, and the failure is kept to be said).
  async function loadSessions() {
    try {
      store.dispatch(actions.sessions(await api.sessions()));
      return null;
    } catch (error) {
      store.dispatch(actions.sessionsFailed(message(error)));
      return error;
    }
  }

  // Reads what the desk said when it opened its session. A failed read says nothing.
  async function readSessionNotice() {
    try {
      store.dispatch(actions.sessionNotice(await api.sessionNotice()));
    } catch {
      // The notice is a courtesy: the next connect asks again.
    }
  }

  // The name of the saved session `id` as the console lists it, or a plain word for one it does not.
  function sessionName(id) {
    return (
      store.getState().sessions.list?.find((saved) => saved.id === id)?.name ?? "the saved session"
    );
  }

  // Runs one of the Session panel's commands: `run` resolves to the outcome in words, and the list
  // of saved sessions is read again after it, whatever the answer. A refusal is said after
  // `refused`. Answers `{ ok, message }`.
  async function sessionCommand(run, refused) {
    let result;
    try {
      result = { ok: true, message: await run() };
    } catch (error) {
      result = { ok: false, message: sentenceWithReason(refused, message(error)) };
    }
    await loadSessions();
    return result;
  }

  // Reads the backups of the musicians' mixes into the store. Answers `{ ok, message }`; a failed
  // read keeps the backups as they were known, or unknown, and the failure is kept to be said.
  async function loadMixBackups() {
    try {
      const list = await api.mixBackups();
      store.dispatch(actions.mixBackups(list));
      return { ok: true, message: list.length === 1 ? "1 mix backup." : `${list.length} mix backups.` };
    } catch (error) {
      store.dispatch(actions.mixBackupsFailed(message(error)));
      return {
        ok: false,
        message: sentenceWithReason("The mix backups could not be listed", message(error)),
      };
    }
  }

  // Puts the backup `stamp` back on the musicians who are connected, once every desk edit and load
  // asked for before it was answered. Answers `{ ok, message }` (formatMixRestored), then reads the
  // status and the backups again, whatever the answer.
  function restoreMixBackup(stamp) {
    return inTurn(async () => {
      let result;
      try {
        const outcome = await api.restoreMixBackup(stamp);
        await readStatus();
        const musicians = store.getState().musicians;
        result = {
          ok: true,
          message: formatMixRestored(outcome, {
            labelAt,
            nameOf: (skipped) => skippedName(skipped, musicians),
          }),
        };
      } catch (error) {
        result = { ok: false, message: sentenceWithReason("The mixes were not restored", message(error)) };
      }
      await loadMixBackups();
      return result;
    });
  }

  // Sets the playback buffer of the musician `musicianKey` names once every desk edit and load
  // asked for before it was answered (a load replaces the buffers). Answers `{ ok, message }`
  // (formatBufferSet) and reads the status after an answer.
  function setMusicianBuffer(musicianKey, mode, ms = null) {
    return inTurn(async () => {
      try {
        const answer = await api.setMusicianBuffer(musicianKey, mode, ms);
        await readStatus();
        return formatBufferSet(answer, nameForKey(store.getState().musicians, musicianKey));
      } catch (error) {
        return { ok: false, message: sentenceWithReason("The buffer was not set", message(error)) };
      }
    });
  }

  // Turns redundancy on or off for the musician `musicianKey` names once every desk edit and load
  // asked for before it was answered (a load replaces the settings). Answers `{ ok, message }`
  // (formatRedundancySet) and reads the status after an answer.
  function setMusicianRedundancy(musicianKey, on, delayBlocks = null) {
    return inTurn(async () => {
      try {
        const answer = await api.setMusicianRedundancy(musicianKey, on, delayBlocks);
        await readStatus();
        const state = store.getState();
        return formatRedundancySet(
          answer,
          nameForKey(state.musicians, musicianKey),
          blockMsOf(state.engine?.status?.captureFormat),
        );
      } catch (error) {
        return { ok: false, message: sentenceWithReason("Redundancy was not set", message(error)) };
      }
    });
  }

  // The last answer to a limiter change of each mix, in words.
  const limiterSaid = new Map();

  // Sends the limiter change `sent` of the mix `address`, then reads the status after its answer;
  // the change shown meanwhile is settled either way. Only the command decides whether it was set:
  // a status that cannot be read after a set leaves the set, and the strip shows what the desk
  // answered until the next status.
  //
  // The change is settled on every path (in `finally`), even when the answer cannot be read, so
  // the strip never stays on a change that is no longer on its way.
  async function sendLimiter(address, sent) {
    const local = address === LOCAL_MIX;
    let name = local ? "The local mix" : address;
    let kept = null;
    try {
      const musician = local ? null : musicianByAddress(store.getState(), address);
      name = local ? name : musician ? musicianLabel(musician) : address;
      if (!local && !musician) {
        throw new Error(`${name} is not here`);
      }
      const answer = local
        ? await api.setLocalLimiter(sent)
        : await api.setMusicianLimiter(musicianKey(musician), sent);
      const said = formatLimiterSet(answer, name);
      const settings = said.ok ? clampLimiter(answer.settings) : null;
      limiterSaid.set(address, said);
      const unread = await readStatus();
      kept = unread ? settings : null;
    } catch (error) {
      const whose = local ? "The local mix's" : `${name}'s`;
      limiterSaid.set(address, {
        ok: false,
        message: sentenceWithReason(`${whose} limiter was not set`, message(error)),
      });
    } finally {
      store.dispatch(actions.limiterSettled(address, sent, kept));
    }
  }

  const limiterSender = createCoalescer(sendLimiter);

  // Changes the master limiter of the mix `address` (a musician's address, or LOCAL_MIX) to
  // `settings`: shown at once, sent through the mix's coalescer, answered once that mix's changes
  // settled.
  async function editMixLimiter(address, settings) {
    const sent = clampLimiter(settings);
    store.dispatch(actions.limiterEdit(address, sent));
    limiterSender.push(address, sent);
    await limiterSender.idle(address);
    return limiterSaid.get(address) ?? { ok: false, message: "The limiter was not set." };
  }

  // The last answer to a send of each channel and effect, a change of each effect and a return of
  // each mix and effect, in words.
  const sendSaid = new Map();
  const effectSaid = new Map();
  const returnSaid = new Map();

  // Sends the send `level` of `key` ("channel:effect") and applies the strips the desk answers;
  // the send shown meanwhile is settled either way.
  async function sendChannelSend(key, level) {
    const [channel, effect] = [Number(key.split(":")[0]), key.split(":")[1]];
    try {
      const answer = await api.setChannelSend(channel, effect, level);
      const said = formatProcessingSet(answer);
      sendSaid.set(key, said);
      if (said.ok) {
        store.dispatch(actions.processingApplied(channel, null, answer));
        await readStripsIfUnknown();
      }
    } catch (error) {
      sendSaid.set(key, {
        ok: false,
        message: sentenceWithReason(`CH ${channel}'s ${effect} send was not changed`, message(error)),
      });
    } finally {
      store.dispatch(actions.sendSettled(channel, effect, level));
    }
  }

  const sendSender = createCoalescer(sendChannelSend);

  // Changes the send of input `channel` (1-based) to `effect` ("reverb" or "delay") to `level`
  // percent: shown at once, sent through a coalescer per channel and effect (the latest level
  // wins), answered `{ ok, message }` once settled.
  async function editChannelSend(channel, effect, level) {
    const key = `${channel}:${effect}`;
    const held = clampLevel(level);
    store.dispatch(actions.sendEdit(channel, effect, held));
    sendSender.push(key, held);
    await sendSender.idle(key);
    return sendSaid.get(key) ?? { ok: false, message: "The send was not changed." };
  }

  // Sends the effect change `sent` and keeps both effects as the desk answered them.
  async function sendEffect(effect, sent) {
    let answer;
    try {
      answer = await api.setEffect(effect, sent);
    } catch (error) {
      effectSaid.set(effect, {
        ok: false,
        message: sentenceWithReason(`The ${effect} was not set`, message(error)),
      });
      store.dispatch(actions.effectSettled(effect, sent));
      return;
    }
    const said = formatEffectSet(answer);
    effectSaid.set(effect, said);
    store.dispatch(actions.effectSettled(effect, sent, said.ok ? clampEffects(answer.effects) : null));
  }

  const effectSender = createCoalescer(sendEffect);

  // Changes the shared `effect`'s settings to `settings` (the whole effect): shown at once, sent
  // through a coalescer per effect, answered `{ ok, message }` once settled.
  async function editEffect(effect, settings) {
    const sent = clampEffects({ [effect]: settings })[effect];
    store.dispatch(actions.effectEdit(effect, sent));
    effectSender.push(effect, sent);
    await effectSender.idle(effect);
    return effectSaid.get(effect) ?? { ok: false, message: "The effect was not set." };
  }

  // Sends the return `level` of `key` ("address|effect") and keeps the mix's returns as answered.
  async function sendReturn(key, level) {
    const cut = key.lastIndexOf("|");
    const [address, effect] = [key.slice(0, cut), key.slice(cut + 1)];
    const local = address === LOCAL_MIX;
    const musician = local ? null : musicianByAddress(store.getState(), address);
    const name = local ? "The local mix" : musician ? musicianLabel(musician) : address;
    let answer;
    try {
      if (!local && !musician) {
        throw new Error(`${name} is not here`);
      }
      answer = local
        ? await api.setLocalFxReturn(effect, level)
        : await api.setMusicianFxReturn(musicianKey(musician), effect, level);
    } catch (error) {
      const whose = local ? "The local mix's" : `${name}'s`;
      returnSaid.set(key, {
        ok: false,
        message: sentenceWithReason(`${whose} ${effect} return was not set`, message(error)),
      });
      store.dispatch(actions.returnSettled(address, effect, level));
      return;
    }
    const said = formatReturnSet(answer, name);
    returnSaid.set(key, said);
    store.dispatch(actions.returnSettled(address, effect, level, said.ok ? clampLevels(answer.returns) : null));
  }

  const returnSender = createCoalescer(sendReturn);

  // Changes the return of `effect` in the mix `address` (a musician's address, or LOCAL_MIX) to
  // `level` percent: shown at once, sent through a coalescer per mix and effect, answered
  // `{ ok, message }` once settled.
  async function editMixReturn(address, effect, level) {
    const key = `${address}|${effect}`;
    const held = clampLevel(level);
    store.dispatch(actions.returnEdit(address, effect, held));
    returnSender.push(key, held);
    await returnSender.idle(key);
    return returnSaid.get(key) ?? { ok: false, message: "The return was not set." };
  }

  // Sends every edit still waiting, as the coalescers would have: after a load that was refused.
  function sendWaiting() {
    for (const channel of Object.keys(store.getState().processingPending)) {
      processingSender.push(Number(channel), null);
    }
    for (const [address, edits] of Object.entries(store.getState().pending)) {
      const { mix, engineer } = splitEdits(edits);
      if (Object.keys(mix).length > 0) {
        sender.push(address, null);
      }
      if (Object.keys(engineer).length > 0) {
        engineerSender.push(address, null);
      }
    }
  }

  // Loads the saved session `id` once every edit asked for before it was answered. The edits still
  // waiting are held back while it loads; once it loaded they are dropped (none was meant for the
  // loaded session) and the window builds its next group edit on the loaded groups. A refused load
  // changed nothing, so they are kept and go out.
  function loadSession(id) {
    return inTurn(async () => {
      loading = true;
      let endLoad;
      loadEnded = new Promise((resolve) => {
        endLoad = resolve;
      });
      let loaded = false;
      try {
        return await sessionCommand(async () => {
          const outcome = await api.loadSession(id);
          loaded = true;
          keptGroups = null;
          if (outcome.audioLoaded) {
            // The session keeps exactly what the next start opens.
            choiceKnown = true;
            choiceUnsaved = false;
          }
          // The strip edits waiting were meant for the session before: they are dropped, and so said.
          for (const channel of Object.keys(store.getState().processingPending)) {
            processingSaid.set(Number(channel), {
              ok: false,
              message: sentenceWithReason(
                `CH ${channel}'s processing was not changed`,
                "a session was loaded meanwhile",
              ),
            });
          }
          store.dispatch(actions.sessionLoaded(outcome));
          await readStatus();
          // The loaded session's processing replaced every strip; strips that cannot be read are
          // no longer known, and the answer says so.
          const unread = await loadProcessing();
          if (unread) {
            store.dispatch(
              actions.processingUnknown(
                sentenceWithReason(
                  "The loaded session's channel processing could not be read",
                  message(unread),
                ),
              ),
            );
          }
          const musicians = store.getState().musicians;
          const words = formatSessionLoaded(outcome, {
            labelAt,
            nameOf: (skipped) => skippedName(skipped, musicians),
          });
          return unread
            ? `${words} ${sentenceWithReason("Its channel processing could not be read", message(unread))}`
            : words;
        }, `"${sessionName(id)}" was not loaded`);
      } finally {
        loading = false;
        // A refused load changed nothing, so the edits held back go out; after a load only the
        // edits made once it landed are left, meant for the loaded session.
        sendWaiting();
        loadEnded = null;
        endLoad();
      }
    });
  }

  // The edits a load would drop: every mix and strip edit still waiting and every channel or group
  // edit not answered yet.
  function unsavedEdits() {
    const { pending, processingPending } = store.getState();
    const mixEdits = Object.values(pending).reduce(
      (count, edits) => count + Object.keys(edits).length,
      0,
    );
    return mixEdits + Object.keys(processingPending).length + editsOnTheirWay;
  }

  // Reads every channel's strip into the store. Resolves to null, or to the error of a failed read
  // (the strips stay as they were known, or unknown).
  async function loadProcessing() {
    try {
      store.dispatch(actions.processing(await api.processing()));
      return null;
    } catch (error) {
      return error;
    }
  }

  // Reads the strip presets into the store, as loadProcessing reads the strips.
  async function loadStripPresets() {
    try {
      store.dispatch(actions.stripPresets(await api.stripPresets()));
      return null;
    } catch (error) {
      return error;
    }
  }

  // Edits `channel`'s strip to `settings`, the whole strip: shown at once (channelStrip) and sent
  // through the channel's coalescer, so a knob dragged sends only its latest value after the one in
  // flight. Resolves to the last answer in words once the channel has nothing left to send.
  // A session load holds the edit back: it is dropped once the load lands (and so said), or sent
  // once it was refused, so the answer waits for the load too and is never a success not sent.
  async function editProcessing(channel, settings) {
    processingSaid.delete(channel);
    store.dispatch(actions.processingEdit(channel, settings));
    processingSender.push(channel, null);
    await processingSender.idle(channel);
    while (loadEnded) {
      await loadEnded;
      await processingSender.idle(channel);
    }
    return (
      processingSaid.get(channel) ?? {
        ok: false,
        message: sentenceWithReason(`CH ${channel}'s processing was not changed`, "it was not sent"),
      }
    );
  }

  // Runs a processing change that is not an edit (a reset, a copy, a preset) once every desk edit
  // and load asked for before it was answered and the strip edits of `channels` and of their
  // linked partners went out, so it lands after them. Answers `{ ok, message }`
  // (formatProcessingSet).
  function processingCommand(channels, send, refused) {
    return inTurn(async () => {
      const touched = stripUnitChannels(store.getState().engine.status?.channels, channels);
      await Promise.all(touched.map((channel) => processingSender.idle(channel)));
      try {
        const answer = await send();
        const said = formatProcessingSet(answer);
        if (said.ok) {
          store.dispatch(actions.processingApplied(null, null, answer));
          await readStripsIfUnknown();
        }
        return said;
      } catch (error) {
        return { ok: false, message: sentenceWithReason(refused, message(error)) };
      }
    });
  }

  // Runs a strip preset change: `send` resolves to the desk's answer, whose presets the store
  // takes. Answers `{ ok, message }` (formatStripPresetsSet).
  async function stripPresetCommand(send, refused) {
    try {
      const answer = await send();
      if (answer?.presets) {
        store.dispatch(actions.stripPresets(answer.presets));
      }
      return formatStripPresetsSet(answer);
    } catch (error) {
      return { ok: false, message: sentenceWithReason(refused, message(error)) };
    }
  }

  // Works out the response of the equaliser `eq` for `channel`'s curve and keeps it in the store.
  // Answers `{ ok, message, points }`.
  async function loadEqResponse(channel, eq, points = null) {
    try {
      const curve = await api.eqResponse(eq, points);
      store.dispatch(actions.eqCurve(channel, curve));
      return { ok: true, message: "", points: curve };
    } catch (error) {
      return {
        ok: false,
        message: sentenceWithReason("The equaliser's curve could not be drawn", message(error)),
        points: null,
      };
    }
  }

  // Reads the recording settings into the store. Resolves to null, or to the error of a failed
  // read (the settings stay as they were known, or unknown).
  async function loadRecordingSettings() {
    try {
      store.dispatch(actions.recordingSettings(await api.recordingSettings()));
      return null;
    } catch (error) {
      return error;
    }
  }

  // Keeps `{ action, ok, message }` as the last recording outcome and answers it with `extra`.
  function recordingSaid(action, ok, words, extra = {}) {
    store.dispatch(actions.recordingOutcome(action, ok, words));
    return { ok, message: words, ...extra };
  }

  // Settings changes go out one at a time, each built when its turn comes on the settings the desk
  // answered last, which hold every change before it that was kept, so a second toggle made before
  // the first was answered keeps the first (an arm press is a delta, `arm`, for the same reason). A
  // refused change is in no later request: those carry only the kept settings and their own fields.
  let settingsLane = Promise.resolve();

  function setRecordingSettings(change) {
    const send = async () => {
      const request = settingsRequest(store.getState().recordingSettings, change);
      let answer;
      try {
        answer = await api.setRecordingSettings(request);
      } catch (error) {
        return recordingSaid(
          "settings",
          false,
          sentenceWithReason("The recording settings were not changed", message(error)),
        );
      }
      store.dispatch(actions.recordingSettings(answer));
      await refreshEstimate();
      return recordingSaid("settings", true, "The recording settings were saved.");
    };
    const result = settingsLane.then(send);
    settingsLane = result.catch(() => null);
    return result;
  }

  // Reads what a take with the kept settings would take on the disk. Resolves to null, or to the
  // error of a failed read (the estimate is then forgotten).
  async function refreshEstimate() {
    try {
      store.dispatch(actions.estimate(await api.recordEstimate()));
      return null;
    } catch (error) {
      store.dispatch(actions.estimate(null));
      return error;
    }
  }

  // Lists the takes in the recordings folder. Resolves to null, or to the error of a failed
  // listing (the last list known stays, and the failure is kept to be said).
  async function refreshTakes() {
    try {
      store.dispatch(actions.takes(await api.listTakes()));
      return null;
    } catch (error) {
      store.dispatch(actions.takesFailed(message(error)));
      return error;
    }
  }

  // Starts a take called `name` with the kept settings, its folder named in the window's local
  // time, then reads the status (which shows it recording) and the takes.
  async function startRecording(name) {
    if (isSoundcheckNow(store.getState())) {
      // A take records the live inputs, never a soundcheck's playback; the engine refuses too.
      return recordingSaid("start", false, SOUNDCHECK_BLOCKS_RECORDING);
    }
    const takeName = String(name ?? "").trim();
    let started;
    try {
      started = await api.startRecording(takeName, -new Date().getTimezoneOffset());
    } catch (error) {
      return recordingSaid(
        "start",
        false,
        sentenceWithReason("The recording did not start", message(error)),
      );
    }
    await readStatus();
    await refreshTakes();
    return recordingSaid("start", true, formatStarted(started, takeName || "Take"));
  }

  // Stops the take. While its files are still being finished, asks again `stopRetries` times
  // `stopRetryMs` apart before saying so; the take keeps stopping and Stop can be pressed again.
  async function stopRecording() {
    let answer;
    try {
      answer = await api.stopRecording();
      for (let retry = 0; answer.finishing && retry < stopRetries && !disposed; retry += 1) {
        await wait(stopRetryMs);
        answer = await api.stopRecording();
      }
    } catch (error) {
      await readStatus();
      return recordingSaid(
        "stop",
        false,
        sentenceWithReason("The recording was not stopped", message(error)),
      );
    }
    await readStatus();
    if (answer.finishing) {
      return recordingSaid(
        "stop",
        false,
        "The take's files are still being finished, so it has not stopped yet. Press Stop again " +
          "in a moment.",
      );
    }
    await refreshTakes();
    return recordingSaid("stop", true, formatStopped(answer.take), { take: answer.take });
  }

  // Marks the take being recorded with `name` (blank: the next "Marker n").
  async function addMarker(name) {
    let marker;
    try {
      marker = await api.addMarker(String(name ?? ""));
    } catch (error) {
      return recordingSaid(
        "marker",
        false,
        sentenceWithReason("The marker was not added", message(error)),
      );
    }
    await readStatus();
    return recordingSaid(
      "marker",
      true,
      `Marked "${marker.name}" at ${formatDuration(marker.seconds)}.`,
      { marker },
    );
  }

  // Opens a take folder (as a take lists it) in the system's file manager.
  async function openTakeFolder(folder) {
    try {
      await api.openTakeFolder(folder);
    } catch (error) {
      return recordingSaid(
        "open",
        false,
        sentenceWithReason("The take folder was not opened", message(error)),
      );
    }
    return recordingSaid("open", true, `Opened ${folder}.`);
  }

  // Runs the soundcheck command `action` ("start", "pause", "resume", "seek", "loop", "stop"):
  // `send` asks the desk; its answer, the soundcheck now, shows at once on the run's status
  // (`readAfter` reads the whole status after it too), and the outcome is said as the store's
  // recording outcome ("soundcheck"). Answers `{ ok, message, soundcheck }`.
  async function soundcheckCommand(action, send, { readAfter = false } = {}) {
    const shown = store.getState().engine.run;
    let answer;
    try {
      answer = await send();
    } catch (error) {
      return recordingSaid(
        "soundcheck",
        false,
        sentenceWithReason(SOUNDCHECK_REFUSED[action], message(error)),
      );
    }
    store.dispatch(actions.soundcheck(answer, shown));
    if (readAfter) {
      await readStatus();
    }
    return recordingSaid("soundcheck", true, formatSoundcheckDone(action, answer), {
      soundcheck: answer,
    });
  }

  // Plays the take in `folder` in place of the live inputs; never while a take records.
  async function startSoundcheck(folder, { startFrame = 0, looping = false } = {}) {
    if (isRecordingNow(store.getState())) {
      return recordingSaid("soundcheck", false, RECORDING_BLOCKS_SOUNDCHECK);
    }
    return soundcheckCommand("start", () => api.startSoundcheck(folder, startFrame, looping), {
      readAfter: true,
    });
  }

  // Seeks go out one at a time and the last one asked wins: a slider dragged across the take sends
  // the first move and then only the latest, never every step.
  let seekWanted = null;
  let seeking = null;
  function seekSoundcheck(frame) {
    seekWanted = frame;
    seeking ??= (async () => {
      let result = null;
      try {
        while (seekWanted !== null) {
          const next = seekWanted;
          seekWanted = null;
          result = await soundcheckCommand("seek", () => api.seekSoundcheck(next));
        }
      } finally {
        seeking = null;
      }
      return result;
    })();
    return seeking;
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
    /** Lists the outputs the running engine offers for the local mix; see loadOutputs. */
    loadOutputs,
    /**
     * Plays the local mix on `{ device, firstChannel }`, or nowhere with null, one choice at a
     * time; answers `{ ok, message, superseded }`.
     */
    setLocalOutput,
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
    /** Reads the saved sessions again; resolves to null, or to the error of a failed read. */
    loadSessions,
    /**
     * Saves the desk's whole session, and every musician's mix while an engine runs, as `name`;
     * answers `{ ok, message }` (formatSessionSaved).
     */
    saveSession: (name) =>
      sessionCommand(
        async () => formatSessionSaved(await api.saveSession(name)),
        "The session was not saved",
      ),
    /** Renames the saved session `id` to `name`; answers `{ ok, message }`. */
    renameSession: (id, name) =>
      sessionCommand(async () => {
        const before = sessionName(id);
        const renamed = await api.renameSession(id, name);
        return `Renamed the saved session "${before}" to "${renamed.name}".`;
      }, "The saved session was not renamed"),
    /** Deletes the saved session `id`; answers `{ ok, message }`. */
    deleteSession: (id) =>
      sessionCommand(async () => {
        const name = sessionName(id);
        await api.deleteSession(id);
        return `Deleted the saved session "${name}".`;
      }, "The saved session was not deleted"),
    /**
     * Loads the saved session `id` in place of the desk's (see loadSession above); answers `{ ok,
     * message }` (formatSessionLoaded).
     */
    loadSession,
    /** How many edits a load would drop now; see loadSession. */
    unsavedEdits,
    /** Reads the backups of the musicians' mixes again; answers `{ ok, message }`. */
    loadMixBackups,
    /**
     * One musician's link history, `musicianKey` naming them as the desk does: the samples
     * numbered after `sinceSeq`, or all of them (see linkHistory in bridge.js). Rejects when no
     * engine runs or the key names nobody.
     */
    linkHistory: (musicianKey, sinceSeq = null) => api.linkHistory(musicianKey, sinceSeq),
    /**
     * Sets the playback buffer of the musician `musicianKey` names: `mode` "auto", or "fixed" with
     * `ms` (see setMusicianBuffer in bridge.js); answers `{ ok, message }` (formatBufferSet).
     */
    setMusicianBuffer,
    /**
     * Turns redundancy on (`on`, the copies `delayBlocks` blocks late, 2 when null) or off for the
     * musician `musicianKey` names (see setMusicianRedundancy in bridge.js); answers `{ ok, message
     * }` (formatRedundancySet).
     */
    setMusicianRedundancy,
    /**
     * Changes the master limiter of the mix `address` (a musician's address, or LOCAL_MIX) to
     * `settings` `{ enabled, ceilingDb, releaseMs }` (see limiter.js): shown at once (mixLimiter)
     * and sent with the latest change winning; answers `{ ok, message }` (formatLimiterSet).
     */
    editMixLimiter,
    editChannelSend,
    editEffect,
    editMixReturn,
    /** Reads every channel's strip again; resolves to null, or to the error of a failed read. */
    loadProcessing,
    /**
     * Edits the strip of `channel` (1-based) to `settings`, the whole strip (see processing.js):
     * shown at once over the kept one (channelStrip in console-state.js) and sent through the
     * channel's own lane, the latest edit winning. Resolves to `{ ok, message }`, the last answer
     * (formatProcessingSet), once the channel has nothing left to send; a refusal is also kept as
     * the store's `processingError`.
     */
    editProcessing,
    /** Takes every part of `channel`'s strip off; answers `{ ok, message }`. */
    resetChannelProcessing: (channel) =>
      processingCommand(
        [channel],
        () => api.resetChannelProcessing(channel),
        `CH ${channel}'s processing was not reset`,
      ),
    /** Copies the strip of `from` onto every channel in `to` (1-based); answers `{ ok, message }`. */
    copyChannelProcessing: (from, to) =>
      processingCommand(
        [from, ...to],
        () => api.copyChannelProcessing(from, to),
        `CH ${from}'s processing was not copied`,
      ),
    /** Reads the strip presets again; resolves to null, or to the error of a failed read. */
    loadStripPresets,
    /** Saves `settings` as the strip preset `name`; answers `{ ok, message }`. */
    saveStripPreset: (name, settings) =>
      stripPresetCommand(
        () => api.saveStripPreset(name, settings),
        "The strip preset was not saved",
      ),
    /** Deletes the strip preset `name`; answers `{ ok, message }`. */
    deleteStripPreset: (name) =>
      stripPresetCommand(() => api.deleteStripPreset(name), "The strip preset was not deleted"),
    /** Puts the strip preset `name` on `channel` (1-based); answers `{ ok, message }`. */
    applyStripPreset: (name, channel) =>
      processingCommand(
        [channel],
        () => api.applyStripPreset(name, channel),
        `"${name}" was not applied to CH ${channel}`,
      ),
    /**
     * Works out the response of the equaliser `eq` for `channel`'s curve (`points` of them, 128
     * when null) and keeps it as the store's `eqCurves[channel]`; answers `{ ok, message, points
     * }`.
     */
    loadEqResponse,
    /**
     * Puts the mix backup `stamp` back on the musicians who are connected now; answers `{ ok,
     * message }` (formatMixRestored).
     */
    restoreMixBackup,
    /** Reads the recording settings again; resolves to null, or to the error of a failed read. */
    loadRecordingSettings,
    /**
     * Changes the recording settings with `change`, `{ folder?, format?, includeLocal?, armed?,
     * arm? }` (the folder null or blank for the default, the inputs 1-based; `arm` `{ input, on,
     * channels }` arms or disarms one input, a pair as one, on the armed inputs of the turn; see
     * settingsRequest), on top of the last change kept; answers `{ ok, message }`.
     */
    setRecordingSettings,
    /** Starts a take called `name` with the kept settings; answers `{ ok, message }`. */
    startRecording,
    /**
     * Stops the take, asking again while its files are being finished; answers `{ ok, message,
     * take }`, `take` the finished take when it stopped.
     */
    stopRecording,
    /** Marks the take being recorded; answers `{ ok, message, marker }`. */
    addMarker,
    /** Lists the takes again; resolves to null, or to the error of a failed listing. */
    refreshTakes,
    /** Reads the disk estimate again; resolves to null, or to the error of a failed read. */
    refreshEstimate,
    /** Opens a take folder in the system's file manager; answers `{ ok, message }`. */
    openTakeFolder,
    /**
     * Plays the take in `folder` (as a take lists it) in place of the live inputs, from
     * `startFrame`, looping when `looping`: the virtual soundcheck, and what a take's Play does.
     * Refused in words while a take records. Answers `{ ok, message, soundcheck }`.
     */
    startSoundcheck,
    /** Pauses the soundcheck's take; answers `{ ok, message, soundcheck }`. */
    pauseSoundcheck: () => soundcheckCommand("pause", () => api.pauseSoundcheck()),
    /** Plays the take on (from its start once it stopped at its end). */
    resumeSoundcheck: () => soundcheckCommand("resume", () => api.resumeSoundcheck()),
    /** Moves the take to `frame`; quick moves send only the latest after the first. */
    seekSoundcheck,
    /** Whether the take starts again at its end. */
    setSoundcheckLoop: (looping) =>
      soundcheckCommand("loop", () => api.setSoundcheckLoop(Boolean(looping))),
    /** Stops the soundcheck: back to the live inputs at once. */
    stopSoundcheck: () =>
      soundcheckCommand("stop", () => api.stopSoundcheck(), { readAfter: true }),
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
