// The behaviour <ml-fader> and <ml-pan> share: a 0-100 value operated by pointer and keyboard.
//
// The host is the ARIA slider: it is focusable, carries the value, and takes the keys (arrows one
// step, Page Up and Page Down ten, Home and End either end). A pointer drag on the thumb keeps the
// grab point; a press on the track jumps there. A double-click resets to the `default` attribute.
//
// Events (bubbling, composed): `input` { value } on every change the user makes, `change`
// { value } when a gesture ends (pointer released, each key press, a reset). Setting `value`
// never fires them. While a drag is in progress the user wins: a value set from outside is held
// and applied when the drag ends, so the thumb never jumps under the pointer.
//
// A subclass sets `static range` (see values.js) and `static orientation`, and implements
// `pointerFractionAt(event)`, `isThumb(node)`, `valueText(value)` and `render(value)`; it may
// override `snap(value)`. Host attributes are set on connection, never in a constructor.

import { clampLevel, keyValue, levelFraction, levelFromFraction } from "../lib/values.js";
import { emit, numberAttribute, syncLabel, upgradeProperty } from "./support.js";

export class SliderElement extends HTMLElement {
  static observedAttributes = ["value", "default", "label", "disabled"];

  #value = null;
  #drag = null;
  #held = null;

  constructor() {
    super();
    this.addEventListener("pointerdown", this.#onPointerDown);
    this.addEventListener("pointermove", this.#onPointerMove);
    this.addEventListener("pointerup", this.#endDrag);
    this.addEventListener("pointercancel", this.#endDrag);
    this.addEventListener("lostpointercapture", this.#endDrag);
    this.addEventListener("keydown", this.#onKeyDown);
    this.addEventListener("dblclick", this.#onDoubleClick);
  }

  connectedCallback() {
    upgradeProperty(this, "value");
    const { range, orientation } = this.constructor;
    this.setAttribute("role", "slider");
    this.setAttribute("aria-orientation", orientation);
    this.setAttribute("aria-valuemin", String(range.min));
    this.setAttribute("aria-valuemax", String(range.max));
    this.#syncDisabled();
    syncLabel(this);
    this.#show(this.value);
  }

  attributeChangedCallback(name) {
    if (name === "value") {
      this.value = numberAttribute(this, "value", this.defaultValue);
    } else if (name === "disabled") {
      this.#syncDisabled();
    } else if (name === "label") {
      syncLabel(this);
    }
  }

  /** The value a double-click resets to: the `default` attribute, else the range's reset. */
  get defaultValue() {
    return clampLevel(numberAttribute(this, "default", this.constructor.range.reset));
  }

  get value() {
    return this.#value ?? clampLevel(numberAttribute(this, "value", this.defaultValue));
  }

  set value(value) {
    const next = clampLevel(value);
    if (this.#drag) {
      this.#held = next;
      return;
    }
    this.#show(next);
  }

  get disabled() {
    return this.hasAttribute("disabled");
  }

  set disabled(disabled) {
    this.toggleAttribute("disabled", Boolean(disabled));
  }

  /** A dragged value can land on a detent; plain sliders keep it. */
  snap(value) {
    return value;
  }

  #show(value) {
    this.#value = value;
    this.setAttribute("aria-valuenow", String(value));
    this.setAttribute("aria-valuetext", this.valueText(value));
    this.render(value);
  }

  #syncDisabled() {
    this.tabIndex = this.disabled ? -1 : 0;
    this.setAttribute("aria-disabled", String(this.disabled));
  }

  #change(value, { commit }) {
    if (value !== this.value) {
      this.#show(value);
      emit(this, "input", { value });
    }
    if (commit) {
      emit(this, "change", { value: this.value });
    }
  }

  #onPointerDown = (event) => {
    if (this.disabled || event.button !== 0) {
      return;
    }
    event.preventDefault();
    this.focus();
    this.setPointerCapture(event.pointerId);
    const pointer = this.pointerFractionAt(event);
    const onThumb = event.composedPath().some((node) => this.isThumb(node));
    this.#drag = {
      start: this.value,
      offset: onThumb ? levelFraction(this.value) - pointer : 0,
    };
    this.#held = null;
    this.#change(this.snap(levelFromFraction(pointer + this.#drag.offset)), { commit: false });
  };

  #onPointerMove = (event) => {
    if (!this.#drag) {
      return;
    }
    const fraction = this.pointerFractionAt(event) + this.#drag.offset;
    this.#change(this.snap(levelFromFraction(fraction)), { commit: false });
  };

  #endDrag = () => {
    if (!this.#drag) {
      return;
    }
    const { start } = this.#drag;
    this.#drag = null;
    if (this.value !== start) {
      emit(this, "change", { value: this.value });
    }
    if (this.#held !== null) {
      const held = this.#held;
      this.#held = null;
      this.#show(held);
    }
  };

  #onKeyDown = (event) => {
    if (this.disabled || event.altKey || event.ctrlKey || event.metaKey) {
      return;
    }
    const next = keyValue(event.key, this.value, this.constructor.range);
    if (next === null) {
      return;
    }
    event.preventDefault();
    this.#change(next, { commit: next !== this.value });
  };

  #onDoubleClick = (event) => {
    if (this.disabled) {
      return;
    }
    event.preventDefault();
    const reset = this.defaultValue;
    this.#change(reset, { commit: reset !== this.value });
  };
}
