// <ml-toggle>: an on/off button such as Mute or Solo, e.g.
//   <ml-toggle kind="mute" label="Mute Kick">M</ml-toggle>
//
// Attributes: `pressed` (boolean), `kind` ("mute" fills red, "solo" fills amber, none fills
//   light), `label` (accessible name; the visible text is the content), `wide` (64 px, for the
//   master's "Mute"), `disabled`.
// Property: `pressed`.
// Events: `change` { pressed } after a click or Space/Enter flips it.
//
// It is a native button inside, so it is focusable and keyboard operable, and it reports
// aria-pressed. On is never shown by colour alone: the button fills and a lamp bar lights under
// the text. 32 by 32 px at least.

import { attachShadowRoot, define, emit, sheet, upgradeProperty } from "./support.js";

const styles = sheet(`
  :host {
    display: inline-block;
    --on-fill: var(--ml-text);
  }
  :host([kind="mute"]) {
    --on-fill: var(--ml-red);
  }
  :host([kind="solo"]) {
    --on-fill: var(--ml-accent);
  }
  button {
    position: relative;
    min-width: var(--ml-target);
    height: var(--ml-target);
    padding: 0 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-surface-raised);
    color: var(--ml-text);
    font: var(--ml-weight-bold) var(--ml-text-sm) / 1 var(--ml-font-sans);
    cursor: pointer;
  }
  :host([wide]) button {
    min-width: 64px;
  }
  button:hover {
    border-color: var(--ml-text-muted);
  }
  button:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  button[aria-pressed="true"] {
    border-color: var(--on-fill);
    background: var(--on-fill);
    color: var(--ml-text-on-bright);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .lamp {
    position: absolute;
    left: 6px;
    right: 6px;
    bottom: 3px;
    height: 2px;
    border-radius: 1px;
    background: currentColor;
    opacity: 0;
  }
  button[aria-pressed="true"] .lamp {
    opacity: 1;
  }
`);

class MlToggle extends HTMLElement {
  static observedAttributes = ["pressed", "label", "disabled"];

  #button;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<button type="button" part="button" aria-pressed="false"><slot></slot><span class="lamp" aria-hidden="true"></span></button>`,
      { delegatesFocus: true },
    );
    this.#button = root.querySelector("button");
    this.#button.addEventListener("click", () => {
      if (this.disabled) {
        return;
      }
      this.pressed = !this.pressed;
      emit(this, "change", { pressed: this.pressed });
    });
  }

  connectedCallback() {
    upgradeProperty(this, "pressed");
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  get pressed() {
    return this.hasAttribute("pressed");
  }

  set pressed(pressed) {
    this.toggleAttribute("pressed", Boolean(pressed));
  }

  get disabled() {
    return this.hasAttribute("disabled");
  }

  set disabled(disabled) {
    this.toggleAttribute("disabled", Boolean(disabled));
  }

  #sync() {
    this.#button.setAttribute("aria-pressed", String(this.pressed));
    this.#button.disabled = this.disabled;
    const label = this.getAttribute("label");
    if (label) {
      this.#button.setAttribute("aria-label", label);
    } else {
      this.#button.removeAttribute("aria-label");
    }
  }
}

define("ml-toggle", MlToggle);
