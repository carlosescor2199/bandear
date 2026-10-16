// <ml-mix-tools>: the Mix screen's tools for the open musician's mix, beside the musician tabs:
// "Mirror" (follow no one or another musician's mix), "Copy mix to…" (this mix onto others) and
// "Presets" (save this mix as a preset, apply one to musicians, rename or delete one).
//
// Property: `data`, mixToolsView (mixes.js) and the open musician's `address`:
//   { address, name, mirror: { value, options, note }, copyTargets, presetTargets, presets,
//   canUse }.
// Method: `answered(kind, ok)` after the screen's command for `kind` ("mirror", "save", "rename",
//   "copy", "apply", "delete") was answered: a done save or rename empties its field, a done copy
//   unticks its musicians, and the Mirror control shows the desk's mirror again whatever the
//   answer (a refused choice never stays on show).
// Events (bubbling, composed): `mirror-change` { leader } (an address, or null for none),
//   `mix-copy` { to }, `preset-save` { name }, `preset-apply` { id, to }, `preset-rename` { id,
//   name }, `preset-delete` { id } once a deletion is confirmed, `presets-load` {} to read the
//   presets again. The tools change no data themselves: the screen answers with new data.
//
// Copy and Presets are disclosures: one panel opens under the row at a time, Escape closes it and
// gives the focus back to its button. A musician whose mix follows another's is offered disabled,
// with the reason beside it. Delete asks first, as a group's does: "Keep" takes Delete's place and
// the confirming Delete only works after a short pause. The rules live in createMixTools
// (mix-tools.js), tested without a browser; this element renders what it answers.

import { createMixTools } from "../lib/mix-tools.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    position: relative;
    display: block;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-md);
  }
  select,
  input[type="text"],
  button {
    min-height: 36px;
    padding: 0 10px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-ground);
    color: var(--ml-text);
    font: var(--ml-weight-medium) var(--ml-text-md) var(--ml-font-sans);
  }
  button {
    background: var(--ml-surface-raised);
    cursor: pointer;
  }
  button[aria-expanded="true"] {
    border-color: var(--ml-accent);
  }
  button.primary {
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
    font-weight: var(--ml-weight-bold);
  }
  button.danger {
    border-color: var(--ml-danger-border);
    background: var(--ml-danger-bg);
    color: var(--ml-danger-text);
  }
  button[aria-disabled="true"] {
    cursor: default;
    opacity: 0.55;
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .note {
    margin: 6px 0 0;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
    text-align: right;
  }
  .panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(440px, calc(100vw - 32px));
    padding: 14px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-lg);
    background: var(--ml-surface);
    box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
  }
  .panel h2 {
    margin: 0;
    font-size: var(--ml-text-base);
    font-weight: var(--ml-weight-bold);
  }
  fieldset {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--ml-border-subtle);
    border-radius: var(--ml-radius-md);
  }
  legend {
    padding: 0 4px;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-sm);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .choice {
    min-height: 28px;
    color: var(--ml-text);
  }
  .choice .why,
  .hint,
  .empty {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .field {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .field input {
    flex: 1 1 160px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .confirm {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  [hidden] {
    display: none !important;
  }
`);

class MlMixTools extends HTMLElement {
  #data = null;
  #parts;
  #tools = createMixTools({
    emit: (type, detail) => emit(this, type, detail),
    // A delete that became confirmable, or disarmed itself.
    changed: () => this.#renderDelete(),
  });

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="row">
        <label>Mirror <select class="mirror"></select></label>
        <button type="button" class="open" data-panel="copy" aria-expanded="false" aria-controls="copy-panel">Copy mix to…</button>
        <button type="button" class="open" data-panel="presets" aria-expanded="false" aria-controls="presets-panel">Presets</button>
      </div>
      <p class="note" id="mirror-note" data-mirror-note hidden></p>
      <section class="panel" id="copy-panel" data-for="copy" hidden>
        <h2 data-copy-title></h2>
        <fieldset><legend>Copy to</legend><div class="choices" data-list="copy"></div></fieldset>
        <p class="hint">Channels, pans, mutes, solos and group levels. Never their master, their phone's cap or your limit and locks.</p>
        <p class="hint" data-copy-idle hidden>Start the engine to copy a mix.</p>
        <div class="actions"><button type="button" class="primary" data-copy>Copy</button><button type="button" data-close>Close</button></div>
      </section>
      <section class="panel" id="presets-panel" data-for="presets" hidden>
        <h2>Presets</h2>
        <div class="field">
          <label for="save-name" data-save-label></label>
          <input id="save-name" type="text" autocomplete="off" spellcheck="false" maxlength="40" />
          <button type="button" data-save>Save preset</button>
        </div>
        <p class="hint" data-presets-idle hidden>Start the engine to save or apply presets.</p>
        <p class="empty" data-unread hidden>The presets could not be read. <button type="button" data-load>Read them again</button></p>
        <p class="empty" data-none hidden>No presets yet.</p>
        <div data-has hidden>
          <div class="field">
            <label>Preset <select class="preset"></select></label>
          </div>
          <fieldset><legend>Apply to</legend><div class="choices" data-list="apply"></div></fieldset>
          <div class="actions"><button type="button" class="primary" data-apply>Apply</button></div>
          <div class="field">
            <label for="rename-to">Rename to</label>
            <input id="rename-to" type="text" autocomplete="off" spellcheck="false" maxlength="40" />
            <button type="button" data-rename>Rename</button>
          </div>
          <div class="actions"><span class="delete"></span></div>
        </div>
        <div class="actions"><button type="button" data-close>Close</button></div>
      </section>`,
    );
    const pick = (selector) => root.querySelector(selector);
    this.#parts = {
      mirror: pick("select.mirror"),
      mirrorNote: pick("[data-mirror-note]"),
      openers: [...root.querySelectorAll("button.open")],
      panels: [...root.querySelectorAll(".panel")],
      copyTitle: pick("[data-copy-title]"),
      copyChoices: pick('[data-list="copy"]'),
      copyIdle: pick("[data-copy-idle]"),
      copy: pick("[data-copy]"),
      saveLabel: pick("[data-save-label]"),
      saveName: pick("#save-name"),
      save: pick("[data-save]"),
      presetsIdle: pick("[data-presets-idle]"),
      unread: pick("[data-unread]"),
      none: pick("[data-none]"),
      has: pick("[data-has]"),
      preset: pick("select.preset"),
      applyChoices: pick('[data-list="apply"]'),
      apply: pick("[data-apply]"),
      renameTo: pick("#rename-to"),
      rename: pick("[data-rename]"),
      delete: pick(".delete"),
    };
    const parts = this.#parts;
    const tools = this.#tools;
    parts.mirror.addEventListener("change", () => {
      tools.chooseMirror(parts.mirror.value);
      this.#renderMirror();
    });
    for (const opener of parts.openers) {
      opener.addEventListener("click", () => this.#focus(tools.toggle(opener.dataset.panel)));
    }
    for (const panel of parts.panels) {
      panel.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          this.#focus(tools.escape());
        }
      });
    }
    for (const close of root.querySelectorAll("[data-close]")) {
      close.addEventListener("click", () => this.#focus(tools.escape()));
    }
    for (const list of [parts.copyChoices, parts.applyChoices]) {
      list.addEventListener("change", (event) => {
        const box = event.target;
        if (box?.type === "checkbox") {
          tools.tick(list.dataset.list, box.value, box.checked);
          this.#render();
        }
      });
    }
    parts.copy.addEventListener("click", () => tools.copy());
    parts.save.addEventListener("click", () => tools.save(parts.saveName.value));
    parts.saveName.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        tools.save(parts.saveName.value);
      }
    });
    root.querySelector("[data-load]").addEventListener("click", () => emit(this, "presets-load", {}));
    parts.preset.addEventListener("change", () => {
      tools.keep();
      this.#renderPresets();
    });
    parts.apply.addEventListener("click", () => tools.apply(parts.preset.value));
    parts.rename.addEventListener("click", () => tools.rename(parts.preset.value, parts.renameTo.value));
    parts.renameTo.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        tools.rename(parts.preset.value, parts.renameTo.value);
      }
    });
  }

  disconnectedCallback() {
    this.#tools.dispose();
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    this.#tools.setData(data);
    this.#render();
  }

  /** The screen's command for `kind` was answered (see createMixTools' `answered`). */
  answered(kind, ok) {
    const { clearSave, clearRename } = this.#tools.answered(kind, ok);
    if (clearSave) {
      this.#parts.saveName.value = "";
    }
    if (clearRename) {
      this.#parts.renameTo.value = "";
    }
    if (this.#data) {
      this.#render();
    }
  }

  #render() {
    const parts = this.#parts;
    const data = this.#data;
    if (!data) {
      return;
    }
    this.#renderMirror();
    setText(parts.copyTitle, `Copy ${data.name}'s mix`);
    setText(parts.saveLabel, `Save ${data.name}'s mix as`);
    this.#renderChoices(parts.copyChoices, data.copyTargets);
    parts.copyIdle.hidden = data.canUse;
    reflect(parts.copy, "aria-disabled", this.#tools.copyReady() ? null : "true");
    this.#renderPresets();
  }

  // The Mirror control always shows the desk's mirror, or the choice on its way to the desk, also
  // after its options were rebuilt.
  #renderMirror() {
    if (!this.#data) {
      return;
    }
    const { mirror } = this.#data;
    const select = this.#parts.mirror;
    const same =
      select.options.length === mirror.options.length &&
      mirror.options.every(
        (option, index) =>
          select.options[index].value === option.value &&
          select.options[index].textContent === option.label,
      );
    if (!same) {
      select.replaceChildren(
        ...mirror.options.map((option) => {
          const element = document.createElement("option");
          element.value = option.value;
          element.textContent = option.label;
          return element;
        }),
      );
    }
    select.value = this.#tools.mirrorValue();
    reflect(select, "aria-label", `Mirror: whose mix ${this.#data.name}'s follows`);
    this.#parts.mirrorNote.hidden = mirror.note === null;
    setText(this.#parts.mirrorNote, mirror.note ?? "");
    reflect(select, "aria-describedby", mirror.note ? "mirror-note" : null);
  }

  // Keeps one checkbox per target, ticked as the tools say, a disabled one with its reason.
  #renderChoices(container, targets) {
    const list = container.dataset.list;
    const same =
      container.children.length === targets.length &&
      targets.every((target, index) => container.children[index].dataset.address === target.address);
    if (!same) {
      container.replaceChildren(
        ...targets.map((target) => {
          const label = document.createElement("label");
          label.className = "choice";
          label.dataset.address = target.address;
          label.innerHTML = `<input type="checkbox" /><span class="text"></span><span class="why"></span>`;
          label.querySelector("input").value = target.address;
          return label;
        }),
      );
    }
    targets.forEach((target, index) => {
      const label = container.children[index];
      const box = label.querySelector("input");
      box.disabled = target.disabled;
      box.checked = this.#tools.isTicked(list, target.address);
      setText(label.querySelector(".text"), target.label);
      setText(label.querySelector(".why"), target.note ? `(${target.note})` : "");
    });
  }

  #renderPresets() {
    const data = this.#data;
    if (!data) {
      return;
    }
    const parts = this.#parts;
    const presets = data.presets;
    parts.presetsIdle.hidden = data.canUse;
    reflect(parts.save, "aria-disabled", data.canUse ? null : "true");
    parts.unread.hidden = presets !== null;
    parts.none.hidden = presets === null || presets.length > 0;
    parts.has.hidden = !presets || presets.length === 0;
    if (!presets || presets.length === 0) {
      return;
    }
    const kept = parts.preset.value;
    const same =
      parts.preset.options.length === presets.length &&
      presets.every(
        (preset, index) =>
          parts.preset.options[index].value === preset.id &&
          parts.preset.options[index].textContent === preset.name,
      );
    if (!same) {
      parts.preset.replaceChildren(
        ...presets.map((preset) => {
          const option = document.createElement("option");
          option.value = preset.id;
          option.textContent = preset.name;
          return option;
        }),
      );
      parts.preset.value = presets.some((preset) => preset.id === kept) ? kept : presets[0].id;
    }
    this.#renderChoices(parts.applyChoices, data.presetTargets);
    const name = this.#selectedName();
    reflect(parts.apply, "aria-disabled", this.#tools.applyReady(parts.preset.value) ? null : "true");
    reflect(parts.apply, "aria-label", `Apply ${name}`);
    reflect(parts.renameTo, "aria-label", `New name for ${name}`);
    this.#renderDelete();
  }

  #selectedName() {
    const id = this.#parts.preset.value;
    return this.#data?.presets?.find((preset) => preset.id === id)?.name ?? "";
  }

  // Opens or closes the panels as the tools say, and puts the focus where they say.
  #focus({ focus }) {
    const open = this.#tools.open();
    for (const opener of this.#parts.openers) {
      reflect(opener, "aria-expanded", String(opener.dataset.panel === open));
    }
    for (const panel of this.#parts.panels) {
      panel.hidden = panel.dataset.for !== open;
    }
    this.#renderDelete();
    if (focus === "panel") {
      this.#parts.panels
        .find((panel) => panel.dataset.for === open)
        ?.querySelector("input, select, button")
        ?.focus();
    } else if (focus) {
      this.#parts.openers.find((opener) => opener.dataset.panel === focus)?.focus();
    }
  }

  #renderDelete() {
    const slot = this.#parts.delete;
    const id = this.#parts.preset.value;
    const name = this.#selectedName();
    const { armed, ready } = this.#tools.deleteState(id);
    if (!armed) {
      if (slot.dataset.state !== "idle") {
        slot.dataset.state = "idle";
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "danger";
        remove.textContent = "Delete preset";
        remove.addEventListener("click", () => {
          this.#tools.arm(this.#parts.preset.value);
          this.#renderDelete();
        });
        slot.replaceChildren(remove);
      }
      reflect(slot.firstElementChild, "aria-label", `Delete the preset ${name}`);
      return;
    }
    if (slot.dataset.state === "armed") {
      reflect(slot.querySelector(".danger"), "aria-disabled", ready ? null : "true");
      return;
    }
    slot.dataset.state = "armed";
    const prompt = document.createElement("span");
    prompt.className = "confirm";
    const keep = document.createElement("button");
    keep.type = "button";
    keep.textContent = "Keep";
    keep.addEventListener("click", () => {
      this.#tools.keep();
      this.#renderDelete();
    });
    const question = document.createElement("span");
    question.textContent = `Delete ${name}? No musician's mix changes.`;
    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.className = "danger";
    confirm.textContent = "Delete";
    reflect(confirm, "aria-disabled", ready ? null : "true");
    confirm.addEventListener("click", () => {
      if (this.#tools.confirmDelete(id)) {
        this.#renderDelete();
      }
    });
    // Keep sits where Delete was, so a second click on the same spot keeps the preset.
    prompt.append(keep, question, confirm);
    slot.replaceChildren(prompt);
    keep.focus();
  }
}

define("ml-mix-tools", MlMixTools);
