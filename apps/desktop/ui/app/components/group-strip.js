// <ml-group-strip>: one configured group's level in a musician's mix. While the mix follows
// another musician's, the strip says so in words and its fader takes no move.
//
// Property: `data`, one entry of groupStrips (mix-view.js):
//   { index, name, tone, invalid, level, readout, following }.
// Event: `mix-edit` { field: "groupLevels", index, value } (bubbling, composed) while its fader
//   moves. Groups have no mute of their own in the engine, so the strip offers none.

import "./fader.js";
import "./group-chip.js";
import { stripStyles } from "./strip-styles.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    width: 74px;
  }
  .strip {
    background: var(--ml-surface-sunken);
  }
  .follows {
    max-width: 100%;
    overflow: hidden;
    color: var(--ml-accent);
    font-size: var(--ml-text-2xs);
    font-weight: var(--ml-weight-bold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
`);

class MlGroupStrip extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      [stripStyles, styles],
      `<div class="strip" role="group">
        <div class="caption" aria-hidden="true">GROUP</div>
        <ml-group-chip></ml-group-chip>
        <div class="follows" hidden></div>
        <div class="levels"><ml-fader></ml-fader></div>
        <output class="readout"></output>
      </div>`,
    );
    this.#parts = {
      strip: root.querySelector(".strip"),
      chip: root.querySelector("ml-group-chip"),
      follows: root.querySelector(".follows"),
      fader: root.querySelector("ml-fader"),
      readout: root.querySelector(".readout"),
    };
    this.#parts.fader.addEventListener("input", (event) => {
      if (this.#data) {
        emit(this, "mix-edit", {
          field: "groupLevels",
          index: this.#data.index,
          value: event.detail.value,
        });
      }
    });
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    const following = data.following ?? null;
    reflect(
      parts.strip,
      "aria-label",
      following ? `${data.name} group, follows ${following}'s mix` : `${data.name} group`,
    );
    parts.follows.hidden = !following;
    setText(parts.follows, following ? `Follows ${following}` : "");
    reflect(parts.fader, "disabled", Boolean(following));
    reflect(parts.chip, "name", data.name);
    reflect(parts.chip, "tone", data.tone);
    reflect(parts.chip, "invalid", data.invalid);
    reflect(parts.fader, "label", `${data.name} group level`);
    parts.fader.value = data.level;
    setText(parts.readout, data.readout);
  }
}

define("ml-group-strip", MlGroupStrip);
