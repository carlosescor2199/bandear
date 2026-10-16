// <ml-musician-tabs>: one tab per musician, with their name and the state of their link.
//
// Property: `data`, from musicianTabs (mix-view.js): [{ address, label, link, selected }].
// Event: `musician-select` { address } (bubbling, composed) when a tab is clicked or reached with
//   the arrow keys, Home or End (the open tab follows focus). The tabs do not select themselves:
//   the screen answers with new data.
//
// The open tab is marked by aria-selected, a bolder border and a bar under it; each link state is
// a word with a shaped mark, never a colour alone.

import "./link-state.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    display: block;
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  button {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--ml-target-primary);
    padding: 0 14px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: transparent;
    color: var(--ml-text);
    font: var(--ml-weight-bold) var(--ml-text-base) var(--ml-font-sans);
    cursor: pointer;
  }
  button:hover {
    border-color: var(--ml-text-muted);
  }
  button:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  button[aria-selected="true"] {
    border: 2px solid var(--ml-accent);
    background: var(--ml-surface-raised);
    box-shadow: inset 0 -3px 0 var(--ml-accent);
  }
  ml-link-state {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
`);

const NAVIGATION_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"]);

class MlMusicianTabs extends HTMLElement {
  #data = [];
  #list;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="tabs" role="tablist" aria-label="Whose mix"></div>`,
    );
    this.#list = root.querySelector(".tabs");
    this.#list.addEventListener("click", (event) => {
      const tab = event.target.closest("button[role='tab']");
      if (tab) {
        emit(this, "musician-select", { address: tab.dataset.address });
      }
    });
    this.#list.addEventListener("keydown", (event) => this.#onKeyDown(event));
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const tabs = [...this.#list.children];
    const same =
      tabs.length === data.length && tabs.every((tab, i) => tab.dataset.address === data[i].address);
    if (!same) {
      this.#list.replaceChildren(...data.map(() => this.#newTab()));
    }
    const anySelected = data.some((tab) => tab.selected);
    [...this.#list.children].forEach((tab, index) => {
      const { address, label, link, selected } = data[index];
      tab.dataset.address = address;
      reflect(tab, "aria-selected", String(selected));
      // One tab is in the page's tab order: the open one, or the first when none is open.
      tab.tabIndex = selected || (!anySelected && index === 0) ? 0 : -1;
      setText(tab.querySelector(".name"), label);
      const state = tab.querySelector("ml-link-state");
      reflect(state, "level", link.level);
      reflect(state, "label", link.label);
      reflect(state, "detail", link.detail);
    });
  }

  #newTab() {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.setAttribute("role", "tab");
    const name = document.createElement("span");
    name.className = "name";
    tab.append(name, document.createElement("ml-link-state"));
    return tab;
  }

  #onKeyDown(event) {
    if (!NAVIGATION_KEYS.has(event.key)) {
      return;
    }
    const tabs = [...this.#list.children];
    const current = tabs.indexOf(event.target.closest("button[role='tab']"));
    if (current === -1 || tabs.length === 0) {
      return;
    }
    event.preventDefault();
    let next = current;
    if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = tabs.length - 1;
    } else {
      const step = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
      next = (current + step + tabs.length) % tabs.length;
    }
    tabs[next].focus();
    emit(this, "musician-select", { address: tabs[next].dataset.address });
  }
}

define("ml-musician-tabs", MlMusicianTabs);
