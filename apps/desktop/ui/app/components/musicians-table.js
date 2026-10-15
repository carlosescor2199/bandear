// <ml-musicians-table>: the musicians the engine sends to, with their names, links and counts, a
// guarded Remove, and a form to add a phone by address.
//
// Property: `data` = { rows, addError, tableError }: rows from musicianRows (network-view.js);
//   the two errors are sentences to show, or null.
// Method: `clearAddress()` empties the address field after a phone was added.
// Events (bubbling, composed): `musician-rename` { address, name } when an editable name is
//   committed, `musician-remove` { address } when a removal is confirmed, `musician-add`
//   { address } when the form is sent.
//
// A name commits on Enter or when the field loses focus; Escape puts the saved name back. A name
// the phone announced is the phone's, so its field is read-only; a local label is marked as such.
// A field being typed in is never overwritten by new data. Remove asks first: the row shows
// "Keep" where Remove was, and the confirming Remove only works after a short pause, so a double
// click or a stray click cannot remove anyone.

import "./link-state.js";
import { CONFIRM_DELAY_MS, confirmReady } from "../lib/network-view.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const CLIENT_NAME_TITLE =
  "Announced by the musician in their own app; the name is set on their phone, not on this desk";
const LOCAL_LABEL_TITLE =
  "Local label for this desk, shown only while the musician has not announced a name. " +
  "Enter saves, Escape reverts";
const ARMED_FOR_MS = 10000;

const styles = sheet(`
  :host {
    display: block;
  }
  .wrap {
    overflow-x: auto;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-surface);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--ml-text-md);
  }
  th {
    padding: 12px 8px;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-xs);
    font-weight: var(--ml-weight-medium);
    letter-spacing: 0.12em;
    text-align: left;
    text-transform: uppercase;
    white-space: nowrap;
  }
  th:first-child,
  td:first-child {
    padding-left: 14px;
  }
  td {
    padding: 10px 8px;
    border-top: 1px solid var(--ml-border-subtle);
    vertical-align: middle;
  }
  .num {
    font-family: var(--ml-font-mono);
    text-align: right;
    white-space: nowrap;
  }
  .warn {
    color: var(--ml-accent);
  }
  .name-line {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  input,
  button {
    font: inherit;
    color: inherit;
  }
  .name {
    width: 14em;
    min-height: var(--ml-target);
    padding: 0 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
    font-weight: var(--ml-weight-bold);
  }
  .name:disabled {
    border-color: transparent;
    background: transparent;
  }
  .badge {
    padding: 1px 6px;
    border: 1px dashed var(--ml-text-muted);
    border-radius: var(--ml-radius-sm);
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-xs);
    white-space: nowrap;
  }
  .address,
  .detail {
    color: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-xs);
  }
  .control {
    white-space: nowrap;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 6px;
    border: 2px solid var(--ml-text-muted);
    border-radius: 50%;
  }
  .dot[data-on] {
    border-color: var(--ml-green);
    background: var(--ml-green);
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .confirm {
    display: inline-flex;
    align-items: center;
    gap: 6px;
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
  .danger {
    border-color: var(--ml-danger-border);
    background: var(--ml-danger-bg);
    color: var(--ml-danger-text);
  }
  button[aria-disabled="true"] {
    cursor: default;
    opacity: 0.5;
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .empty,
  .hint {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-md);
  }
  .empty {
    padding: 14px;
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  label {
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-md);
  }
  .address-field {
    width: 16em;
    min-height: var(--ml-target-primary);
    padding: 0 10px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-ground);
    font-family: var(--ml-font-mono);
  }
  .add {
    min-height: var(--ml-target-primary);
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  .error {
    width: 100%;
    margin: 0;
    color: var(--ml-danger-text);
    font-size: var(--ml-text-md);
  }
  .hint {
    margin: 8px 0 0;
  }
  [hidden] {
    display: none !important;
  }
`);

class MlMusiciansTable extends HTMLElement {
  #data = { rows: [], addError: null, tableError: null };
  #parts;
  #rows = new Map();
  #armed = null;
  #timers = [];

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<p class="error" role="alert" data-table-error hidden></p>
      <div class="wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">Musician</th>
              <th scope="col">Control</th>
              <th scope="col" title="What the musician's phone reports about the audio it receives">Link</th>
              <th scope="col">Mix</th>
              <th scope="col" class="num">Sent</th>
              <th scope="col" class="num">Discarded</th>
              <th scope="col" class="num" title="Packets this computer discarded for the phone">Loss</th>
              <th scope="col"><span hidden>Actions</span></th>
            </tr>
          </thead>
          <tbody></tbody>
        </table>
        <p class="empty" data-empty>No musicians yet. A phone that finds this computer appears here on its own.</p>
      </div>
      <form novalidate>
        <label for="address">Add a phone by address</label>
        <input id="address" class="address-field" type="text" placeholder="192.168.1.50:50000"
          autocomplete="off" spellcheck="false" />
        <button type="submit" class="add">Add phone</button>
        <p class="error" role="alert" data-add-error hidden></p>
      </form>
      <p class="hint">
        For a fixed rack or for debugging. Adding or removing a phone never restarts the engine:
        the capture, the control server and the discovery beacon keep running. Removing stops that
        phone's audio at once; its control channel stays open and its next mix is answered with
        "no UDP target configured for client IP".
      </p>`,
    );
    this.#parts = {
      body: root.querySelector("tbody"),
      empty: root.querySelector("[data-empty]"),
      tableError: root.querySelector("[data-table-error]"),
      addError: root.querySelector("[data-add-error]"),
      address: root.querySelector(".address-field"),
      form: root.querySelector("form"),
    };
    this.#parts.form.addEventListener("submit", (event) => {
      event.preventDefault();
      emit(this, "musician-add", { address: this.#parts.address.value });
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
    parts.tableError.hidden = !data.tableError;
    setText(parts.tableError, data.tableError ?? "");
    parts.addError.hidden = !data.addError;
    setText(parts.addError, data.addError ?? "");
    reflect(parts.address, "aria-invalid", data.addError ? "true" : null);
    parts.empty.hidden = data.rows.length > 0;
    const addresses = data.rows.map((row) => row.address);
    const same =
      addresses.length === this.#rows.size && addresses.every((address) => this.#rows.has(address));
    if (!same) {
      // Only a change of who is listed rebuilds the rows, so a name being typed keeps its field.
      this.#rows.clear();
      if (this.#armed && !addresses.includes(this.#armed.address)) {
        this.#disarm();
      }
      parts.body.replaceChildren(
        ...data.rows.map((row) => {
          const view = this.#newRow(row.address);
          this.#rows.set(row.address, view);
          return view.row;
        }),
      );
    }
    for (const row of data.rows) {
      this.#update(this.#rows.get(row.address), row);
    }
  }

  clearAddress() {
    this.#parts.address.value = "";
  }

  #newRow(address) {
    const row = document.createElement("tr");
    row.innerHTML = `
      <td>
        <div class="name-line">
          <input class="name" type="text" placeholder="unnamed" />
          <span class="badge" hidden>local label</span>
        </div>
        <div class="address"></div>
      </td>
      <td class="control"><span class="dot" aria-hidden="true"></span><span class="control-word"></span></td>
      <td><ml-link-state></ml-link-state><div class="detail"></div></td>
      <td class="mix"></td>
      <td class="num sent"></td>
      <td class="num discarded"></td>
      <td class="num loss"></td>
      <td class="actions"></td>`;
    const view = {
      row,
      address,
      name: row.querySelector(".name"),
      badge: row.querySelector(".badge"),
      addressText: row.querySelector(".address"),
      dot: row.querySelector(".dot"),
      control: row.querySelector(".control-word"),
      link: row.querySelector("ml-link-state"),
      detail: row.querySelector(".detail"),
      mix: row.querySelector(".mix"),
      sent: row.querySelector(".sent"),
      discarded: row.querySelector(".discarded"),
      loss: row.querySelector(".loss"),
      actions: row.querySelector(".actions"),
      saved: "",
    };
    view.name.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        view.name.blur();
      } else if (event.key === "Escape") {
        view.name.value = view.saved;
        view.name.blur();
      }
    });
    view.name.addEventListener("change", () => {
      // A field turned read-only (the phone announced a name meanwhile) commits nothing.
      if (!view.name.disabled && view.name.value.trim() !== view.saved) {
        emit(this, "musician-rename", { address, name: view.name.value.trim() });
      }
    });
    row.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.#armed?.address === address) {
        this.#disarm();
        this.#renderActions(view);
      }
    });
    this.#renderActions(view);
    return view;
  }

  #update(view, row) {
    const typing = this.shadowRoot.activeElement === view.name;
    view.saved = row.name;
    if (!typing || !row.editable) {
      view.name.value = row.name;
    }
    view.name.disabled = !row.editable;
    view.name.title = row.editable ? LOCAL_LABEL_TITLE : CLIENT_NAME_TITLE;
    reflect(view.name, "aria-label", `Name of ${row.name || row.address}`);
    view.badge.hidden = !row.nameIsLocal;
    setText(view.addressText, row.address);
    reflect(view.dot, "data-on", row.connected);
    setText(view.control, row.connected ? "connected" : "offline");
    reflect(view.link, "level", row.link.level);
    reflect(view.link, "label", row.link.label);
    reflect(view.link, "detail", row.link.detail);
    setText(view.detail, row.link.detail);
    setText(view.mix, row.mix);
    setText(view.sent, row.sent);
    setText(view.discarded, row.discarded);
    setText(view.loss, row.loss);
    view.loss.classList.toggle("warn", row.lossHigh);
    view.label = row.name || row.address;
    this.#renderActions(view);
  }

  #renderActions(view) {
    const armed = this.#armed?.address === view.address;
    const shown = view.actions.dataset.state;
    if (!armed && shown === "idle") {
      reflect(view.actions.firstElementChild, "aria-label", `Remove ${view.label ?? view.address}`);
      return;
    }
    if (!armed) {
      view.actions.dataset.state = "idle";
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Remove";
      remove.setAttribute("aria-label", `Remove ${view.label ?? view.address}`);
      remove.addEventListener("click", () => this.#arm(view));
      view.actions.replaceChildren(remove);
      return;
    }
    const ready = confirmReady(this.#armed.at, performance.now());
    if (shown === "armed") {
      reflect(view.actions.querySelector(".danger"), "aria-disabled", ready ? null : "true");
      return;
    }
    view.actions.dataset.state = "armed";
    const prompt = document.createElement("span");
    prompt.className = "confirm";
    const question = document.createElement("span");
    question.textContent = `Remove ${view.label ?? view.address}?`;
    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.className = "danger";
    confirm.textContent = "Remove";
    reflect(confirm, "aria-disabled", ready ? null : "true");
    confirm.addEventListener("click", () => {
      if (this.#armed?.address === view.address && confirmReady(this.#armed.at, performance.now())) {
        this.#disarm();
        emit(this, "musician-remove", { address: view.address });
        this.#renderActions(view);
      }
    });
    const keep = document.createElement("button");
    keep.type = "button";
    keep.textContent = "Keep";
    keep.addEventListener("click", () => {
      this.#disarm();
      this.#renderActions(view);
    });
    // Keep sits where Remove was, so a second click on the same spot keeps the musician.
    prompt.append(question, confirm, keep);
    view.actions.replaceChildren(prompt);
    keep.focus();
  }

  #arm(view) {
    this.#disarm();
    this.#armed = { address: view.address, at: performance.now() };
    this.#timers.push(
      setTimeout(() => this.#renderActions(view), CONFIRM_DELAY_MS),
      setTimeout(() => {
        if (this.#armed?.address === view.address) {
          this.#disarm();
          this.#renderActions(view);
        }
      }, ARMED_FOR_MS),
    );
    this.#renderActions(view);
  }

  #disarm() {
    this.#armed = null;
    for (const timer of this.#timers.splice(0)) {
      clearTimeout(timer);
    }
  }
}

define("ml-musicians-table", MlMusiciansTable);
