// <ml-rec-chip>: the REC chip the shell shows above every screen while a take records, its files
// are being finished or it stopped on an error. It is a link to the Record screen.
//
// Property: `data`, from recChip (record-view.js): `{ kind, word, time, detail, label }`, or null
//   to hide the chip. `kind` is "recording", "stopping" or "failed"; `label` is the link's
//   accessible name, which says it all in words.
// No events: it is a plain link to #/record.

import { attachShadowRoot, define, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    display: inline-block;
  }
  a {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: var(--ml-target);
    padding: 0 12px;
    border: 1px solid var(--ml-danger-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-danger-bg);
    color: var(--ml-text);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    text-decoration: none;
  }
  a:hover {
    border-color: var(--ml-red);
  }
  a:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  a[data-kind="stopping"] {
    border-color: var(--ml-warn-border);
    background: var(--ml-warn-bg);
  }
  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--ml-red);
  }
  a[data-kind="recording"] .dot {
    animation: pulse 1.2s ease-in-out infinite;
  }
  a[data-kind="stopping"] .dot {
    background: var(--ml-accent);
  }
  a[data-kind="failed"] .dot {
    border-radius: 2px;
  }
  .time {
    font-family: var(--ml-font-mono);
  }
  .detail {
    color: var(--ml-text-secondary);
    font-weight: var(--ml-weight-medium);
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
`);

class MlRecChip extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<a href="#/record">
        <span class="dot" aria-hidden="true"></span>
        <span class="word" aria-hidden="true"></span>
        <span class="time" aria-hidden="true"></span>
        <span class="detail" aria-hidden="true"></span>
      </a>`,
    );
    this.#parts = {
      link: root.querySelector("a"),
      word: root.querySelector(".word"),
      time: root.querySelector(".time"),
      detail: root.querySelector(".detail"),
    };
  }

  connectedCallback() {
    this.hidden = this.#data === null;
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    this.hidden = data === null;
    if (!data) {
      return;
    }
    const parts = this.#parts;
    reflect(parts.link, "data-kind", data.kind);
    reflect(parts.link, "aria-label", data.label);
    reflect(parts.link, "title", data.label);
    setText(parts.word, data.word);
    setText(parts.time, data.time);
    setText(parts.detail, data.detail);
  }
}

define("ml-rec-chip", MlRecChip);
