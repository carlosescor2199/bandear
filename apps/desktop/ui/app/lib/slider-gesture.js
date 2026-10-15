// The gestures of a 0-100 slider (<ml-fader>, <ml-pan>), kept apart from the DOM so they can be
// tested. The element turns pointer and keyboard events into calls here and dispatches what they
// return: a list of `{ type: "input" | "change", value }`.
//
// - A press on the track jumps there; a press on the thumb keeps the grab point, so the value
//   never jumps under the pointer. Every move that changes the value is an `input`; the release
//   is one `change` when the value ended somewhere other than where the drag began.
// - A key step (see keyValue in values.js) and a reset are an `input` and a `change` at once.
// - While a drag is in progress the pointer wins: a value set from outside waits, and shows when
//   the drag ends, before its `change` goes out, so whoever answers that `change` has the last
//   word.

import { clampLevel, keyValue, levelFraction, levelFromFraction } from "./values.js";

export class SliderGesture {
  #range;
  #snap;
  #value;
  #drag = null;
  #held = null;

  /** `range` as in values.js (FADER, PAN); `snap` adjusts a dragged value, as a detent does. */
  constructor({ range, value = range.reset, snap = (dragged) => dragged }) {
    this.#range = range;
    this.#snap = snap;
    this.#value = this.#clamp(value);
  }

  /** The value to show. */
  get value() {
    return this.#value;
  }

  get dragging() {
    return this.#drag !== null;
  }

  /** A value from outside. True when the value to show changed; during a drag it waits. */
  set(value) {
    const next = this.#clamp(value);
    if (this.#drag) {
      this.#held = next;
      return false;
    }
    if (next === this.#value) {
      return false;
    }
    this.#value = next;
    return true;
  }

  /** A press at `fraction` (0-1) along the track, on the thumb or not. */
  press(fraction, { onThumb = false } = {}) {
    this.#drag = {
      start: this.#value,
      offset: onThumb ? levelFraction(this.#value) - fraction : 0,
    };
    this.#held = null;
    return this.#dragTo(fraction);
  }

  /** The pointer moved to `fraction` along the track. */
  move(fraction) {
    return this.#drag ? this.#dragTo(fraction) : [];
  }

  /** The pointer was released, or the gesture was cancelled. */
  release() {
    if (!this.#drag) {
      return [];
    }
    const { start } = this.#drag;
    const dragged = this.#value;
    this.#drag = null;
    if (this.#held !== null) {
      this.#value = this.#held;
      this.#held = null;
    }
    return dragged === start ? [] : [{ type: "change", value: dragged }];
  }

  /** A key press; `null` for a key the slider does not use. */
  key(key) {
    const next = keyValue(key, this.#value, this.#range);
    return next === null ? null : this.#commit(next);
  }

  /** Back to `value`, as a double-click does. */
  reset(value) {
    return this.#commit(this.#clamp(value));
  }

  #clamp(value) {
    return clampLevel(value, this.#range.min, this.#range.max);
  }

  #dragTo(fraction) {
    const next = this.#snap(levelFromFraction(fraction + this.#drag.offset));
    if (next === this.#value) {
      return [];
    }
    this.#value = next;
    return [{ type: "input", value: next }];
  }

  #commit(next) {
    if (next === this.#value) {
      return [];
    }
    this.#value = next;
    return [
      { type: "input", value: next },
      { type: "change", value: next },
    ];
  }
}
