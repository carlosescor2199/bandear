// <ml-group-chip>: the small label naming a channel's group, e.g.
//   <ml-group-chip name="Drums" tone="teal"></ml-group-chip>
//
// Attributes: `name` (the group's name; none reads "No group"), `tone` ("teal", "blue",
//   "violet", "orange" or "none"; see groupTone in values.js), `invalid` (the group names an
//   input the current device lacks: a dashed border, a "!" mark and a spoken note).
// No events. The name is always written out, so the tone is never the only cue.

import { attachShadowRoot, define, sheet } from "./support.js";

const styles = sheet(`
  :host {
    display: inline-block;
    --chip-bg: var(--ml-group-none-bg);
    --chip-text: var(--ml-group-none-text);
  }
  :host([tone="teal"]) {
    --chip-bg: var(--ml-group-teal-bg);
    --chip-text: var(--ml-group-teal-text);
  }
  :host([tone="blue"]) {
    --chip-bg: var(--ml-group-blue-bg);
    --chip-text: var(--ml-group-blue-text);
  }
  :host([tone="violet"]) {
    --chip-bg: var(--ml-group-violet-bg);
    --chip-text: var(--ml-group-violet-text);
  }
  :host([tone="orange"]) {
    --chip-bg: var(--ml-group-orange-bg);
    --chip-text: var(--ml-group-orange-text);
  }
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    padding: 2px 7px;
    border: 1px solid transparent;
    border-radius: var(--ml-radius-sm);
    background: var(--chip-bg);
    color: var(--chip-text);
    font-size: var(--ml-text-2xs);
    font-weight: var(--ml-weight-medium);
    line-height: 14px;
    white-space: nowrap;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mark,
  .note {
    display: none;
  }
  :host([invalid]) .chip {
    border-color: currentColor;
    border-style: dashed;
  }
  :host([invalid]) .mark {
    display: inline;
    font-weight: var(--ml-weight-bold);
  }
  :host([invalid]) .note {
    display: inline;
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
`);

class MlGroupChip extends HTMLElement {
  static observedAttributes = ["name", "invalid"];

  #name;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<span class="chip" part="chip"><span class="mark" aria-hidden="true">!</span><span class="name"></span><span class="note">, input missing</span></span>`,
    );
    this.#name = root.querySelector(".name");
  }

  connectedCallback() {
    this.#sync();
  }

  attributeChangedCallback() {
    this.#sync();
  }

  #sync() {
    this.#name.textContent = this.getAttribute("name") || "No group";
    if (this.hasAttribute("invalid")) {
      this.title = "This group names an input the current device does not have";
    } else {
      this.removeAttribute("title");
    }
  }
}

define("ml-group-chip", MlGroupChip);
