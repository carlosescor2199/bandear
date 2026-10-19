// <ml-record-track>: one track of the Record screen: its arm button, its name and source, a live
// input meter and the waveform lane drawn while a take records.
//
// Property: `data`, a row from trackRows or localTrackRow (record-view.js) plus `{ locked, lane }`:
//   `locked` while a take records (arming stays as it is), `lane` "off" (not armed), "armed"
//   (records with the next take) or "recording".
// Methods: `setLevel(level)`, the meter's level (0-100, as `meters` gives it); `drawHistory(history)`,
//   draws the lane from a peak history (createPeakHistory, record-view.js), newest at the right.
//   The screen calls it at most once an animation frame.
// Events (bubbling, composed): `track-arm` { key } when the arm button is pressed.

import "./meter.js";
import { meterFraction } from "../lib/values.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const LANE_NOTES = Object.freeze({
  off: "Not armed: this input is not being recorded",
  armed: "Armed: records with the next take",
  recording: "",
});

const LOCAL_NOTES = Object.freeze({
  off: "Not recorded: arm it to record the local mix as a stereo file",
  armed: "Armed: the local mix records with the next take",
  recording: "",
});

const styles = sheet(`
  :host {
    display: block;
    border-bottom: 1px solid var(--ml-border-subtle);
  }
  :host(:last-child) {
    border-bottom: 0;
  }
  .row {
    display: flex;
    min-height: 48px;
  }
  .row[data-local] {
    background: var(--ml-surface-sunken);
  }
  .head {
    display: flex;
    flex: 0 0 260px;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 6px 14px;
    border-right: 1px solid var(--ml-border-subtle);
  }
  .arm {
    flex: none;
    width: var(--ml-target);
    height: var(--ml-target);
    padding: 0;
    border: 1px solid var(--ml-border);
    border-radius: 50%;
    background: var(--ml-surface-raised);
    color: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-xs);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  .arm[aria-pressed="true"] {
    border-color: var(--ml-red);
    background: var(--ml-red);
    color: var(--ml-text-on-bright);
  }
  .arm:disabled {
    cursor: default;
    opacity: 0.7;
  }
  .arm:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .names {
    flex: 1 1 auto;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    font-size: var(--ml-text-md);
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .source {
    color: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-xs);
  }
  ml-meter {
    flex: 0 0 56px;
    width: 56px;
  }
  .lane {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
  }
  canvas {
    position: absolute;
    inset: 4px 0;
    width: 100%;
    height: calc(100% - 8px);
  }
  .note {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    padding: 0 12px;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .note:empty {
    display: none;
  }
`);

class MlRecordTrack extends HTMLElement {
  #data = null;
  #parts;
  #color = "";

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="row">
        <div class="head">
          <button type="button" class="arm" aria-pressed="false">R</button>
          <div class="names"><div class="name"></div><div class="source"></div></div>
          <ml-meter orientation="horizontal"></ml-meter>
        </div>
        <div class="lane">
          <canvas aria-hidden="true"></canvas>
          <span class="note"></span>
        </div>
      </div>`,
    );
    this.#parts = {
      row: root.querySelector(".row"),
      arm: root.querySelector(".arm"),
      name: root.querySelector(".name"),
      source: root.querySelector(".source"),
      meter: root.querySelector("ml-meter"),
      canvas: root.querySelector("canvas"),
      note: root.querySelector(".note"),
    };
    this.#parts.arm.addEventListener("click", () => {
      if (this.#data) {
        emit(this, "track-arm", { key: this.#data.key });
      }
    });
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    reflect(parts.row, "data-local", Boolean(data.local));
    reflect(parts.arm, "aria-pressed", data.armed ? "true" : "false");
    reflect(
      parts.arm,
      "aria-label",
      data.local ? "Record the local mix" : `Arm ${data.name} (${data.source})`,
    );
    reflect(
      parts.arm,
      "title",
      data.locked ? "Arming stays as it is while a take records" : data.armed ? "Disarm" : "Arm",
    );
    parts.arm.disabled = Boolean(data.locked);
    setText(parts.name, data.name);
    setText(parts.source, data.source);
    reflect(parts.meter, "label", `${data.name} level`);
    const notes = data.local ? LOCAL_NOTES : LANE_NOTES;
    setText(parts.note, notes[data.lane] ?? "");
    parts.canvas.hidden = data.lane === "off";
  }

  setLevel(level) {
    this.#parts.meter.setLevel(level);
  }

  /** Draws the lane from `history`, newest at the right edge, each level as a mirrored bar. */
  drawHistory(history) {
    const canvas = this.#parts.canvas;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (width === 0 || height === 0) {
      return;
    }
    const scale = window.devicePixelRatio || 1;
    const pixelsWide = Math.round(width * scale);
    const pixelsHigh = Math.round(height * scale);
    if (canvas.width !== pixelsWide || canvas.height !== pixelsHigh) {
      canvas.width = pixelsWide;
      canvas.height = pixelsHigh;
    }
    if (!this.#color) {
      const tokens = getComputedStyle(this);
      const name = this.#data?.local ? "--ml-accent" : "--ml-group-teal-text";
      this.#color = tokens.getPropertyValue(name).trim() || "#7fe0d0";
    }
    const context = canvas.getContext("2d");
    context.setTransform(scale, 0, 0, scale, 0, 0);
    context.clearRect(0, 0, width, height);
    const middle = height / 2;
    const step = width / history.capacity;
    const start = width - history.length * step;
    context.fillStyle = this.#color;
    context.globalAlpha = 0.8;
    history.forEach((level, index) => {
      const half = Math.max(0.5, meterFraction(level) * (middle - 1));
      context.fillRect(start + index * step, middle - half, Math.max(1, step), half * 2);
    });
    context.globalAlpha = 1;
  }
}

define("ml-record-track", MlRecordTrack);
