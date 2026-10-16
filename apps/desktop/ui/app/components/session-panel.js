// <ml-session-panel>: the engineer's named sessions, in the Network and system screen's aside:
// "Save session as…", the saved sessions with Load, Rename and Delete, what the desk said when it
// opened its session, and the outcome of the last command in words.
//
// Property: `data`, sessionPanelView (sessions.js) and `unsaved`, a function answering how many
//   edits a load would drop now (the console's unsavedEdits).
// Methods: `say({ ok, message })` shows a command's outcome (a status line, or an alert when it
//   failed); `answered(kind, ok)` after the screen's command for `kind` ("save", "load", "rename",
//   "delete") was answered (see sessionPanelCommands): a done save or rename empties its field and a
//   load asked about is no longer asked.
// Events (bubbling, composed): `session-save` { name }, `session-load` { id }, `session-rename`
//   { id, name }, `session-delete` { id } once a deletion is confirmed, `sessions-load` {} to list
//   them again. The panel changes no data itself: the screen answers with new data.
//
// Load asks first while edits are still on their way, since loading drops them ("Load anyway" or
// "Cancel"). Delete asks first as a preset's does: "Keep" takes Delete's place and the confirming
// Delete only works after a short pause. The rules live in createSessionPanel (sessions.js),
// tested without a browser; this element renders what it answers.

import { createSessionPanel } from "../lib/sessions.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    display: block;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-surface);
  }
  h2 {
    margin: 0;
    font-size: var(--ml-text-lg);
  }
  label {
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-md);
  }
  input,
  select,
  button {
    min-height: var(--ml-target);
    font: inherit;
    font-size: var(--ml-text-md);
    color: inherit;
  }
  input,
  select {
    min-width: 0;
    padding: 0 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
  }
  button {
    padding: 0 12px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-surface-raised);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  button[aria-disabled="true"] {
    cursor: default;
    opacity: 0.5;
  }
  .primary {
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  .danger {
    border-color: var(--ml-danger-border);
    background: var(--ml-danger-bg);
    color: var(--ml-danger-text);
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px 8px;
    align-items: center;
  }
  .field label {
    grid-column: 1 / -1;
  }
  .actions,
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .notice,
  .asking {
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--ml-warn-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-warn-bg);
    font-size: var(--ml-text-md);
  }
  .asking {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .asking p {
    margin: 0;
  }
  .detail,
  .hint,
  .empty {
    margin: 0;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .outcome {
    margin: 0;
    font-size: var(--ml-text-md);
  }
  .outcome--error {
    color: var(--ml-danger-text);
  }
  hr {
    width: 100%;
    margin: 2px 0;
    border: 0;
    border-top: 1px solid var(--ml-border-subtle);
  }
  [hidden] {
    display: none !important;
  }
`);

class MlSessionPanel extends HTMLElement {
  #data = null;
  #parts;
  #panel = createSessionPanel({
    emit: (type, detail) => emit(this, type, detail),
    // A delete that became confirmable, or disarmed itself.
    changed: () => this.#renderDelete(),
  });

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<section aria-labelledby="session-title">
        <h2 id="session-title">Session</h2>
        <p class="notice" role="status" data-notice hidden></p>
        <div class="field">
          <label for="save-name">Save session as</label>
          <input id="save-name" type="text" autocomplete="off" spellcheck="false" maxlength="40" />
          <button type="button" data-save>Save</button>
        </div>
        <p class="hint" data-idle hidden>The engine is not running: the session is saved without the musicians' mixes.</p>
        <hr />
        <p class="empty" data-unread hidden><span data-unread-text></span> <button type="button" data-list>List them again</button></p>
        <p class="empty" data-none hidden>No saved sessions yet.</p>
        <div data-has hidden>
          <div class="field">
            <label for="saved">Saved sessions</label>
            <select id="saved"></select>
            <button type="button" class="primary" data-load aria-describedby="saved-detail">Load</button>
          </div>
          <p class="detail" id="saved-detail" data-detail></p>
          <div class="asking" role="alert" data-asking hidden>
            <p data-asking-text></p>
            <div class="actions">
              <button type="button" data-cancel>Cancel</button>
              <button type="button" class="danger" data-anyway>Load anyway</button>
            </div>
          </div>
          <div class="field">
            <label for="rename-to">Rename to</label>
            <input id="rename-to" type="text" autocomplete="off" spellcheck="false" maxlength="40" />
            <button type="button" data-rename>Rename</button>
          </div>
          <div class="actions"><span class="delete"></span></div>
        </div>
        <p class="hint">
          Loading replaces the channels, groups, presets, labels, limits, locks and mirrors, and
          sends each saved mix to the musicians who are connected. A phone that is not connected
          keeps its own mix. The saved audio input is used from the next start.
        </p>
        <p class="outcome" role="status" data-said></p>
        <p class="outcome outcome--error" role="alert" data-failed></p>
      </section>`,
    );
    const $ = (selector) => root.querySelector(selector);
    this.#parts = {
      notice: $("[data-notice]"),
      saveName: $("#save-name"),
      save: $("[data-save]"),
      idle: $("[data-idle]"),
      unread: $("[data-unread]"),
      unreadText: $("[data-unread-text]"),
      none: $("[data-none]"),
      has: $("[data-has]"),
      saved: $("#saved"),
      load: $("[data-load]"),
      detail: $("[data-detail]"),
      asking: $("[data-asking]"),
      askingText: $("[data-asking-text]"),
      cancel: $("[data-cancel]"),
      anyway: $("[data-anyway]"),
      renameTo: $("#rename-to"),
      rename: $("[data-rename]"),
      delete: $(".delete"),
      said: $("[data-said]"),
      failed: $("[data-failed]"),
    };
    const parts = this.#parts;
    const panel = this.#panel;
    const save = () => panel.save(parts.saveName.value);
    parts.save.addEventListener("click", save);
    parts.saveName.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        save();
      }
    });
    $("[data-list]").addEventListener("click", () => emit(this, "sessions-load", {}));
    parts.saved.addEventListener("change", () => {
      panel.choose(parts.saved.value);
      this.#render();
    });
    parts.load.addEventListener("click", () => {
      panel.load(this.#data?.unsaved?.() ?? 0);
      this.#render();
      if (panel.asking()) {
        parts.cancel.focus();
      }
    });
    parts.cancel.addEventListener("click", () => {
      panel.cancelLoad();
      this.#render();
      parts.load.focus();
    });
    parts.anyway.addEventListener("click", () => {
      panel.confirmLoad();
      this.#render();
    });
    parts.asking.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        panel.cancelLoad();
        this.#render();
        parts.load.focus();
      }
    });
    const rename = () => panel.rename(parts.renameTo.value);
    parts.rename.addEventListener("click", rename);
    parts.renameTo.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        rename();
      }
    });
  }

  disconnectedCallback() {
    this.#panel.dispose();
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    this.#panel.setRows(data.rows);
    this.#render();
  }

  /** Shows a command's outcome in words: `{ ok, message }`. */
  say({ ok, message }) {
    setText(this.#parts.said, ok ? message : "");
    setText(this.#parts.failed, ok ? "" : message);
  }

  /** The screen's command for `kind` was answered (see createSessionPanel's `answered`). */
  answered(kind, ok) {
    const { clearSave, clearRename } = this.#panel.answered(kind, ok);
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
    const data = this.#data;
    if (!data) {
      return;
    }
    const parts = this.#parts;
    const panel = this.#panel;
    parts.notice.hidden = data.notice === null;
    setText(parts.notice, data.notice ?? "");
    parts.idle.hidden = data.engineRunning;
    parts.unread.hidden = data.unread === null;
    setText(
      parts.unreadText,
      data.unread ? `The saved sessions could not be listed: ${data.unread}.` : "Listing the saved sessions…",
    );
    parts.none.hidden = data.unread !== null || data.rows.length > 0;
    parts.has.hidden = data.rows.length === 0;
    if (data.rows.length === 0) {
      return;
    }
    const key = data.rows.map((row) => `${row.id}\u0000${row.label}`).join("\u0001");
    if (parts.saved.dataset.options !== key) {
      parts.saved.dataset.options = key;
      parts.saved.replaceChildren(
        ...data.rows.map((row) => {
          const option = document.createElement("option");
          option.value = row.id;
          option.textContent = row.label;
          return option;
        }),
      );
    }
    parts.saved.value = panel.chosen() ?? "";
    const chosen = data.rows.find((row) => row.id === panel.chosen());
    setText(parts.detail, chosen?.detail ?? "");
    reflect(parts.load, "aria-disabled", panel.loadReady() ? null : "true");
    reflect(parts.load, "aria-label", `Load ${chosen?.name ?? ""}`);
    const asking = panel.asking();
    parts.asking.hidden = asking === null;
    if (asking) {
      const edits = asking.unsaved === 1 ? "1 change is" : `${asking.unsaved} changes are`;
      setText(
        parts.askingText,
        `${edits} still on the way to the engine. Loading "${asking.name}" drops them.`,
      );
    }
    reflect(parts.rename, "aria-disabled", chosen?.loadable ? null : "true");
    reflect(parts.renameTo, "aria-label", `New name for ${chosen?.name ?? ""}`);
    this.#renderDelete();
  }

  #renderDelete() {
    const slot = this.#parts.delete;
    const name = this.#data?.rows.find((row) => row.id === this.#panel.chosen())?.name ?? "";
    const { armed, ready } = this.#panel.deleteState();
    if (!armed) {
      if (slot.dataset.state !== "idle") {
        slot.dataset.state = "idle";
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "danger";
        remove.textContent = "Delete";
        remove.addEventListener("click", () => {
          this.#panel.arm();
          this.#renderDelete();
        });
        slot.replaceChildren(remove);
      }
      reflect(slot.firstElementChild, "aria-label", `Delete the saved session ${name}`);
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
      this.#panel.keep();
      this.#renderDelete();
    });
    keep.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        this.#panel.keep();
        this.#renderDelete();
      }
    });
    const question = document.createElement("span");
    question.textContent = `Delete ${name}? The desk's session does not change.`;
    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.className = "danger";
    confirm.textContent = "Delete";
    reflect(confirm, "aria-disabled", ready ? null : "true");
    confirm.addEventListener("click", () => {
      if (this.#panel.confirmDelete()) {
        this.#renderDelete();
      }
    });
    // Keep sits where Delete was, so a second click on the same spot keeps the session.
    prompt.append(keep, question, confirm);
    slot.replaceChildren(prompt);
    keep.focus();
  }
}

define("ml-session-panel", MlSessionPanel);
