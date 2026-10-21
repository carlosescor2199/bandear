// <ml-channel-table>: the device's inputs with the engineer's details: name, instrument, live
// level, stereo pair, group, whether musicians see the input and a link to its processing (a
// pair's opens on its first side), with whether the processing is on in words.
//
// Property: `data` = { rows, kinds }: rows from channelRows and kinds KIND_OPTIONS
//   (channels-view.js), each with `processing` (processingActive: true while its strip does
//   something, null while the strips are not read yet).
// Method: `setLevels(levels)`, the input peaks of a `meters` event, one per input.
// Events (bubbling, composed): `channel-rename` { index, name } when a name is committed,
//   `channel-kind` { index, kind }, `channel-link` { index, linked } and `channel-shown`
//   { index, shown }. The table does not change its data itself: the screen answers with new data.
//
// A name commits on Enter or when its field loses focus; Escape puts the saved name back; a field
// being typed in is never overwritten by new data. A stereo toggle that cannot be used stays
// focusable, says why beside it and is announced with the reason. Hiding one side of a pair hides
// both, and the Shown box says so. State is written out ("Linked with CH 2", "Hidden"), never shown
// by colour alone.

import "./group-chip.js";
import "./meter.js";
import { kindIcon, stereoIcon } from "./kind-icons.js";
import { processingLink } from "../lib/processing-view.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const NAME_TITLE = "Shown on every phone and on the Mix. Enter saves, Escape reverts";

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
  thead th {
    padding: 12px 8px;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-xs);
    font-weight: var(--ml-weight-medium);
    letter-spacing: 0.12em;
    text-align: left;
    text-transform: uppercase;
    white-space: nowrap;
  }
  tbody td,
  tbody th {
    padding: 8px;
    border-top: 1px solid var(--ml-border-subtle);
    vertical-align: middle;
  }
  tr > :first-child {
    padding-left: 14px;
  }
  tr > :last-child {
    padding-right: 14px;
  }
  .input {
    color: var(--ml-text-secondary);
    font-family: var(--ml-font-mono);
    font-weight: var(--ml-weight-regular);
    text-align: left;
    white-space: nowrap;
  }
  tr[data-hidden] .input {
    box-shadow: inset 3px 0 0 var(--ml-text-muted);
  }
  input,
  select,
  button {
    font: inherit;
    color: inherit;
  }
  .name,
  select {
    min-height: var(--ml-target);
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
    font-size: var(--ml-text-md);
  }
  .name {
    width: 11em;
    padding: 0 8px;
  }
  .name::placeholder {
    color: var(--ml-text-muted);
  }
  select {
    padding: 0 6px;
  }
  .kind {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ml-text-secondary);
  }
  .level {
    min-width: 110px;
  }
  .stereo {
    white-space: nowrap;
  }
  .stereo-line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: var(--ml-target);
    min-height: var(--ml-target);
    padding: 0 10px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-surface-raised);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  .link[aria-pressed="true"] {
    border-color: var(--ml-text);
    background: var(--ml-text);
    color: var(--ml-text-on-bright);
  }
  .link[aria-disabled="true"] {
    border-style: dashed;
    background: transparent;
    color: var(--ml-text-muted);
    cursor: default;
  }
  .stereo-text[data-linked] {
    color: var(--ml-text);
  }
  .stereo-text,
  .reason {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .reason {
    max-width: 22em;
    margin: 4px 0 0;
    white-space: normal;
  }
  .shown {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: var(--ml-target);
    cursor: pointer;
  }
  .shown input {
    width: 18px;
    height: 18px;
    margin: 0;
    accent-color: var(--ml-accent);
  }
  .shown-word[data-hidden] {
    color: var(--ml-text-muted);
  }
  .processing {
    white-space: nowrap;
  }
  .edit-processing {
    display: inline-flex;
    align-items: center;
    min-height: var(--ml-target);
    padding: 0 10px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-surface-raised);
    color: var(--ml-text);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    text-decoration: none;
  }
  .edit-processing:hover {
    border-color: var(--ml-text-muted);
  }
  .processing-word {
    margin-left: 8px;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .processing-word[data-on] {
    color: var(--ml-text);
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  [hidden] {
    display: none !important;
  }
`);

class MlChannelTable extends HTMLElement {
  #data = { rows: [], kinds: [] };
  #body;
  #views = [];

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<div class="wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">Input</th>
              <th scope="col">Name</th>
              <th scope="col">Instrument</th>
              <th scope="col">Level</th>
              <th scope="col">Stereo</th>
              <th scope="col">Group</th>
              <th scope="col" title="A hidden input is shown nowhere and no one hears it">Shown</th>
              <th scope="col">Processing</th>
            </tr>
          </thead>
          <tbody></tbody>
        </table>
      </div>`,
    );
    this.#body = root.querySelector("tbody");
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    if (this.#views.length !== data.rows.length) {
      // Only a change of the number of inputs rebuilds the rows, so a name being typed keeps its
      // field.
      this.#views = data.rows.map((row) => this.#newRow(row.index));
      this.#body.replaceChildren(...this.#views.map((view) => view.row));
    }
    data.rows.forEach((row, position) => this.#update(this.#views[position], row));
  }

  setLevels(levels) {
    this.#views.forEach((view, index) => view.meter.setLevel(levels[index] ?? 0));
  }

  #newRow(index) {
    const row = document.createElement("tr");
    row.innerHTML = `
      <th scope="row" class="input"></th>
      <td><input class="name" type="text" maxlength="32" autocomplete="off" spellcheck="false" /></td>
      <td><span class="kind"><span class="kind-icon"></span><select></select></span></td>
      <td class="level"><ml-meter orientation="horizontal"></ml-meter></td>
      <td class="stereo">
        <div class="stereo-line">
          <button type="button" class="link">${stereoIcon()}<span>Stereo</span></button>
          <span class="stereo-text"></span>
        </div>
        <p class="reason" id="reason-${index}" hidden></p>
      </td>
      <td><ml-group-chip></ml-group-chip></td>
      <td><label class="shown"><input type="checkbox" /><span class="shown-word"></span></label></td>
      <td class="processing"><a class="edit-processing">Processing</a><span class="processing-word"></span></td>`;
    const view = {
      row,
      index,
      input: row.querySelector(".input"),
      name: row.querySelector(".name"),
      kindIcon: row.querySelector(".kind-icon"),
      kind: row.querySelector("select"),
      meter: row.querySelector("ml-meter"),
      link: row.querySelector(".link"),
      stereoText: row.querySelector(".stereo-text"),
      reason: row.querySelector(".reason"),
      chip: row.querySelector("ml-group-chip"),
      shown: row.querySelector(".shown input"),
      shownWord: row.querySelector(".shown-word"),
      processing: row.querySelector(".edit-processing"),
      processingWord: row.querySelector(".processing-word"),
      saved: "",
      shownKind: null,
      stereo: null,
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
      if (view.name.value.trim() !== view.saved) {
        emit(this, "channel-rename", { index, name: view.name.value.trim() });
      }
    });
    view.kind.addEventListener("change", () => {
      emit(this, "channel-kind", { index, kind: view.kind.value });
    });
    view.link.addEventListener("click", () => {
      if (view.stereo?.available) {
        emit(this, "channel-link", { index, linked: !view.stereo.linked });
      }
    });
    view.shown.addEventListener("change", () => {
      emit(this, "channel-shown", { index, shown: view.shown.checked });
    });
    return view;
  }

  #update(view, row) {
    const typing = this.shadowRoot.activeElement === view.name;
    setText(view.input, row.input);
    view.saved = row.name;
    if (!typing) {
      view.name.value = row.name;
    }
    view.name.placeholder = row.input;
    reflect(view.name, "aria-label", `Name of ${row.input}`);
    view.name.title = NAME_TITLE;

    if (view.kind.options.length !== this.#data.kinds.length) {
      view.kind.replaceChildren(
        ...this.#data.kinds.map(({ value, label }) => new Option(label, value)),
      );
    }
    view.kind.value = row.kind;
    reflect(view.kind, "aria-label", `Instrument of ${row.label}`);
    if (view.shownKind !== row.kind) {
      view.shownKind = row.kind;
      view.kindIcon.innerHTML = kindIcon(row.kind);
    }

    reflect(view.meter, "label", `${row.label} input level`);

    const { stereo } = row;
    view.stereo = stereo;
    reflect(view.link, "aria-pressed", String(stereo.linked));
    reflect(view.link, "aria-label", stereo.label);
    reflect(view.link, "aria-disabled", stereo.available ? null : "true");
    reflect(view.link, "aria-describedby", stereo.reason ? view.reason.id : null);
    view.link.title =
      stereo.reason ??
      (stereo.linked
        ? "Undo the pair: two strips again"
        : "Pair with the next input: one strip whose pan is a balance");
    setText(view.stereoText, stereo.text);
    reflect(view.stereoText, "data-linked", stereo.linked);
    view.reason.hidden = !stereo.reason;
    setText(view.reason, stereo.reason ?? "");

    reflect(view.chip, "name", row.group?.name ?? null);
    reflect(view.chip, "tone", row.group?.tone ?? "none");
    reflect(view.chip, "invalid", row.group?.invalid ?? false);

    view.shown.checked = !row.hidden;
    const together = stereo.linked ? ` and CH ${stereo.partner + 1}, its pair` : "";
    reflect(view.shown, "aria-label", `Show ${row.input}${together} to musicians`);
    view.shown.title = stereo.linked
      ? "Both inputs of a pair are shown or hidden together"
      : "A hidden input is shown nowhere and no one hears it";
    setText(view.shownWord, row.hidden ? "Hidden" : "Shown");
    reflect(view.shownWord, "data-hidden", row.hidden);
    reflect(view.row, "data-hidden", row.hidden);

    // A pair is one strip: its processing opens on its first side.
    const sides = stereo.linked ? [stereo.first, stereo.partner] : [row.index];
    const link = processingLink({ sides, label: row.label, active: row.processing ?? null });
    reflect(view.processing, "href", link.href);
    reflect(view.processing, "aria-label", link.ariaLabel);
    view.processing.title = "Open this input's high-pass, equaliser, gate and compressor";
    setText(view.processingWord, link.word);
    reflect(view.processingWord, "data-on", link.state === "on");
  }
}

define("ml-channel-table", MlChannelTable);
