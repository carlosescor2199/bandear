// A tiny state container: one state value, a pure reducer and plain subscribers. No framework.
//
// `dispatch` runs the reducer; when the state changes (a new object), every subscriber is called
// with the new state and the action. An action dispatched by a subscriber waits until every
// subscriber has seen the current one, so all of them see the states in the same order.

export function createStore(reducer, initialState) {
  let state = initialState;
  let dispatching = false;
  const queue = [];
  const listeners = new Set();

  function dispatch(action) {
    queue.push(action);
    if (dispatching) {
      return state;
    }
    dispatching = true;
    try {
      while (queue.length > 0) {
        const next = queue.shift();
        const after = reducer(state, next);
        if (after === state) {
          continue;
        }
        state = after;
        // A copy, so a subscriber that unsubscribes while notified does not skip the next one.
        for (const listener of [...listeners]) {
          listener(state, next);
        }
      }
    } finally {
      // A reducer or subscriber that threw drops the actions queued behind it, so they cannot run
      // later against a state they were not dispatched for.
      queue.length = 0;
      dispatching = false;
    }
    return state;
  }

  function subscribe(listener) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }

  return {
    getState: () => state,
    dispatch,
    subscribe,
  };
}
