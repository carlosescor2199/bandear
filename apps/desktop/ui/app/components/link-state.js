// <ml-link-state>: a phone's link health as a mark and a word, e.g.
//   <ml-link-state level="warning" label="Warning" detail="Loss 0.82% · ..."></ml-link-state>
//
// Attributes: `level` ("good", "warning", "bad" or "unknown"), `label` (the word; linkState in
//   format.js gives it, otherwise the level's own word), `detail` (the numbers, as a tooltip).
// No events. The word always names the state; the mark's shape repeats it: a dot for good, a
//   triangle for a warning, a square for bad, a ring for unknown.

import { attachShadowRoot, define, sheet } from "./support.js";

const WORDS = Object.freeze({
  good: "Good",
  warning: "Warning",
  bad: "Poor",
  unknown: "Unknown",
});

const styles = sheet(`
  :host {
    display: inline-block;
    --mark: var(--ml-text-muted);
  }
  :host([level="good"]) {
    --mark: var(--ml-green);
  }
  :host([level="warning"]) {
    --mark: var(--ml-accent);
  }
  :host([level="bad"]) {
    --mark: var(--ml-red);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
  }
  .mark {
    flex: none;
    width: 8px;
    height: 8px;
    border: 2px solid var(--mark);
    border-radius: 50%;
  }
  :host([level="good"]) .mark {
    background: var(--mark);
  }
  :host([level="warning"]) .mark {
    width: 10px;
    height: 9px;
    border: 0;
    border-radius: 0;
    background: var(--mark);
    clip-path: polygon(50% 0, 100% 100%, 0 100%);
  }
  :host([level="bad"]) .mark {
    border-radius: 1px;
    background: var(--mark);
  }
`);

class MlLinkState extends HTMLElement {
  static observedAttributes = ["level", "label", "detail"];

  #text;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<span class="link" part="link"><span class="mark" aria-hidden="true"></span><span class="text"></span></span>`,
    );
    this.#text = root.querySelector(".text");
  }

  connectedCallback() {
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  #sync() {
    const level = this.getAttribute("level") ?? "unknown";
    this.#text.textContent = this.getAttribute("label") || WORDS[level] || WORDS.unknown;
    const detail = this.getAttribute("detail");
    if (detail) {
      this.title = detail;
    } else {
      this.removeAttribute("title");
    }
  }
}

define("ml-link-state", MlLinkState);
