// The console's controller: the one place that talks to the engine through a bridge and keeps
// the store. It lives as long as the window; screens come and go and use it. No DOM.
//
// - On creation it listens to the engine's events, then asks for the engine's status. Only when
//   the engine answers that it is not running does it start one, with the start request the old
//   window sent (see engine-config.js), unless `autoStart` is false. Any other failure, and a
//   failed listen, is the console's connection error; `reconnect` tries again.
// - `start` and `stop` drive the engine and its phase. A start, a failed start and a confirmed
//   stop end the current run, so late events of an ended engine are ignored (see
//   console-state.js). A refused stop or an unreadable status after a start ends nothing: the
//   engine may still run, so its run stays live, a refused stop is shown, and the status is read
//   again.
// - `edit` records local mix edits and sends them with `set_mix` through a per-musician
//   coalescer: one request in flight per musician, the latest edits always sent, each answer
//   settling the edits it carried and offering its mix by revision.
// - Meters skip the store: `onMeters` listeners get each `meters` payload of the live run.

import { createEngineApi, isEngineNotRunning } from "./bridge.js";
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

export function createConsoleApp({ bridge, autoStart = true }) {
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

  // Reads the engine's status into the store. Resolves to false when it could not be read; a
  // "not running" answer then means the engine has stopped, any other failure tells nothing.
  async function readStatus() {
    try {
      store.dispatch(actions.status(await api.status()));
      return true;
    } catch (error) {
      if (isEngineNotRunning(error)) {
        store.dispatch(actions.engine("idle"));
      }
      return false;
    }
  }

  // After a start, the status the pump sends fills the view; when none has come yet, ask again a
  // few times rather than leave the screen on "starting".
  function fillAfterStart(attempt = 1) {
    if (attempt > STATUS_RETRIES) {
      return;
    }
    const timer = setTimeout(async () => {
      if (!disposed && store.getState().engine.phase === "starting" && !(await readStatus())) {
        fillAfterStart(attempt + 1);
      }
    }, STATUS_RETRY_MS);
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
    if (!(await readStatus()) && store.getState().engine.phase === "starting") {
      fillAfterStart();
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

  // Listens to the engine (once) and loads its status. Never rejects: a failure is shown as the
  // console's connection error, and `reconnect` tries again.
  async function connect() {
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
    let status;
    try {
      status = await api.status();
    } catch (error) {
      if (!isEngineNotRunning(error)) {
        store.dispatch(actions.connectionFailed(message(error)));
        return;
      }
      // No engine runs yet, which is the one failure that means exactly that.
      store.dispatch(actions.connected());
      if (autoStart) {
        await start();
      } else {
        store.dispatch(actions.engine("idle"));
      }
      return;
    }
    store.dispatch(actions.connected());
    store.dispatch(actions.status(status));
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
