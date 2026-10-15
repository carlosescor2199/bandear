// The console's controller: the one place that talks to the engine through a bridge and keeps
// the store. It lives as long as the window; screens come and go and use it. No DOM.
//
// - On creation it listens to the engine's events, then asks for the engine's status. When no
//   engine runs it starts one with the start request the old window sent (see engine-config.js),
//   unless `autoStart` is false.
// - `start` and `stop` drive the engine and its phase; each start or stop ends the current run, so
//   late events of an ended engine are ignored (see console-state.js).
// - `edit` records local mix edits and sends them with `set_mix` through a per-musician
//   coalescer: one request in flight per musician, the latest edits always sent, each answer
//   settling the edits it carried.
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
      await api.setMix(address, buildMixRequest(musician.mix, sent));
      store.dispatch(actions.mixApplied(address, sent));
    } catch (error) {
      store.dispatch(actions.mixFailed(address, sent, error));
    }
  }

  const sender = createCoalescer(sendMix);

  async function start() {
    store.dispatch(actions.engine("starting"));
    try {
      await api.start(startRequest(store.getState().audio));
      // The pump's first status is usually here already; this covers a window without one.
      store.dispatch(actions.status(await api.status()));
    } catch (error) {
      store.dispatch(actions.engine("error", message(error)));
    }
  }

  async function stop() {
    store.dispatch(actions.engine("stopping"));
    try {
      await api.stop();
      store.dispatch(actions.engine("idle"));
    } catch (error) {
      store.dispatch(actions.engine("error", message(error)));
    }
  }

  // An event that arrives after `dispose` (a listener still being registered) is dropped.
  function heard(handler) {
    return (payload) => {
      if (!disposed) {
        handler(payload);
      }
    };
  }

  async function connect() {
    const subscriptions = await Promise.all([
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
    unlisteners.push(...subscriptions);
    if (disposed) {
      return;
    }
    let status;
    try {
      status = await api.status();
    } catch {
      // No engine runs yet: the engine_status call is the one that fails.
      if (autoStart) {
        await start();
      } else {
        store.dispatch(actions.engine("idle"));
      }
      return;
    }
    store.dispatch(actions.status(status));
  }

  const ready = connect();

  return {
    store,
    /** Resolves once the first status is in, or the first start has been tried. */
    ready,
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
