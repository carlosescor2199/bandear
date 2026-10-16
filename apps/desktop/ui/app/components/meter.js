// <ml-meter>: a peak meter, drawn on a 60 dB scale (green, amber from -6 dBFS, red at full scale).
//
// Attributes: `level` (0-100, the engine's peak), `label` (accessible name, e.g. "Kick input"),
//   `orientation` ("vertical", the default, or "horizontal").
// Method: `setLevel(level)`, also the `level` property. Call it as often as levels arrive (the
//   `meters` event, about 30 a second): it only stores the level and draws once in the next
//   animation frame, as a compositor-only transform. The spoken value follows at most twice a
//   second, so a screen reader is not flooded.
// No events.

import { formatDb } from "../lib/format.js";
import { clampLevel, levelToDb, meterFraction, meterZone } from "../lib/values.js";
import { attachShadowRoot, define, numberAttribute, sheet, syncLabel } from "./support.js";

const SPOKEN_INTERVAL_MS = 500;

const styles = sheet(`
  :host {
    display: inline-block;
    position: relative;
    width: 8px;
    height: 100%;
    min-height: 40px;
    overflow: hidden;
    contain: layout paint;
    border: 1px solid var(--ml-border-subtle);
    border-radius: 3px;
    background: var(--ml-meter-ground);
  }
  :host([orientation="horizontal"]) {
    width: 100%;
    min-width: 40px;
    height: 8px;
    min-height: 0;
  }
  .bar {
    position: absolute;
    inset: 0;
    transform-origin: bottom;
    transform: scaleY(0);
    background: var(--ml-meter-safe);
    will-change: transform;
  }
  :host([orientation="horizontal"]) .bar {
    transform-origin: left;
    transform: scaleX(0);
  }
  .bar[data-zone="hot"] {
    background: var(--ml-meter-hot);
  }
  .bar[data-zone="clip"] {
    background: var(--ml-meter-clip);
  }
`);

class MlMeter extends HTMLElement {
  static observedAttributes = ["level", "label", "orientation"];

  #bar;
  #level = 0;
  #frame = 0;
  #zone = "";
  #spokenAt = -Infinity;
  #spokenTimer = 0;

  constructor() {
    super();
    const root = attachShadowRoot(this, styles, `<div class="bar" part="bar"></div>`);
    this.#bar = root.querySelector(".bar");
  }

  connectedCallback() {
    this.setAttribute("role", "meter");
    this.setAttribute("aria-valuemin", "0");
    this.setAttribute("aria-valuemax", "100");
    syncLabel(this);
    this.#draw();
  }

  disconnectedCallback() {
    cancelAnimationFrame(this.#frame);
    clearTimeout(this.#spokenTimer);
    this.#frame = 0;
    this.#spokenTimer = 0;
  }

  attributeChangedCallback(name) {
    if (name === "level") {
      this.setLevel(numberAttribute(this, "level", 0));
    } else if (name === "label") {
      syncLabel(this);
    } else {
      this.#draw();
    }
  }

  get level() {
    return this.#level;
  }

  set level(level) {
    this.setLevel(level);
  }

  setLevel(level) {
    this.#level = clampLevel(level);
    if (this.#frame === 0 && this.isConnected) {
      this.#frame = requestAnimationFrame(() => this.#draw());
    }
  }

  #draw() {
    this.#frame = 0;
    const fraction = meterFraction(this.#level);
    const axis = this.getAttribute("orientation") === "horizontal" ? "X" : "Y";
    this.#bar.style.transform = `scale${axis}(${fraction})`;
    const zone = meterZone(this.#level);
    if (zone !== this.#zone) {
      this.#zone = zone;
      this.#bar.dataset.zone = zone;
    }
    this.#speak();
  }

  #speak() {
    const now = performance.now();
    const wait = this.#spokenAt + SPOKEN_INTERVAL_MS - now;
    if (wait > 0) {
      // The last level of a burst is still spoken, once the interval has passed.
      if (this.#spokenTimer === 0 && this.isConnected) {
        this.#spokenTimer = setTimeout(() => {
          this.#spokenTimer = 0;
          this.#speak();
        }, wait);
      }
      return;
    }
    this.#spokenAt = now;
    this.setAttribute("aria-valuenow", String(this.#level));
    this.setAttribute("aria-valuetext", formatDb(levelToDb(this.#level)));
  }
}

define("ml-meter", MlMeter);
