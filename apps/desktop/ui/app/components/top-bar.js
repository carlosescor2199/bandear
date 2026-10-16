// <ml-top-bar>: the Mix screen's top bar: the engine's state, the input device and format, the
// phones that need attention (a link to Network and system) and the Start or Stop button.
//
// Property: `data`, from topBarView (mix-view.js):
//   { phase, device, format, attention, attentionLabel, action }.
// Events: `engine-start` and `engine-stop` (bubbling, composed) when the button is pressed.

import "./status-pill.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const BUSY_WORDS = Object.freeze({ starting: "Starting…", stopping: "Stopping…" });

const styles = sheet(`
  :host {
    display: block;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .readout {
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
  }
  .readout span {
    font-family: var(--ml-font-mono);
  }
  .device {
    color: var(--ml-text);
  }
  .spacer {
    flex-grow: 1;
  }
  a,
  button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: var(--ml-font-sans);
    font-weight: var(--ml-weight-bold);
  }
  a:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .attention {
    min-height: var(--ml-target);
    padding: 0 12px;
    border: 1px solid var(--ml-warn-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-warn-bg);
    color: var(--ml-text);
    font-size: var(--ml-text-sm);
    text-decoration: none;
  }
  .attention:hover {
    border-color: var(--ml-accent);
  }
  .attention svg {
    flex: none;
    color: var(--ml-accent);
  }
  .engine {
    min-height: var(--ml-target-primary);
    padding: 0 16px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-surface-raised);
    color: var(--ml-text-muted);
    font-size: var(--ml-text-md);
    cursor: pointer;
  }
  .engine[data-action="start"] {
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  .engine[data-action="stop"] {
    border-color: var(--ml-danger-border);
    background: var(--ml-danger-bg);
    color: var(--ml-danger-text);
  }
  .engine:disabled {
    cursor: default;
  }
`);

class MlTopBar extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="bar">
        <ml-status-pill state="idle"></ml-status-pill>
        <span class="readout" hidden><span class="device"></span> · <span class="format"></span></span>
        <span class="spacer"></span>
        <a class="attention" href="#/network" hidden>
          <svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4 2 20h20L12 4z"/><path d="M12 10v4M12 17v.5"/></svg>
          <span class="attention-text"></span>
        </a>
        <button type="button" class="engine"></button>
      </div>`,
    );
    this.#parts = {
      pill: root.querySelector("ml-status-pill"),
      readout: root.querySelector(".readout"),
      device: root.querySelector(".device"),
      format: root.querySelector(".format"),
      attention: root.querySelector(".attention"),
      attentionText: root.querySelector(".attention-text"),
      button: root.querySelector(".engine"),
    };
    this.#parts.button.addEventListener("click", () => {
      const action = this.#data?.action;
      if (action) {
        emit(this, action === "start" ? "engine-start" : "engine-stop", {});
      }
    });
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    reflect(parts.pill, "state", data.phase);
    parts.readout.hidden = data.device === null;
    setText(parts.device, data.device ?? "");
    setText(parts.format, data.format ?? "");
    parts.attention.hidden = data.attentionLabel === null;
    setText(parts.attentionText, data.attentionLabel ?? "");
    reflect(parts.button, "data-action", data.action);
    parts.button.disabled = data.action === null;
    let word = BUSY_WORDS[data.phase] ?? "";
    if (data.action) {
      word = data.action === "start" ? "Start engine" : "Stop engine";
    }
    setText(parts.button, word);
  }
}

define("ml-top-bar", MlTopBar);
