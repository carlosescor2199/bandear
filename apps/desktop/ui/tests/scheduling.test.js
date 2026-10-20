// The shared timing helpers (scheduling.js): a frame scheduler that stops for good once disposed,
// and a throttle that always runs once more after the last push.
import test from "node:test";
import assert from "node:assert/strict";

import { frameScheduler, trailingThrottle } from "../app/lib/scheduling.js";

// Animation frames the test runs by hand.
function manualFrames() {
  const waiting = new Map();
  let next = 1;
  return {
    request(run) {
      waiting.set(next, run);
      return next++;
    },
    cancel(id) {
      waiting.delete(id);
    },
    pending: () => waiting.size,
    runAll() {
      const runs = [...waiting.values()];
      waiting.clear();
      runs.forEach((run) => run());
    },
  };
}

test("a frame scheduler draws once per frame however often it is asked", () => {
  const frames = manualFrames();
  let draws = 0;
  const scheduler = frameScheduler(() => (draws += 1), frames);
  scheduler.schedule();
  scheduler.schedule();
  assert.equal(frames.pending(), 1);
  frames.runAll();
  assert.equal(draws, 1);
  scheduler.schedule();
  frames.runAll();
  assert.equal(draws, 2, "a new frame can be asked for once the last one drew");
  scheduler.now();
  assert.equal(draws, 3);
});

test("a disposed frame scheduler never draws again, not even for a late promise", async () => {
  const frames = manualFrames();
  let draws = 0;
  const scheduler = frameScheduler(() => (draws += 1), frames);
  scheduler.schedule();
  scheduler.dispose();
  assert.equal(frames.pending(), 0, "the frame asked for is cancelled");
  assert.equal(scheduler.disposed, true);

  // An answer that arrives after the screen was left.
  await Promise.resolve().then(() => scheduler.schedule());
  scheduler.now();
  assert.equal(frames.pending(), 0, "no frame is asked for after dispose");
  frames.runAll();
  assert.equal(draws, 0);
});

test("a frame already running when the scheduler is disposed does not draw", () => {
  const frames = manualFrames();
  let draws = 0;
  // A frame whose cancel comes too late (the browser already queued it).
  const scheduler = frameScheduler(() => (draws += 1), { request: frames.request, cancel: () => {} });
  scheduler.schedule();
  scheduler.dispose();
  frames.runAll();
  assert.equal(draws, 0);
});

test("a trailing throttle runs at once, then once more at the end of the interval with the latest value", () => {
  let clock = 0;
  const waiting = new Map();
  let next = 1;
  const timers = {
    setTimeout(run, ms) {
      waiting.set(next, { run, at: clock + ms });
      return next++;
    },
    clearTimeout(id) {
      waiting.delete(id);
    },
  };
  const advance = (ms) => {
    clock += ms;
    for (const [id, { run, at }] of [...waiting]) {
      if (at <= clock) {
        waiting.delete(id);
        run();
      }
    }
  };
  let value = 0;
  const spoken = [];
  const throttle = trailingThrottle(() => spoken.push(value), {
    intervalMs: 500,
    now: () => clock,
    timers,
  });

  value = 1;
  throttle.push();
  assert.deepEqual(spoken, [1], "the first push runs at once");
  advance(100);
  value = 2;
  throttle.push();
  advance(100);
  value = 3;
  throttle.push();
  assert.deepEqual(spoken, [1], "pushes inside the interval wait");
  advance(300);
  assert.deepEqual(spoken, [1, 3], "the last value is run when the interval ends");
  advance(2000);
  assert.deepEqual(spoken, [1, 3], "nothing more once the pushes stop");

  value = 4;
  throttle.push();
  assert.deepEqual(spoken, [1, 3, 4], "after a quiet interval a push runs at once");
  advance(10);
  value = 5;
  throttle.push();
  throttle.cancel();
  advance(1000);
  assert.deepEqual(spoken, [1, 3, 4], "a cancelled run never comes");
});
