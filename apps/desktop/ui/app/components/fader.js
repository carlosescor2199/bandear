// <ml-fader>: a vertical level fader, 0-100 %.
//
// Attributes: `value` (0-100, default 100), `default` (the double-click reset, default 100),
//   `label` (accessible name, e.g. "Kick level"), `tone` ("accent" for the musician's own
//   channel, "muted" when the channel is muted; the fill colour only, the strip says why),
//   `disabled`.
// Property: `value`.
// Events: `input` { value } while it moves, `change` { value } when a gesture ends.
// Keyboard: arrows 1 %, Page Up and Page Down 10 %, Home 0 %, End 100 %.
//
// It fills its container's height (at least 160 px) and is 40 px wide, so the whole track is the
// pointer target. The value readout ("62%", "muted") belongs to the strip around it.

import { formatPercent } from "../lib/format.js";
import { FADER, pointerFraction } from "../lib/values.js";
import { SliderElement } from "./slider.js";
import { attachShadowRoot, define, sheet } from "./support.js";

const THUMB_HEIGHT = 20;

const styles = sheet(`
  :host {
    display: inline-block;
    position: relative;
    width: 40px;
    height: 100%;
    min-height: 160px;
    touch-action: none;
    user-select: none;
    cursor: ns-resize;
    border-radius: var(--ml-radius-sm);
    --thumb-height: ${THUMB_HEIGHT}px;
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
    position: absolute;
    inset: 0;
    --position: 1;
  }
  .rail,
  .fill {
    position: absolute;
    left: 50%;
    width: 6px;
    margin-left: -3px;
    border-radius: 3px;
  }
  .rail {
    top: calc(var(--thumb-height) / 2);
    bottom: calc(var(--thumb-height) / 2);
    background: var(--ml-track);
  }
  .fill {
    bottom: calc(var(--thumb-height) / 2);
    height: calc((100% - var(--thumb-height)) * var(--position));
    background: var(--ml-fader-fill);
  }
  :host([tone="accent"]) .fill {
    background: var(--ml-accent);
  }
  :host([tone="muted"]) .fill {
    background: var(--ml-fader-fill-muted);
  }
  .thumb {
    position: absolute;
    left: 50%;
    bottom: calc((100% - var(--thumb-height)) * var(--position));
    width: 34px;
    height: var(--thumb-height);
    margin-left: -17px;
    border-radius: var(--ml-radius-sm);
    background: var(--ml-thumb);
    box-shadow: var(--ml-thumb-shadow);
  }
  .thumb::after {
    content: "";
    position: absolute;
    left: 7px;
    right: 7px;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    border-radius: 1px;
    background: var(--ml-surface-raised);
  }
`);

class MlFader extends SliderElement {
  static range = FADER;
  static orientation = "vertical";

  #track;
  #thumb;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="track" part="track">
        <div class="rail"></div><div class="fill"></div><div class="thumb" part="thumb"></div>
      </div>`,
    );
    this.#track = root.querySelector(".track");
    this.#thumb = root.querySelector(".thumb");
  }

  pointerFractionAt(event) {
    const rect = this.getBoundingClientRect();
    return pointerFraction(event.clientY, rect.top + THUMB_HEIGHT / 2, rect.height - THUMB_HEIGHT, {
      reverse: true,
    });
  }

  isThumb(node) {
    return node === this.#thumb;
  }

  valueText(value) {
    return formatPercent(value);
  }

  render(value) {
    this.#track.style.setProperty("--position", String(value / 100));
  }
}

define("ml-fader", MlFader);
