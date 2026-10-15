import test from "node:test";
import assert from "node:assert/strict";

import { createStore } from "../app/lib/store.js";

function counter(state, action) {
  return action.type === "add" ? { count: state.count + action.by } : state;
}

test("a store starts from its initial state", () => {
  const store = createStore(counter, { count: 1 });

  assert.deepEqual(store.getState(), { count: 1 });
});

test("dispatch runs the reducer and tells every subscriber the new state and the action", () => {
  const store = createStore(counter, { count: 0 });
  const seen = [];
  store.subscribe((state, action) => seen.push(["first", state.count, action.type]));
  store.subscribe((state, action) => seen.push(["second", state.count, action.type]));

  store.dispatch({ type: "add", by: 2 });

  assert.deepEqual(store.getState(), { count: 2 });
  assert.deepEqual(seen, [
    ["first", 2, "add"],
    ["second", 2, "add"],
  ]);
});

test("an action that leaves the state unchanged notifies nobody", () => {
  const store = createStore(counter, { count: 0 });
  let calls = 0;
  store.subscribe(() => {
    calls += 1;
  });

  store.dispatch({ type: "nothing" });

  assert.equal(calls, 0);
});

test("unsubscribing stops the notifications", () => {
  const store = createStore(counter, { count: 0 });
  let calls = 0;
  const unsubscribe = store.subscribe(() => {
    calls += 1;
  });

  store.dispatch({ type: "add", by: 1 });
  unsubscribe();
  store.dispatch({ type: "add", by: 1 });

  assert.equal(calls, 1);
});

test("a subscriber that unsubscribes while being notified does not skip the others", () => {
  const store = createStore(counter, { count: 0 });
  const seen = [];
  const unsubscribe = store.subscribe(() => {
    seen.push("leaving");
    unsubscribe();
  });
  store.subscribe(() => seen.push("staying"));

  store.dispatch({ type: "add", by: 1 });
  store.dispatch({ type: "add", by: 1 });

  assert.deepEqual(seen, ["leaving", "staying", "staying"]);
});

test("an action dispatched by a subscriber runs after every subscriber saw the current one", () => {
  const store = createStore(counter, { count: 0 });
  const seen = [];
  store.subscribe((state) => {
    seen.push(["first", state.count]);
    if (state.count === 1) {
      store.dispatch({ type: "add", by: 10 });
    }
  });
  store.subscribe((state) => seen.push(["second", state.count]));

  store.dispatch({ type: "add", by: 1 });

  assert.deepEqual(seen, [
    ["first", 1],
    ["second", 1],
    ["first", 11],
    ["second", 11],
  ]);
});

test("a reducer that throws leaves the state as it was and the store usable", () => {
  const store = createStore((state, action) => {
    if (action.type === "boom") {
      throw new Error("boom");
    }
    return counter(state, action);
  }, { count: 3 });

  assert.throws(() => store.dispatch({ type: "boom" }), /boom/);
  store.dispatch({ type: "add", by: 1 });

  assert.deepEqual(store.getState(), { count: 4 });
});
