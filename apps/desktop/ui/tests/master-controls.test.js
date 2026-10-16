// The master strip's controls joined to the mix, with stand-in faders that behave as <ml-fader>
// does: the same slider gesture (slider-gesture.js) and the same `input` and `change` events, so
// the wiring the strip uses (master-controls.js) is tested without a browser.
import test from "node:test";
import assert from "node:assert/strict";

import { showMasterValues, wireMasterControls } from "../app/lib/master-controls.js";
import { SliderGesture } from "../app/lib/slider-gesture.js";
import { FADER } from "../app/lib/values.js";

/** A fader as <ml-fader> is one: its value is the gesture's, and the gesture's events go out. */
class StandInFader extends EventTarget {
  #gesture = new SliderGesture({ range: FADER });

  get value() {
    return this.#gesture.value;
  }

  set value(value) {
    this.#gesture.set(value);
  }

  press(fraction) {
    this.#dispatch(this.#gesture.press(fraction));
  }

  move(fraction) {
    this.#dispatch(this.#gesture.move(fraction));
  }

  release() {
    this.#dispatch(this.#gesture.release());
  }

  key(key) {
    this.#dispatch(this.#gesture.key(key) ?? []);
  }

  #dispatch(events) {
    for (const { type, value } of events) {
      this.dispatchEvent(new CustomEvent(type, { detail: { value } }));
    }
  }
}

/** A toggle as <ml-toggle> is one: a press flips it and says so. */
class StandInToggle extends EventTarget {
  pressed = false;

  press() {
    this.pressed = !this.pressed;
    this.dispatchEvent(new CustomEvent("change", { detail: { pressed: this.pressed } }));
  }
}

const DATA = Object.freeze({ volume: 85, cap: 90, limit: 100, muted: false });

function strip() {
  const parts = {
    level: new StandInFader(),
    cap: new StandInFader(),
    limit: new StandInFader(),
    mute: new StandInToggle(),
  };
  const edits = [];
  wireMasterControls(parts, (field, value) => edits.push([field, value]));
  showMasterValues(parts, DATA);
  return { parts, edits };
}

test("a finished gesture on the limit sends one edit of the engineer's limit, never one per move", () => {
  const { parts, edits } = strip();

  parts.limit.press(1);
  parts.limit.move(0.8);
  parts.limit.move(0.6);
  assert.deepEqual(edits, [], "nothing goes out while the limit moves");
  parts.limit.release();

  assert.deepEqual(edits, [["engineerLimitPercent", 60]]);
});

test("a re-render in the middle of a drag of the limit does not move its thumb", () => {
  const { parts, edits } = strip();
  parts.limit.press(1);
  parts.limit.move(0.6);

  // A status arrives mid-drag with the limit the engine still has.
  showMasterValues(parts, DATA);

  assert.equal(parts.limit.value, 60, "the pointer wins while it drags");
  parts.limit.release();
  assert.deepEqual(edits, [["engineerLimitPercent", 60]], "the dragged value is what goes out");
});

test("a key press on the limit is a finished gesture", () => {
  const { parts, edits } = strip();

  parts.limit.key("PageDown");

  assert.deepEqual(edits, [["engineerLimitPercent", 90]]);
});

test("the level and the phone's cap go out as they move, and Mute as it is pressed", () => {
  const { parts, edits } = strip();

  parts.level.press(0.5);
  parts.cap.press(0.3);
  parts.mute.press();

  assert.deepEqual(edits, [
    ["volumePercent", 50],
    ["maxLevelPercent", 30],
    ["muted", true],
  ]);
  assert.deepEqual(
    [parts.level.value, parts.cap.value, parts.limit.value],
    [50, 30, 100],
    "each control shows its own value",
  );
});
