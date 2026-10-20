// The channel processing editor's wiring: which strip is open, the changes it sends through the
// console app (console-app.js), A/B, the equaliser curve asked of the desk once the edits pause,
// presets, copies and the last outcome in words. No DOM: the screen (screens/processing-screen.js)
// draws `view()` and hands the engineer's gestures to these commands; the tests drive it without a
// browser.
//
// A/B: "Bypass" keeps the open strip here and sends a neutral one, so the engineer hears the
// channel untouched; turning it off sends the kept strip back. While bypassed the strip shown is
// the kept one and nothing else is sent for it. A strip the desk shows working meanwhile, with no
// edit of the editor's on its way, was changed elsewhere (a session load, a preset in another
// window): its bypass ends and the kept strip is dropped. Leaving the editor ends every bypass.

import { channelStrip } from "./console-state.js";
import { clampStrip } from "./processing.js";
import {
  abBypass,
  abRestore,
  abSettle,
  curveSource,
  presetOptions,
  stepChoice,
  stripChoices,
  withSetting,
} from "./processing-view.js";

/** How long the edits must pause before the desk is asked for the equaliser's curve. */
export const CURVE_DEBOUNCE_MS = 120;

// The channel the editor had open last, so opening it again from the sidebar comes back to it.
let lastOpened = null;

/**
 * The editor's commands for the console `app`, opened on `channel` (1-based; null: the one open
 * last, else the first). `changed()` is called when the view changes outside the store (an
 * outcome, the desk's curve, a bypass). `timers` stands in for setTimeout and clearTimeout.
 */
export function processingEditor({
  app,
  channel = null,
  changed = () => {},
  timers = globalThis,
  debounceMs = CURVE_DEBOUNCE_MS,
}) {
  let open = channel ?? lastOpened ?? 1;
  let ab = {};
  let outcome = null;
  let curveTimer = 0;
  let curveAsk = 0;
  // The equaliser the store's curve of `curveFor.channel` was worked out for.
  let curveFor = { channel: null, eq: null };

  const state = () => app.store.getState();
  const choices = () => stripChoices(state().engine.status);
  const choiceOf = (number) => choices().find((choice) => choice.sides.includes(number)) ?? null;

  function settle() {
    for (const key of Object.keys(ab)) {
      const settled = abSettle(ab, Number(key), channelStrip(state(), Number(key)));
      if (settled !== ab) {
        ab = settled;
        changed();
      }
    }
  }
  const unsubscribe = app.store.subscribe(settle);

  function normalise() {
    const choice = choiceOf(open) ?? choices()[0] ?? null;
    if (choice) {
      open = choice.channel;
      lastOpened = open;
    }
  }
  normalise();

  const bypassed = () => open in ab;
  const shown = () => (bypassed() ? ab[open] : channelStrip(state(), open).settings);
  const bypassRefusal = () => ({
    ok: false,
    message: `CH ${open} is bypassed: turn the bypass off to change its processing.`,
  });

  function say(result) {
    outcome = result;
    changed();
    return result;
  }

  // Runs a change of the open strip unless it is bypassed, and keeps what it said.
  async function change(run) {
    if (bypassed()) {
      return say(bypassRefusal());
    }
    return say(await run(open));
  }

  // An edit of the whole strip: only a refusal is said, so a drag does not announce every step.
  async function send(settings) {
    if (bypassed()) {
      return say(bypassRefusal());
    }
    const result = await app.editProcessing(open, clampStrip(settings));
    return result.ok ? result : say(result);
  }

  return {
    view() {
      normalise();
      settle();
      const current = state();
      const strip = channelStrip(current, open);
      const settings = shown();
      const choice = choiceOf(open);
      const error = current.processingError;
      const asked = curveFor.channel === open ? curveFor.eq : null;
      return {
        known: choice !== null,
        unread: current.processing === null,
        choices: choices(),
        choice,
        channel: open,
        sides: choice?.sides ?? [open],
        settings,
        active: strip.active,
        pending: strip.pending,
        bypassed: bypassed(),
        canBypass: bypassed() || strip.active,
        presets: presetOptions(current.stripPresets),
        curve: curveSource({ asked, eq: settings.eq, desk: current.eqCurves[open] ?? null }),
        outcome,
        error:
          error && (error.channel === null || choice?.sides.includes(error.channel))
            ? error.message
            : null,
      };
    },
    /** Opens the strip holding `number`; answers the channel open (unchanged when it has none). */
    open(number) {
      if (choiceOf(number)) {
        open = choiceOf(number).channel;
        lastOpened = open;
        outcome = null;
      }
      return open;
    },
    /** Opens the strip `delta` places away; answers the channel open. */
    step(delta) {
      return this.open(stepChoice(choices(), open, delta));
    },
    /** Sets the value at `path` of the open strip ("gate.enabled", "eq.bands.1.q"). */
    edit(path, value) {
      return send(withSetting(shown(), path, value));
    },
    /** Sets the open strip's equaliser to `eq` (a handle dragged on the graph). */
    editEq(eq) {
      return send({ ...shown(), eq });
    },
    /** A/B: bypasses the open strip, or puts its kept strip back. */
    async toggleBypass() {
      const channelOpen = open;
      if (bypassed()) {
        const restored = abRestore(ab, channelOpen);
        ab = restored.ab;
        changed();
        const result = await app.editProcessing(channelOpen, restored.send);
        return say(result.ok ? { ok: true, message: `CH ${channelOpen}'s processing is back on.` } : result);
      }
      const strip = channelStrip(state(), channelOpen);
      if (!strip.active) {
        return say({ ok: false, message: `CH ${channelOpen} has no processing to bypass.` });
      }
      const bypass = abBypass(ab, channelOpen, strip.settings);
      ab = bypass.ab;
      changed();
      const result = await app.editProcessing(channelOpen, bypass.send);
      if (!result.ok) {
        ab = abRestore(ab, channelOpen).ab;
        return say(result);
      }
      return say({
        ok: true,
        message: `CH ${channelOpen} is bypassed: you hear it with no processing.`,
      });
    },
    /** Takes every part of the open strip off. */
    reset() {
      return change((number) => app.resetChannelProcessing(number));
    },
    /** Copies the open strip onto `targets` (1-based). */
    copyTo(targets) {
      if (targets.length === 0) {
        return Promise.resolve(
          say({ ok: false, message: `Pick the channels to copy CH ${open}'s processing to.` }),
        );
      }
      return change((number) => app.copyChannelProcessing(number, targets));
    },
    /** Puts the strip preset `name` on the open strip. */
    applyPreset(name) {
      return change((number) => app.applyStripPreset(name, number));
    },
    /** Saves the open strip as the strip preset `name`. */
    async savePreset(name) {
      const trimmed = String(name ?? "").trim();
      if (!trimmed) {
        return say({ ok: false, message: "Name the preset to save it." });
      }
      return say(await app.saveStripPreset(trimmed, shown()));
    },
    /** Deletes the engineer's strip preset `name`. */
    async deletePreset(name) {
      return say(await app.deleteStripPreset(name));
    },
    /** Reads every strip again, after a read that failed. */
    readAgain() {
      return app.loadProcessing();
    },
    /** Asks the desk for the open strip's curve once the edits pause for `debounceMs`. */
    requestCurve() {
      timers.clearTimeout(curveTimer);
      curveTimer = timers.setTimeout(() => {
        curveTimer = 0;
        const number = open;
        const eq = structuredClone(shown().eq);
        curveAsk += 1;
        const ask = curveAsk;
        app.loadEqResponse(number, eq).then((answer) => {
          if (answer.ok && ask === curveAsk) {
            curveFor = { channel: number, eq };
            changed();
          }
        });
      }, debounceMs);
    },
    /** Stops listening and ends every bypass; resolves once the kept strips were sent. */
    dispose() {
      unsubscribe();
      timers.clearTimeout(curveTimer);
      const kept = Object.entries(ab);
      ab = {};
      return Promise.all(
        kept.map(([number, strip]) => app.editProcessing(Number(number), strip)),
      );
    },
  };
}
