// The console's controller: the one place that talks to the engine through a bridge and keeps
// the store. It lives as long as the window; screens come and go and use it. No DOM.
//
// - On creation it listens to the engine's events, then asks for the engine's state. When the
//   answer is that no engine runs (`state: "notRunning"`, an answer and not an error) it starts
//   one with the start request the old window sent (see engine-config.js), unless `autoStart` is
//   false. A failed read, and a failed listen, is the console's connection error; `reconnect`
//   tries again, one attempt at a time.
// - `start` and `stop` drive the engine and its phase. A start, a failed start and a confirmed
//   stop end the current run, so late events of an ended engine are ignored (see
//   console-state.js). A refused stop or an unreadable status after a start ends nothing: the
//   engine may still run, so its run stays live, a refused stop is shown, and the status is read
//   again.
// - `edit` records local mix edits and sends them with `set_mix` through a per-musician
//   coalescer: one request in flight per musician, the latest edits always sent, each answer
//   settling the edits it carried and offering its mix by revision.
// - Meters skip the store: `onMeters` listeners get each `meters` payload of the live run.

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
import { startRequest } from "./engine-config.js";
import { buildMixRequest } from "./mix.js";
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

  // Sends `address`'s pending edits as they are when the send goes out. The coalescer calls it
  // at most once at a time per musician, and again after an answer when edits are still waiting.
  async function sendMix(address) {
    const state = store.getState();
    const musician = musicianByAddress(state, address);
    const sent = { ...pendingEdits(state, address) };
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

  const sender = createCoalescer(sendMix);

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

  async function start() {
    store.dispatch(actions.engine("starting"));
    try {
      await api.start(startRequest(store.getState().audio));
    } catch (error) {
      // A failed start has stopped the engine it replaced and started none.
      store.dispatch(actions.engine("error", message(error)));
      return;
    }
    // The engine runs now: failing to read its status must not end its run.
    const error = await readStatus();
    if (error) {
      fillAfterStart(error);
    }
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
    let answer;
    try {
      answer = await api.status();
    } catch (error) {
      store.dispatch(actions.connectionFailed(message(error)));
      return;
    }
    store.dispatch(actions.connected());
    if (answer.state === "notRunning" && autoStart) {
      await start();
    } else {
      takeEngineState(answer);
    }
  }

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
    start,
    stop,
    select(address) {
      store.dispatch(actions.select(address));
    },
    /** Local edits to `address`'s mix, keyed as editKey (mix.js) makes them. */
    edit(address, edits) {
      const before = store.getState();
      if (store.dispatch(actions.mixEdit(address, edits)) !== before) {
        sender.push(address, null);
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
