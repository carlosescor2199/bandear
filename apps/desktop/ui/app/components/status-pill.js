// <ml-status-pill>: the engine's state as a pill, e.g. <ml-status-pill state="running">.
//
// Attributes: `state` ("idle", "starting", "running", "stopping", "stopped" or "error"; see
//   PHASES in console-state.js), `label` (text to show instead of the state's own word).
// No events. The host is a polite live region (role "status"), so a change is announced. The word
//   always names the state; the dot's shape (filled, pulsing, hollow, square) repeats it.

import { attachShadowRoot, define, sheet } from "./support.js";

const WORDS = Object.freeze({
  idle: "Not running",
  starting: "Starting",
  running: "Running",
  stopping: "Stopping",
  stopped: "Stopped",
  error: "Error",
});

const styles = sheet(`
  :host {
    display: inline-block;
    --pill-bg: var(--ml-surface-raised);
    --pill-border: var(--ml-border);
    --dot: var(--ml-text-muted);
  }
  :host([state="running"]) {
    --pill-bg: var(--ml-ok-bg);
    --pill-border: var(--ml-ok-border);
    --dot: var(--ml-green);
  }
  :host([state="starting"]),
  :host([state="stopping"]) {
    --pill-bg: var(--ml-warn-bg);
    --pill-border: var(--ml-warn-border);
    --dot: var(--ml-accent);
  }
  :host([state="error"]) {
    --pill-bg: var(--ml-danger-bg);
    --pill-border: var(--ml-danger-border);
    --dot: var(--ml-red);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: var(--ml-target);
    padding: 0 12px;
    border: 1px solid var(--pill-border);
    border-radius: var(--ml-radius-xl);
    background: var(--pill-bg);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--dot);
  }
  :host(:not([state])) .dot,
  :host([state="idle"]) .dot,
  :host([state="stopped"]) .dot {
    border: 2px solid var(--dot);
    background: transparent;
  }
  :host([state="error"]) .dot {
    border-radius: 1px;
  }
  :host([state="starting"]) .dot,
  :host([state="stopping"]) .dot {
    animation: pulse 1s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    to {
      opacity: 0.3;
    }
  }
`);

class MlStatusPill extends HTMLElement {
  static observedAttributes = ["state", "label"];

  #text;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<span class="pill" part="pill"><span class="dot" aria-hidden="true"></span><span class="text"></span></span>`,
    );
    this.#text = root.querySelector(".text");
  }

  connectedCallback() {
    this.setAttribute("role", "status");
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  #sync() {
    const state = this.getAttribute("state") ?? "idle";
    this.#text.textContent = this.getAttribute("label") || WORDS[state] || WORDS.idle;
  }
}

define("ml-status-pill", MlStatusPill);
