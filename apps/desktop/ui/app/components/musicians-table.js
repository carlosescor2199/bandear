// <ml-musicians-table>: the musicians the engine sends to, with their names, links and counts, a
// guarded Remove, and a form to add a phone by address.
//
// Property: `data` = { rows, addError, tableError, selected, bufferSaid, redundancySaid }: rows from
//   musicianRows (network-view.js); the two errors are sentences to show, or null; `selected` the
//   address of the musician whose link history is open, or null; `bufferSaid` and
//   `redundancySaid` `{ address, ok, message }`, what the last buffer or redundancy change of that
//   row did, or null.
// Method: `clearAddress()` empties the address field after a phone was added.
// Events (bubbling, composed): `musician-rename` { address, name } when an editable name is
//   committed, `musician-remove` { address } when a removal is confirmed, `musician-add`
//   { address } when the form is sent, `musician-select` { address } when a musician's History is
//   pressed or their row is clicked outside its fields and buttons (again on the open one closes it).
//   `musician-buffer` { address, key, mode, ms } when a Buffer is chosen: "auto", or "fixed" with
//   the length typed (a string, checked by the screen); see playback-buffer.js.
//   `musician-redundancy` { address, key, on } when Redundancy is switched; see redundancy.js.
//
// A name commits on Enter or when the field loses focus; Escape puts the saved name back. A name
// the phone announced is the phone's, so its field is read-only; a local label is marked as such,
// and its field's title says whether the desk keeps it for the phone (it follows the phone to a new
// IP) or for the address.
// The Buffer cell: Auto or Fixed and, for Fixed, its length in ms (Enter or leaving the field sends
// it, Escape puts the last one back), with the cushion the phone reports under it; a phone that
// takes no buffer from the desk shows "needs a newer app" and no control. A choice waiting for its
// answer stays shown until the row's buffer or its outcome says otherwise.
// The Redundancy cell: a switch (every packet sent again a few blocks later), what it costs in
// bandwidth under it and, while on, the copies sent and the packets the phone reports recovered (or
// that it does not report them); a switch waiting for its answer stays as it was set until the row
// or its outcome says otherwise.
// A field being typed in is never overwritten by new data. Remove asks first: the row shows
// "Keep" where Remove was, and the confirming Remove only works after a short pause, so a double
// click or a stray click cannot remove anyone.

import "./link-state.js";
import { CONFIRM_DELAY_MS, confirmReady } from "../lib/network-view.js";
import {
  DEFAULT_FIXED_MS,
  MAX_BUFFER_MS,
  MIN_BUFFER_MS,
  pendingBufferChoice,
} from "../lib/playback-buffer.js";
import { pendingRedundancyChoice } from "../lib/redundancy.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const CLIENT_NAME_TITLE =
  "Announced by the musician in their own app; the name is set on their phone, not on this desk";
const LOCAL_LABEL_TITLES = Object.freeze({
  phone:
    "Local label for this desk, kept for this phone even when its address changes, and shown " +
    "only while the musician has not announced a name. Enter saves, Escape reverts",
  address:
    "Local label for this desk, kept for this address, and shown only while the musician has " +
    "not announced a name. Enter saves, Escape reverts",
});
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
  tr[data-selected] td {
    background: var(--ml-surface-raised);
  }
  tbody tr {
    cursor: pointer;
  }
  .link-line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .history {
    min-height: 26px;
    padding: 0 8px;
    font-size: var(--ml-text-xs);
  }
  .history[aria-pressed="true"] {
    border-color: var(--ml-accent);
    color: var(--ml-accent);
  }
  .num {
    font-family: var(--ml-font-mono);
    text-align: right;
    white-space: nowrap;
  }
  .warn {
    color: var(--ml-accent);
  }
  .buffer-line {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .buffer-mode,
  .buffer-ms {
    min-height: 26px;
    padding: 0 6px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
    font-size: var(--ml-text-sm);
  }
  .buffer-ms {
    width: 4.5em;
    font-family: var(--ml-font-mono);
  }
  .unit {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-xs);
  }
  .buffer-said {
    max-width: 16em;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-xs);
  }
  .buffer-said[data-error],
  .redundancy-said[data-error] {
    color: var(--ml-danger-text);
  }
  .redundancy-line {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    font-size: var(--ml-text-sm);
  }
  .redundancy-switch {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--ml-blue);
  }
  .redundancy-said {
    max-width: 16em;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-xs);
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
  #data = { rows: [], addError: null, tableError: null, selected: null };
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
              <th scope="col" title="The phone's playback buffer: Auto lets the phone adapt it; Fixed keeps the length set here. Under it, the cushion the phone reports it plays">Buffer</th>
              <th scope="col" title="Redundancy: every packet sent again a few blocks later; it doubles this musician's bandwidth. Under it, the copies sent and the packets the phone reports a copy recovered">Redundancy</th>
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
      <td>
        <div class="link-line">
          <ml-link-state></ml-link-state>
          <button type="button" class="history" aria-pressed="false">History</button>
        </div>
        <div class="detail"></div>
      </td>
      <td class="buffer">
        <div class="buffer-line">
          <select class="buffer-mode">
            <option value="auto">Auto</option>
            <option value="fixed">Fixed</option>
          </select>
          <span class="buffer-fixed">
            <input class="buffer-ms" type="number" min="${MIN_BUFFER_MS}" max="${MAX_BUFFER_MS}"
              step="1" inputmode="numeric" />
            <span class="unit">ms</span>
          </span>
        </div>
        <div class="detail buffer-detail"></div>
        <div class="buffer-said" role="status"></div>
      </td>
      <td class="redundancy">
        <label class="redundancy-line">
          <input class="redundancy-switch" type="checkbox" role="switch" />
          <span class="redundancy-word"></span>
        </label>
        <div class="detail redundancy-cost"></div>
        <div class="detail redundancy-counts"></div>
        <div class="redundancy-said" role="status"></div>
      </td>
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
      history: row.querySelector(".history"),
      detail: row.querySelector(".detail"),
      bufferLine: row.querySelector(".buffer-line"),
      bufferMode: row.querySelector(".buffer-mode"),
      bufferFixed: row.querySelector(".buffer-fixed"),
      bufferMs: row.querySelector(".buffer-ms"),
      bufferDetail: row.querySelector(".buffer-detail"),
      bufferSaid: row.querySelector(".buffer-said"),
      // The buffer the engine reports for the row, the key it is set by, a choice sent and not
      // answered yet, and the outcome last shown.
      buffer: null,
      key: null,
      asked: null,
      said: null,
      redundancySwitch: row.querySelector(".redundancy-switch"),
      redundancyWord: row.querySelector(".redundancy-word"),
      redundancyCost: row.querySelector(".redundancy-cost"),
      redundancyCounts: row.querySelector(".redundancy-counts"),
      redundancySaid: row.querySelector(".redundancy-said"),
      // A redundancy switch sent and not answered yet, and the outcome last shown.
      redundancyAsked: null,
      redundancySeen: null,
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
    view.history.addEventListener("click", () => emit(this, "musician-select", { address }));
    view.bufferMode.addEventListener("change", () => {
      const mode = view.bufferMode.value;
      view.bufferFixed.hidden = mode !== "fixed";
      this.#askBuffer(view, mode);
    });
    view.bufferMs.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        view.bufferMs.blur();
      } else if (event.key === "Escape") {
        view.bufferMs.value = String(view.buffer?.ms ?? DEFAULT_FIXED_MS);
        view.bufferMs.blur();
      }
    });
    view.bufferMs.addEventListener("change", () => {
      if (view.bufferMode.value === "fixed") {
        this.#askBuffer(view, "fixed");
      }
    });
    view.redundancySwitch.addEventListener("change", () => {
      const on = view.redundancySwitch.checked;
      view.redundancyAsked = { on };
      emit(this, "musician-redundancy", { address, key: view.key, on });
    });
    row.addEventListener("click", (event) => {
      // A click on the row's own fields and buttons does what they do; anywhere else opens the
      // musician's link history, as History does.
      if (!event.composedPath().some((node) => node.matches?.("input, button, select, label, a"))) {
        emit(this, "musician-select", { address });
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
    view.name.title = row.editable
      ? (LOCAL_LABEL_TITLES[row.labelFollows] ?? LOCAL_LABEL_TITLES.address)
      : CLIENT_NAME_TITLE;
    reflect(view.name, "aria-label", `Name of ${row.name || row.address}`);
    view.badge.hidden = !row.nameIsLocal;
    setText(view.addressText, row.address);
    reflect(view.dot, "data-on", row.connected);
    setText(view.control, row.connected ? "connected" : "offline");
    reflect(view.link, "level", row.link.level);
    reflect(view.link, "label", row.link.label);
    reflect(view.link, "detail", row.link.detail);
    setText(view.detail, row.link.detail);
    this.#updateBuffer(view, row);
    this.#updateRedundancy(view, row);
    setText(view.mix, row.mix);
    setText(view.sent, row.sent);
    setText(view.discarded, row.discarded);
    setText(view.loss, row.loss);
    view.loss.classList.toggle("warn", row.lossHigh);
    view.label = row.name || row.address;
    const selected = this.#data.selected === row.address;
    reflect(view.row, "data-selected", selected);
    reflect(view.history, "aria-pressed", selected ? "true" : "false");
    reflect(view.history, "aria-label", `Link history of ${view.label}`);
    this.#renderActions(view);
  }

  // Sends the Buffer chosen in `view`'s row: Auto, or Fixed with the length in its field.
  #askBuffer(view, mode) {
    const ms = mode === "fixed" ? view.bufferMs.value : null;
    view.asked = { mode, ms };
    emit(this, "musician-buffer", { address: view.address, key: view.key, mode, ms });
  }

  #updateBuffer(view, row) {
    const { buffer } = row;
    view.buffer = buffer;
    view.key = buffer.key;
    view.bufferLine.hidden = !buffer.supported;
    const said = this.#data.bufferSaid?.address === row.address ? this.#data.bufferSaid : null;
    // A choice is shown until the engine reports it, or until its outcome came back.
    view.asked = pendingBufferChoice(view.asked, buffer, said, view.said);
    view.said = said;
    view.bufferMode.value = view.asked?.mode ?? buffer.mode;
    view.bufferFixed.hidden = view.bufferMode.value !== "fixed";
    if (this.shadowRoot.activeElement !== view.bufferMs && view.asked === null) {
      view.bufferMs.value = String(buffer.ms ?? (view.bufferMs.value || DEFAULT_FIXED_MS));
    }
    const label = row.name || row.address;
    reflect(view.bufferMode, "aria-label", `Playback buffer of ${label}`);
    reflect(view.bufferMs, "aria-label", `Fixed playback buffer of ${label}, in milliseconds`);
    setText(view.bufferDetail, buffer.supported ? (buffer.playing ?? "") : buffer.note);
    view.bufferDetail.title = buffer.supported
      ? "The cushion the phone reports it plays"
      : "This phone's app does not take its playback buffer from the desk; update MixLink on it";
    setText(view.bufferSaid, said?.message ?? "");
    reflect(view.bufferSaid, "data-error", said ? !said.ok : false);
  }

  #updateRedundancy(view, row) {
    const cell = row.redundancy;
    const said = this.#data.redundancySaid?.address === row.address ? this.#data.redundancySaid : null;
    // A switch is shown as set until the engine reports it, or until its outcome came back.
    view.redundancyAsked = pendingRedundancyChoice(view.redundancyAsked, cell, said, view.redundancySeen);
    view.redundancySeen = said;
    const on = view.redundancyAsked?.on ?? cell.on;
    view.redundancySwitch.checked = on;
    setText(view.redundancyWord, on ? "On" : "Off");
    const label = row.name || row.address;
    reflect(view.redundancySwitch, "aria-label", `Send ${label}'s audio twice (redundancy)`);
    setText(view.redundancyCost, cell.bandwidth);
    view.redundancyCost.title =
      "Every packet is sent again a few blocks later, so a Wi-Fi burst must take both copies to cost audio";
    setText(view.redundancyCounts, cell.on ? [cell.duplicates, cell.recovered].join(" · ") : "");
    setText(view.redundancySaid, said?.message ?? "");
    reflect(view.redundancySaid, "data-error", said ? !said.ok : false);
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
