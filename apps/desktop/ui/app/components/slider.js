// The behaviour <ml-fader> and <ml-pan> share: a 0-100 value operated by pointer and keyboard.
//
// The host is the ARIA slider: it is focusable, carries the value, and takes the keys (arrows one
// step, Page Up and Page Down ten, Home and End either end). A pointer drag on the thumb keeps the
// grab point; a press on the track jumps there. A double-click resets to the `default` attribute.
// The gestures themselves live in lib/slider-gesture.js, which is tested; this class only turns
// DOM events into gesture calls, renders the value and dispatches what the gesture returns.
//
// Events (bubbling, composed): `input` { value } on every change the user makes, `change`
// { value } when a gesture ends (pointer released, each key press, a reset). Setting `value`
// never fires them. While a drag is in progress a value set from outside waits until it ends.
//
// A subclass sets `static range` (see values.js) and `static orientation`, and implements
// `pointerFractionAt(event)`, `isThumb(node)`, `valueText(value)` and `render(value)`; it may
// override `snap(value)`. Host attributes are set on connection, never in a constructor.

import { SliderGesture } from "../lib/slider-gesture.js";
import { clampLevel } from "../lib/values.js";
import { emit, numberAttribute, syncLabel, upgradeProperty } from "./support.js";

export class SliderElement extends HTMLElement {
  static observedAttributes = ["value", "default", "label", "disabled"];

  #gesture;

  constructor() {
    super();
    const { range } = this.constructor;
    this.#gesture = new SliderGesture({ range, snap: (value) => this.snap(value) });
    this.addEventListener("pointerdown", this.#onPointerDown);
    this.addEventListener("pointermove", this.#onPointerMove);
    this.addEventListener("pointerup", this.#onPointerUp);
    this.addEventListener("pointercancel", this.#onPointerUp);
    this.addEventListener("lostpointercapture", this.#onPointerUp);
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
    this.#show();
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
    return this.#gesture.value;
  }

  set value(value) {
    if (this.#gesture.set(value)) {
      this.#show();
    }
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

  #show() {
    const { value } = this.#gesture;
    this.setAttribute("aria-valuenow", String(value));
    this.setAttribute("aria-valuetext", this.valueText(value));
    this.render(value);
  }

  #syncDisabled() {
    this.tabIndex = this.disabled ? -1 : 0;
    this.setAttribute("aria-disabled", String(this.disabled));
  }

  // Renders the value the gesture shows, then dispatches its events, so a listener that sets the
  // value in answer has the last word.
  #apply(events) {
    if (events === null) {
      return false;
    }
    this.#show();
    for (const { type, value } of events) {
      emit(this, type, { value });
    }
    return true;
  }

  #onPointerDown = (event) => {
    if (this.disabled || event.button !== 0) {
      return;
    }
    event.preventDefault();
    this.focus();
    this.setPointerCapture(event.pointerId);
    const onThumb = event.composedPath().some((node) => this.isThumb(node));
    this.#apply(this.#gesture.press(this.pointerFractionAt(event), { onThumb }));
  };

  #onPointerMove = (event) => {
    if (this.#gesture.dragging) {
      this.#apply(this.#gesture.move(this.pointerFractionAt(event)));
    }
  };

  #onPointerUp = () => {
    if (this.#gesture.dragging) {
      this.#apply(this.#gesture.release());
    }
  };

  #onKeyDown = (event) => {
    if (this.disabled || event.altKey || event.ctrlKey || event.metaKey) {
      return;
    }
    if (this.#apply(this.#gesture.key(event.key))) {
      event.preventDefault();
    }
  };

  #onDoubleClick = (event) => {
    if (this.disabled) {
      return;
    }
    event.preventDefault();
    this.#apply(this.#gesture.reset(this.defaultValue));
  };
}
