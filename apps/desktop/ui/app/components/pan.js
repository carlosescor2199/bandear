// <ml-pan>: a horizontal pan control with a centre detent and its own readout ("L20", "C", "R30").
//
// Attributes: `value` (0 hard left, 50 centre, 100 hard right; default 50), `default` (the
//   double-click reset, default 50), `label` (accessible name, e.g. "Kick pan"), `disabled`.
// Property: `value`.
// Events: `input` { value } while it moves, `change` { value } when a gesture ends.
// Keyboard: arrows 1, Page Up and Page Down 10 (both stop at the centre on the way across),
//   Home hard left, End hard right. A drag within 2 of the centre lands on it.
//
// 64 by 36 px: the track and the readout together are the pointer target.

import { PAN, panLabel, panValueText, pointerFraction, snapToCentre } from "../lib/values.js";
import { SliderElement } from "./slider.js";
import { attachShadowRoot, define, sheet } from "./support.js";

const THUMB_WIDTH = 6;

const styles = sheet(`
  :host {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    width: 64px;
    min-height: var(--ml-target);
    padding-bottom: 2px;
    touch-action: none;
    user-select: none;
    cursor: ew-resize;
    border-radius: var(--ml-radius-xs);
  }
  :host(:focus-visible) {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  :host([disabled]) {
    cursor: default;
    opacity: 0.5;
  }
  .track {
    position: relative;
    width: 100%;
    height: 22px;
    --position: 0.5;
  }
  .rail {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 6px;
    margin-top: -3px;
    border-radius: 3px;
    background: var(--ml-track);
  }
  .centre {
    position: absolute;
    left: 50%;
    top: 3px;
    bottom: 3px;
    width: 1px;
    background: var(--ml-text-muted);
  }
  .thumb {
    position: absolute;
    top: 50%;
    left: calc((100% - ${THUMB_WIDTH}px) * var(--position));
    width: ${THUMB_WIDTH}px;
    height: 14px;
    margin-top: -7px;
    border-radius: 2px;
    background: var(--ml-thumb);
    box-shadow: var(--ml-thumb-shadow);
  }
  .readout {
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-2xs);
    line-height: 12px;
    color: var(--ml-text-muted);
  }
`);

class MlPan extends SliderElement {
  static range = PAN;
  static orientation = "horizontal";

  #track;
  #thumb;
  #readout;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="track" part="track">
        <div class="rail"></div><div class="centre"></div><div class="thumb" part="thumb"></div>
      </div>
      <span class="readout" part="readout" aria-hidden="true"></span>`,
    );
    this.#track = root.querySelector(".track");
    this.#thumb = root.querySelector(".thumb");
    this.#readout = root.querySelector(".readout");
  }

  pointerFractionAt(event) {
    const rect = this.#track.getBoundingClientRect();
    return pointerFraction(event.clientX, rect.left + THUMB_WIDTH / 2, rect.width - THUMB_WIDTH);
  }

  isThumb(node) {
    return node === this.#thumb;
  }

  snap(value) {
    return snapToCentre(value, PAN);
  }

  valueText(value) {
    return panValueText(value);
  }

  render(value) {
    this.#track.style.setProperty("--position", String(value / 100));
    this.#readout.textContent = panLabel(value);
  }
}

define("ml-pan", MlPan);
