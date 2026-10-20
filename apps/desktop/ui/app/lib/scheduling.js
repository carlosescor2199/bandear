// Small timing helpers the screens and components share, with the browser's clocks injectable so
// node tests can drive them.
//
// frameScheduler: draws at most once per animation frame, and never after it was disposed, so a
//   promise that settles after a screen was left cannot draw on (or change the address for) a
//   screen that is gone.
// trailingThrottle: runs at most once per interval and always once more after the last push, so
//   the final value is the one that is kept (a spoken meter value once the meters stop).

/**
 * `draw` at most once per frame. `schedule()` asks for the next frame, `now()` draws at once,
 * `dispose()` cancels the frame asked for and turns both into no-ops; `disposed` says whether it was.
 */
export function frameScheduler(
  draw,
  {
    request = (run) => globalThis.requestAnimationFrame(run),
    cancel = (id) => globalThis.cancelAnimationFrame(id),
  } = {},
) {
  let frame = 0;
  let disposed = false;
  return {
    schedule() {
      if (disposed || frame !== 0) {
        return;
      }
      frame = request(() => {
        frame = 0;
        if (!disposed) {
          draw();
        }
      });
    },
    now() {
      if (!disposed) {
        draw();
      }
    },
    dispose() {
      disposed = true;
      if (frame !== 0) {
        cancel(frame);
      }
      frame = 0;
    },
    get disposed() {
      return disposed;
    },
  };
}

/**
 * Runs `run` at most once per `intervalMs`: a `push()` inside the interval waits for its end, and
 * pushes meanwhile are folded into that one trailing run. `cancel()` drops a run waiting. `now`
 * gives the time in ms; `timers` stands in for setTimeout and clearTimeout.
 */
export function trailingThrottle(
  run,
  { intervalMs, now = () => globalThis.performance.now(), timers = globalThis },
) {
  let lastRun = -Infinity;
  let timer = 0;
  function fire() {
    timer = 0;
    lastRun = now();
    run();
  }
  return {
    push() {
      if (timer !== 0) {
        return;
      }
      const wait = lastRun + intervalMs - now();
      if (wait <= 0) {
        fire();
      } else {
        timer = timers.setTimeout(fire, wait);
      }
    },
    cancel() {
      if (timer !== 0) {
        timers.clearTimeout(timer);
      }
      timer = 0;
    },
  };
}
