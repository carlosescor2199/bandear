// <ml-reduction-meter>: how much a gate or compressor takes off, as a bar that fills from the right
// and the amount in words ("−4.0 dB").
//
// Attributes: `label` (accessible name, e.g. "Compressor gain reduction"), `scale` (the dB the full
//   bar stands for, 20 by default).
// Method: `setReduction(db)`, the positive dB taken off from the `meters` event. Call it as often as
//   meters arrive: it draws once in the next animation frame; the spoken value follows at most twice
//   a second, and once more after the last change, so the value it settles on is announced.
// No events.

import { reductionView } from "../lib/processing-view.js";
import { trailingThrottle } from "../lib/scheduling.js";
import { attachShadowRoot, define, numberAttribute, setText, sheet, syncLabel } from "./support.js";

const SPOKEN_INTERVAL_MS = 500;

const styles = sheet(`
  :host {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 64px;
    align-items: center;
    gap: 10px;
  }
  .track {
    position: relative;
    height: 8px;
    overflow: hidden;
    border-radius: 4px;
    background: var(--ml-track);
  }
  .bar {
    position: absolute;
    inset: 0;
    transform-origin: right;
    transform: scaleX(0);
    background: var(--ml-blue);
    will-change: transform;
  }
  .value {
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-sm);
    text-align: right;
    white-space: nowrap;
  }
`);

class MlReductionMeter extends HTMLElement {
  static observedAttributes = ["label", "scale"];

  #parts;
  #db = 0;
  #frame = 0;
  #spoken = trailingThrottle(() => this.#speak(), { intervalMs: SPOKEN_INTERVAL_MS });

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="track"><div class="bar"></div></div><span class="value" aria-hidden="true">0.0 dB</span>`,
    );
    this.#parts = { bar: root.querySelector(".bar"), value: root.querySelector(".value") };
  }

  connectedCallback() {
    this.setAttribute("role", "meter");
    this.setAttribute("aria-valuemin", "0");
    syncLabel(this);
    this.#draw();
  }

  disconnectedCallback() {
    cancelAnimationFrame(this.#frame);
    this.#frame = 0;
    this.#spoken.cancel();
  }

  attributeChangedCallback(name) {
    if (name === "label") {
      syncLabel(this);
    } else {
      this.#draw();
    }
  }

  setReduction(db) {
    this.#db = db;
    if (this.#frame === 0 && this.isConnected) {
      this.#frame = requestAnimationFrame(() => this.#draw());
    }
  }

  #draw() {
    this.#frame = 0;
    const scale = numberAttribute(this, "scale", 20);
    const view = reductionView(this.#db, scale);
    this.#parts.bar.style.transform = `scaleX(${view.fraction})`;
    setText(this.#parts.value, view.text);
    this.#spoken.push();
  }

  // The value said to assistive technology: at most twice a second, and the last one always.
  #speak() {
    const scale = numberAttribute(this, "scale", 20);
    const view = reductionView(this.#db, scale);
    this.setAttribute("aria-valuemax", String(scale));
    this.setAttribute("aria-valuenow", String(Math.min(scale, Math.max(0, this.#db || 0)).toFixed(1)));
    this.setAttribute("aria-valuetext", view.text);
  }
}

define("ml-reduction-meter", MlReductionMeter);
