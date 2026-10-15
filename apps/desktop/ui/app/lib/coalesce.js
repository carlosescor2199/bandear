// Coalesces high-rate sends, such as a fader dragged across its whole travel.
//
// Each key (a musician's address) has one lane: at most one send in flight. A value pushed while a
// send is in flight waits; a later push replaces it, so the latest value wins and the ones in
// between are never sent. When the send in flight settles, resolved or rejected, the waiting value
// goes out, so the last value pushed is always sent.
//
// `send(key, value)` returns a promise (or throws). It should handle its own errors; `onError`
// only hears about them, and a failure never blocks the lane.

export function createCoalescer(send, { onError } = {}) {
  const lanes = new Map();

  function start(key, lane, value) {
    let sending;
    try {
      sending = Promise.resolve(send(key, value));
    } catch (error) {
      sending = Promise.reject(error);
    }
    sending
      .then(
        () => undefined,
        (error) => onError?.(error, key, value),
      )
      .finally(() => finish(key, lane));
  }

  function finish(key, lane) {
    if (lane.waiting) {
      const { value } = lane;
      lane.waiting = false;
      lane.value = undefined;
      start(key, lane, value);
      return;
    }
    lanes.delete(key);
    for (const resolve of lane.idle.splice(0)) {
      resolve();
    }
  }

  return {
    /** Sends `value` for `key` now, or after the send in flight, replacing any value waiting. */
    push(key, value) {
      const lane = lanes.get(key);
      if (lane) {
        lane.waiting = true;
        lane.value = value;
        return;
      }
      const fresh = { waiting: false, value: undefined, idle: [] };
      lanes.set(key, fresh);
      start(key, fresh, value);
    },
    /** True while `key` has a send in flight or a value waiting. */
    busy(key) {
      return lanes.has(key);
    },
    /** Resolves once `key` has nothing in flight and nothing waiting. */
    idle(key) {
      const lane = lanes.get(key);
      if (!lane) {
        return Promise.resolve();
      }
      return new Promise((resolve) => lane.idle.push(resolve));
    },
  };
}
