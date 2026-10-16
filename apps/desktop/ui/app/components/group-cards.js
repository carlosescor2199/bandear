// <ml-group-cards>: the groups as cards, each with its members, and the editor behind each card's
// Edit: its name, the inputs in it, and a guarded Delete.
//
// Property: `data` = { cards, canAdd }: cards from groupCards (channels-view.js); `canAdd` is false
//   once the engine's 16 groups are there.
// Events (bubbling, composed): `group-add` {} for New group, `group-rename` { id, name } when a
//   name is committed, `group-member` { id, key, numbers, member } when an input (both sides of a
//   pair; `key` the choice's) is put in or out, `group-delete` { id } when a deletion is confirmed.
//
// A name commits on Enter or when its field loses focus; Escape puts the saved name back; a field
// being typed in is never overwritten. Delete asks first: "Keep" takes Delete's place and the
// confirming Delete only works after a short pause, so a double click or a stray click cannot
// delete a group. Cards are kept by group id, so an open editor stays open when another group is
// added or removed.

import "./group-chip.js";
import { CONFIRM_DELAY_MS, confirmReady } from "../lib/network-view.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const ARMED_FOR_MS = 10000;

// Numbers each card's editor, so its Edit button can name it.
let editors = 0;

const styles = sheet(`
  :host {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  h2 {
    margin: 0;
    font-size: var(--ml-text-base);
    font-weight: var(--ml-weight-bold);
  }
  .note,
  .hint,
  .count,
  .empty,
  .owner {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .grow {
    flex: 1 1 auto;
  }
  input,
  button {
    font: inherit;
    color: inherit;
  }
  button {
    min-height: var(--ml-target);
    padding: 0 12px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-surface-raised);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  .add {
    min-height: var(--ml-target-primary);
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  button[aria-disabled="true"] {
    cursor: default;
    opacity: 0.5;
  }
  .danger {
    border-color: var(--ml-danger-border);
    background: var(--ml-danger-bg);
    color: var(--ml-danger-text);
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-lg);
    background: var(--ml-surface);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .top ml-group-chip {
    min-width: 0;
  }
  .members {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .members li {
    padding: 3px 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--ml-border-subtle);
  }
  label.field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
  }
  .group-name {
    min-height: var(--ml-target);
    padding: 0 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
    font-size: var(--ml-text-md);
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: 4px;
    padding: 0;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
  }
  .choices {
    display: flex;
    flex-direction: column;
    max-height: 260px;
    overflow-y: auto;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--ml-target);
    font-size: var(--ml-text-md);
    cursor: pointer;
  }
  .choice input {
    width: 18px;
    height: 18px;
    margin: 0;
    accent-color: var(--ml-accent);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .confirm {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  [hidden] {
    display: none !important;
  }
`);

class MlGroupCards extends HTMLElement {
  #data = { cards: [], canAdd: true };
  #parts;
  #views = new Map();
  #open = new Set();
  #armed = null;
  #timers = [];

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="head">
        <h2>Groups</h2>
        <span class="note">up to 16</span>
        <span class="grow"></span>
        <button type="button" class="add">New group</button>
      </div>
      <p class="hint" data-full hidden>The engine takes at most 16 groups: delete one to make another.</p>
      <p class="empty" data-empty hidden>No groups yet. A group gives every musician one fader for its inputs.</p>
      <div class="cards"></div>`,
    );
    this.#parts = {
      add: root.querySelector(".add"),
      full: root.querySelector("[data-full]"),
      empty: root.querySelector("[data-empty]"),
      cards: root.querySelector(".cards"),
    };
    this.#parts.add.addEventListener("click", () => {
      if (this.#data.canAdd) {
        emit(this, "group-add", {});
      }
    });
  }

  disconnectedCallback() {
    this.#disarm();
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    reflect(parts.add, "aria-disabled", data.canAdd ? null : "true");
    parts.full.hidden = data.canAdd;
    parts.empty.hidden = data.cards.length > 0;
    const ids = new Set(data.cards.map((card) => card.id));
    for (const [id, view] of this.#views) {
      if (!ids.has(id)) {
        view.card.remove();
        this.#views.delete(id);
        this.#open.delete(id);
        if (this.#armed?.id === id) {
          this.#disarm();
        }
      }
    }
    // Each card keeps its element; only one out of place moves, so focus inside stays put.
    data.cards.forEach((card, position) => {
      let view = this.#views.get(card.id);
      if (!view) {
        view = this.#newCard(card.id);
        this.#views.set(card.id, view);
      }
      const here = parts.cards.children[position];
      if (here !== view.card) {
        parts.cards.insertBefore(view.card, here ?? null);
      }
      this.#update(view, card);
    });
  }

  #newCard(id) {
    const card = document.createElement("section");
    card.className = "card";
    card.innerHTML = `
      <div class="top">
        <ml-group-chip></ml-group-chip>
        <span class="count"></span>
        <span class="grow"></span>
        <button type="button" class="edit" aria-expanded="false">Edit</button>
      </div>
      <ul class="members"></ul>
      <p class="empty" data-none>No channels yet.</p>
      <div class="editor" hidden>
        <label class="field">Name<input class="group-name" type="text" autocomplete="off" spellcheck="false" /></label>
        <fieldset>
          <legend></legend>
          <div class="choices"></div>
        </fieldset>
        <p class="hint">Choosing an input that is in another group moves it here. A stereo pair joins as one.</p>
        <div class="actions"><span class="delete"></span><button type="button" class="done">Done</button></div>
      </div>`;
    const view = {
      id,
      card,
      chip: card.querySelector("ml-group-chip"),
      count: card.querySelector(".count"),
      edit: card.querySelector(".edit"),
      members: card.querySelector(".members"),
      none: card.querySelector("[data-none]"),
      editor: card.querySelector(".editor"),
      name: card.querySelector(".group-name"),
      legend: card.querySelector("legend"),
      choices: card.querySelector(".choices"),
      delete: card.querySelector(".delete"),
      done: card.querySelector(".done"),
      saved: "",
      choiceViews: new Map(),
      label: "",
    };
    editors += 1;
    view.editor.id = `group-editor-${editors}`;
    view.edit.setAttribute("aria-controls", view.editor.id);
    view.edit.addEventListener("click", () => this.#toggle(view, !this.#open.has(id)));
    view.done.addEventListener("click", () => {
      this.#toggle(view, false);
      view.edit.focus();
    });
    view.name.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        view.name.blur();
      } else if (event.key === "Escape") {
        event.stopPropagation();
        view.name.value = view.saved;
        view.name.blur();
      }
    });
    view.name.addEventListener("change", () => {
      const name = view.name.value.trim();
      if (name !== view.saved) {
        emit(this, "group-rename", { id, name });
      }
    });
    card.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.#armed?.id === id) {
        this.#disarm();
        this.#renderDelete(view);
      }
    });
    this.#renderDelete(view);
    return view;
  }

  #toggle(view, open) {
    if (open) {
      this.#open.add(view.id);
    } else {
      this.#open.delete(view.id);
      if (this.#armed?.id === view.id) {
        this.#disarm();
        this.#renderDelete(view);
      }
    }
    view.editor.hidden = !open;
    reflect(view.edit, "aria-expanded", String(open));
    setText(view.edit, open ? "Close" : "Edit");
    if (open) {
      view.name.focus();
    }
  }

  #update(view, card) {
    view.label = card.name;
    reflect(view.card, "aria-label", `${card.name} group`);
    reflect(view.chip, "name", card.name);
    reflect(view.chip, "tone", card.tone);
    reflect(view.chip, "invalid", card.invalid);
    setText(view.count, card.count);
    reflect(view.edit, "aria-label", `${this.#open.has(view.id) ? "Close" : "Edit"} ${card.name}`);
    view.members.replaceChildren(
      ...card.members.map((member) => {
        const item = document.createElement("li");
        item.textContent = member;
        return item;
      }),
    );
    view.none.hidden = card.members.length > 0;

    const open = this.#open.has(view.id);
    view.editor.hidden = !open;
    const typing = this.shadowRoot.activeElement === view.name;
    view.saved = card.name;
    if (!typing) {
      view.name.value = card.name;
    }
    reflect(view.name, "aria-label", `Name of the ${card.name} group`);
    setText(view.legend, `Inputs in ${card.name}`);
    this.#updateChoices(view, card);
    this.#renderDelete(view);
  }

  #updateChoices(view, card) {
    const keys = card.choices.map((choice) => choice.key);
    const same =
      keys.length === view.choiceViews.size && keys.every((key) => view.choiceViews.has(key));
    if (!same) {
      view.choiceViews.clear();
      view.choices.replaceChildren(
        ...card.choices.map((choice) => {
          const label = document.createElement("label");
          label.className = "choice";
          label.innerHTML = `<input type="checkbox" /><span class="text"></span><span class="owner"></span>`;
          const box = label.querySelector("input");
          const entry = {
            box,
            text: label.querySelector(".text"),
            owner: label.querySelector(".owner"),
            numbers: choice.numbers,
          };
          box.addEventListener("change", () => {
            emit(this, "group-member", {
              id: view.id,
              key: choice.key,
              numbers: entry.numbers,
              member: box.checked,
            });
          });
          view.choiceViews.set(choice.key, entry);
          return label;
        }),
      );
    }
    for (const choice of card.choices) {
      const entry = view.choiceViews.get(choice.key);
      entry.numbers = choice.numbers;
      entry.box.checked = choice.checked;
      setText(entry.text, choice.label);
      const notes = [];
      if (choice.owner) {
        notes.push(`in ${choice.owner}`);
      }
      if (choice.hidden) {
        notes.push("hidden");
      }
      setText(entry.owner, notes.join(", "));
    }
  }

  #renderDelete(view) {
    const armed = this.#armed?.id === view.id;
    const shown = view.delete.dataset.state;
    if (!armed && shown === "idle") {
      reflect(view.delete.firstElementChild, "aria-label", `Delete the ${view.label} group`);
      return;
    }
    if (!armed) {
      view.delete.dataset.state = "idle";
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Delete group";
      remove.setAttribute("aria-label", `Delete the ${view.label} group`);
      remove.addEventListener("click", () => this.#arm(view));
      view.delete.replaceChildren(remove);
      return;
    }
    const ready = confirmReady(this.#armed.at, performance.now());
    if (shown === "armed") {
      reflect(view.delete.querySelector(".danger"), "aria-disabled", ready ? null : "true");
      return;
    }
    view.delete.dataset.state = "armed";
    const prompt = document.createElement("span");
    prompt.className = "confirm";
    const question = document.createElement("span");
    question.textContent = `Delete ${view.label}? Its inputs keep their own faders.`;
    const keep = document.createElement("button");
    keep.type = "button";
    keep.textContent = "Keep";
    keep.addEventListener("click", () => {
      this.#disarm();
      this.#renderDelete(view);
    });
    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.className = "danger";
    confirm.textContent = "Delete";
    reflect(confirm, "aria-disabled", ready ? null : "true");
    confirm.addEventListener("click", () => {
      if (this.#armed?.id === view.id && confirmReady(this.#armed.at, performance.now())) {
        this.#disarm();
        emit(this, "group-delete", { id: view.id });
        this.#renderDelete(view);
      }
    });
    // Keep sits where Delete was, so a second click on the same spot keeps the group.
    prompt.append(keep, question, confirm);
    view.delete.replaceChildren(prompt);
    keep.focus();
  }

  #arm(view) {
    const previous = this.#armed ? this.#views.get(this.#armed.id) : null;
    this.#disarm();
    if (previous && previous !== view) {
      this.#renderDelete(previous);
    }
    this.#armed = { id: view.id, at: performance.now() };
    this.#timers.push(
      setTimeout(() => this.#renderDelete(view), CONFIRM_DELAY_MS),
      setTimeout(() => {
        if (this.#armed?.id === view.id) {
          this.#disarm();
          this.#renderDelete(view);
        }
      }, ARMED_FOR_MS),
    );
    this.#renderDelete(view);
  }

  #disarm() {
    this.#armed = null;
    for (const timer of this.#timers.splice(0)) {
      clearTimeout(timer);
    }
  }
}

define("ml-group-cards", MlGroupCards);
