import test from "node:test";
import assert from "node:assert/strict";

import { createCoalescer } from "../app/lib/coalesce.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

// A send that never settles on its own, so each test decides when an answer arrives.
function heldSend() {
  const calls = [];
  const send = (key, value) => {
    const answer = deferred();
    calls.push({ key, value, ...answer });
    return answer.promise;
  };
  return { calls, send };
}

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

test("the first value for a key is sent at once", () => {
  const { calls, send } = heldSend();
  const coalescer = createCoalescer(send);

  coalescer.push("bass", 1);

  assert.deepEqual(
    calls.map(({ key, value }) => [key, value]),
    [["bass", 1]],
  );
  assert.equal(coalescer.busy("bass"), true);
});

test("values pushed while a send is in flight wait, and only the latest goes out after it", async () => {
  const { calls, send } = heldSend();
  const coalescer = createCoalescer(send);

  coalescer.push("bass", "a");
  coalescer.push("bass", "b");
  coalescer.push("bass", "c");
  assert.equal(calls.length, 1, "one send in flight per key");

  calls[0].resolve();
  await settle();
  assert.deepEqual(
    calls.map(({ value }) => value),
    ["a", "c"],
    "the latest value wins and the one in between is dropped",
  );

  calls[1].resolve();
  await settle();
  assert.equal(calls.length, 2);
  assert.equal(coalescer.busy("bass"), false);
});

test("a failed send still lets the waiting value go out, and reports the failure", async () => {
  const { calls, send } = heldSend();
  const failures = [];
  const coalescer = createCoalescer(send, {
    onError: (error, key, value) => failures.push([error.message, key, value]),
  });

  coalescer.push("bass", "a");
  coalescer.push("bass", "b");
  calls[0].reject(new Error("refused"));
  await settle();

  assert.deepEqual(failures, [["refused", "bass", "a"]]);
  assert.deepEqual(
    calls.map(({ value }) => value),
    ["a", "b"],
    "the trailing send is guaranteed",
  );
});

test("a send that throws at once counts as finished", async () => {
  let attempts = 0;
  const coalescer = createCoalescer(() => {
    attempts += 1;
    throw new Error("no bridge");
  });

  coalescer.push("bass", 1);
  await settle();
  coalescer.push("bass", 2);
  await settle();

  assert.equal(attempts, 2);
  assert.equal(coalescer.busy("bass"), false);
});

test("each key has its own lane", () => {
  const { calls, send } = heldSend();
  const coalescer = createCoalescer(send);

  coalescer.push("bass", 1);
  coalescer.push("drums", 2);

  assert.deepEqual(
    calls.map(({ key }) => key),
    ["bass", "drums"],
  );
});

test("idle resolves once nothing is in flight or waiting for that key", async () => {
  const { calls, send } = heldSend();
  const coalescer = createCoalescer(send);
  let idle = false;

  await coalescer.idle("bass");
  coalescer.push("bass", 1);
  coalescer.push("bass", 2);
  coalescer.idle("bass").then(() => {
    idle = true;
  });

  calls[0].resolve();
  await settle();
  assert.equal(idle, false, "the trailing send is still going");

  calls[1].resolve();
  await settle();
  assert.equal(idle, true);
});
