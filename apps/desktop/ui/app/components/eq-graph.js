// <ml-eq-graph>: the equaliser's response on a log frequency scale (20 Hz-20 kHz) and ±18 dB, with
// a handle per part (the high-pass filter on the 0 dB line, each band at its frequency and gain).
//
// Property: `data` = { width, height, curve, grid, handles, description, disabled }: the SVG path
//   of the curve, gridLines and bandHandles (processing-view.js), all in a `width` x `height` box,
//   and the curve in words, its text alternative.
// Events (bubbling, composed), each with the handle's `target` ("highPass" or a band's position):
//   `eq-drag` { target, x, y, width, height } while a handle is dragged, in the box's units;
//   `eq-q` { target, steps } for the wheel over a handle (one step a notch;
//   up raises Q); `eq-key` { target, key, shift } for a key on a focused handle. The graph does not
//   change its data itself: the screen answers with new data.
//
// Every handle is focusable and named in words ("Low peak, on, 400 Hz, +6.0 dB, Q 1.00"); the
// keys are listed in its title. A disabled graph (a bypassed strip) draws but takes no gesture.

import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const SVG = "http://www.w3.org/2000/svg";
const KEYS = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "PageUp",
  "PageDown",
  "Home",
  "Enter",
  " ",
]);
const HANDLE_TITLE =
  "Drag to move; wheel or Page Up and Page Down for Q; arrows move it (Shift for bigger steps), " +
  "Home sets 0 dB, Enter turns it on or off";

const styles = sheet(`
  :host {
    display: block;
  }
  .frame {
    position: relative;
    border: 1px solid var(--ml-border-subtle);
    border-radius: var(--ml-radius-md);
    background: var(--ml-ground);
  }
  svg {
    display: block;
    width: 100%;
    height: 200px;
    overflow: visible;
    touch-action: none;
  }
  .grid {
    stroke: var(--ml-border-subtle);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .zero {
    stroke: var(--ml-border);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .curve {
    fill: none;
    stroke: var(--ml-accent);
    stroke-width: 2;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .labels {
    position: absolute;
    inset: 0;
    pointer-events: none;
    color: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-2xs);
  }
  .labels span {
    position: absolute;
    white-space: nowrap;
  }
  .freq {
    bottom: 2px;
    transform: translateX(-50%);
  }
  .gain {
    left: 4px;
    transform: translateY(-50%);
  }
  .handles {
    position: absolute;
    inset: 0;
  }
  .handle {
    position: absolute;
    width: 28px;
    height: 28px;
    margin: -14px 0 0 -14px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: grab;
    touch-action: none;
  }
  .handle::after {
    content: attr(data-mark);
    position: absolute;
    inset: 5px;
    display: grid;
    place-items: center;
    border: 2px solid var(--ml-text);
    border-radius: 50%;
    background: var(--ml-ground);
    color: var(--ml-text);
    font-family: var(--ml-font-mono);
    font-size: 9px;
    font-weight: var(--ml-weight-bold);
  }
  .handle[data-off]::after {
    border-style: dashed;
    border-color: var(--ml-text-muted);
    color: var(--ml-text-muted);
  }
  .handle:active {
    cursor: grabbing;
  }
  .handle:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 0;
  }
  :host([disabled]) .handle {
    cursor: default;
  }
  .description {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
`);

class MlEqGraph extends HTMLElement {
  #data = null;
  #parts;
  #handles = [];
  #dragging = null;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="frame">
        <svg role="img" aria-describedby="description" preserveAspectRatio="none">
          <g class="grid-lines"></g>
          <path class="zero"></path>
          <path class="curve"></path>
        </svg>
        <div class="labels" aria-hidden="true"></div>
        <div class="handles" role="group" aria-label="Equaliser handles"></div>
        <p class="description" id="description"></p>
      </div>`,
    );
    this.#parts = {
      svg: root.querySelector("svg"),
      grid: root.querySelector(".grid-lines"),
      zero: root.querySelector(".zero"),
      curve: root.querySelector(".curve"),
      labels: root.querySelector(".labels"),
      handles: root.querySelector(".handles"),
      description: root.querySelector(".description"),
    };
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    const first = this.#data === null;
    const gridChanged = first || this.#data.width !== data.width || this.#data.height !== data.height;
    this.#data = data;
    const { svg, curve, zero, description } = this.#parts;
    reflect(this, "disabled", data.disabled);
    if (gridChanged) {
      svg.setAttribute("viewBox", `0 0 ${data.width} ${data.height}`);
      this.#drawGrid(data);
      zero.setAttribute("d", `M0 ${data.height / 2}H${data.width}`);
    }
    curve.setAttribute("d", data.curve);
    reflect(svg, "aria-label", "Equaliser response curve");
    setText(description, data.description);
    this.#drawHandles(data);
  }

  #drawGrid({ grid, width, height }) {
    const { grid: lines, labels } = this.#parts;
    const path = [
      ...grid.freqs.map((line) => `M${line.x.toFixed(1)} 0V${height}`),
      ...grid.gains.map((line) => `M0 ${line.y.toFixed(1)}H${width}`),
    ].join("");
    const element = document.createElementNS(SVG, "path");
    element.setAttribute("class", "grid");
    element.setAttribute("d", path);
    lines.replaceChildren(element);
    labels.replaceChildren(
      ...grid.freqs.map((line) => this.#label("freq", `left: ${(100 * line.x) / width}%`, line.label)),
      ...grid.gains
        .filter((line) => line.label !== "0")
        .map((line) => this.#label("gain", `top: ${(100 * line.y) / height}%`, line.label)),
    );
  }

  #label(kind, position, text) {
    const span = document.createElement("span");
    span.className = kind;
    span.style.cssText = position;
    span.textContent = text;
    return span;
  }

  #drawHandles({ handles, width, height }) {
    if (this.#handles.length !== handles.length) {
      this.#handles = handles.map((handle) => this.#newHandle(handle.target));
      this.#parts.handles.replaceChildren(...this.#handles);
    }
    handles.forEach((handle, position) => {
      const button = this.#handles[position];
      button.style.left = `${(100 * handle.x) / width}%`;
      button.style.top = `${(100 * handle.y) / height}%`;
      reflect(button, "aria-label", handle.label);
      reflect(button, "data-off", !handle.enabled);
      reflect(button, "aria-disabled", this.#data.disabled ? "true" : null);
      button.dataset.mark = handle.target === "highPass" ? "H" : String(handle.target + 1);
    });
  }

  #newHandle(target) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "handle";
    button.title = HANDLE_TITLE;
    button.addEventListener("pointerdown", (event) => {
      if (this.#data?.disabled || event.button !== 0) {
        return;
      }
      event.preventDefault();
      button.focus();
      button.setPointerCapture(event.pointerId);
      this.#dragging = target;
    });
    button.addEventListener("pointermove", (event) => {
      if (this.#dragging !== target) {
        return;
      }
      const box = this.#parts.svg.getBoundingClientRect();
      const { width, height } = this.#data;
      emit(this, "eq-drag", {
        target,
        x: ((event.clientX - box.left) / box.width) * width,
        y: ((event.clientY - box.top) / box.height) * height,
        width,
        height,
      });
    });
    const stop = () => {
      if (this.#dragging === target) {
        this.#dragging = null;
      }
    };
    button.addEventListener("pointerup", stop);
    button.addEventListener("pointercancel", stop);
    button.addEventListener("lostpointercapture", stop);
    button.addEventListener(
      "wheel",
      (event) => {
        if (this.#data?.disabled || event.deltaY === 0) {
          return;
        }
        event.preventDefault();
        emit(this, "eq-q", { target, steps: event.deltaY < 0 ? 1 : -1 });
      },
      { passive: false },
    );
    button.addEventListener("keydown", (event) => {
      if (this.#data?.disabled || !KEYS.has(event.key)) {
        return;
      }
      event.preventDefault();
      emit(this, "eq-key", { target, key: event.key, shift: event.shiftKey });
    });
    return button;
  }
}

define("ml-eq-graph", MlEqGraph);
