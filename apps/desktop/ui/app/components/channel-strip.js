// <ml-channel-strip>: one input in a musician's mix: name, group chip, pan, input meter, fader
// with its value, Mute and Solo.
//
// Property: `data`, one entry of channelStrips (mix-view.js):
//   { index, label, group, gain, pan, muted, solo, tone, readout }.
// Method: `setLevel(level)`, the input peak from the `meters` event.
// Event: `mix-edit` { field, index, value } (bubbling, composed) for a move of the fader
//   ("channelGains") or the pan ("pans") and a press of Mute ("channelMuted") or Solo
//   ("channelSolo"). The strip does not change its data itself: the screen answers with new data.

import "./fader.js";
import "./group-chip.js";
import "./meter.js";
import "./pan.js";
import "./toggle.js";
import { stripStyles } from "./strip-styles.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    width: 84px;
  }
  .name {
    font-size: var(--ml-text-md);
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
  }
  .chip {
    min-height: 20px;
  }
  .readout[data-muted] {
    color: var(--ml-text-muted);
  }
`);

class MlChannelStrip extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      [stripStyles, styles],
      `<div class="strip" role="group">
        <div class="name"></div>
        <div class="chip"><ml-group-chip hidden></ml-group-chip></div>
        <ml-pan></ml-pan>
        <div class="levels"><ml-meter></ml-meter><ml-fader></ml-fader></div>
        <output class="readout"></output>
        <div class="buttons">
          <ml-toggle kind="mute">M</ml-toggle><ml-toggle kind="solo">S</ml-toggle>
        </div>
      </div>`,
    );
    const [mute, solo] = root.querySelectorAll("ml-toggle");
    this.#parts = {
      strip: root.querySelector(".strip"),
      name: root.querySelector(".name"),
      chip: root.querySelector("ml-group-chip"),
      pan: root.querySelector("ml-pan"),
      meter: root.querySelector("ml-meter"),
      fader: root.querySelector("ml-fader"),
      readout: root.querySelector(".readout"),
      mute,
      solo,
    };
    this.#forward(this.#parts.fader, "input", "channelGains", (event) => event.detail.value);
    this.#forward(this.#parts.pan, "input", "pans", (event) => event.detail.value);
    this.#forward(mute, "change", "channelMuted", (event) => event.detail.pressed);
    this.#forward(solo, "change", "channelSolo", (event) => event.detail.pressed);
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    const { label } = data;
    reflect(parts.strip, "aria-label", label);
    setText(parts.name, label);
    parts.chip.hidden = data.group === null;
    reflect(parts.chip, "name", data.group?.name ?? null);
    reflect(parts.chip, "tone", data.group?.tone ?? null);
    reflect(parts.chip, "invalid", data.group?.invalid ?? false);
    reflect(parts.pan, "label", `${label} pan`);
    parts.pan.value = data.pan;
    reflect(parts.meter, "label", `${label} input`);
    reflect(parts.fader, "label", `${label} level`);
    reflect(parts.fader, "tone", data.tone === "default" ? null : data.tone);
    parts.fader.value = data.gain;
    setText(parts.readout, data.readout);
    reflect(parts.readout, "data-muted", data.muted);
    reflect(parts.mute, "label", `Mute ${label}`);
    parts.mute.pressed = data.muted;
    reflect(parts.solo, "label", `Solo ${label}`);
    parts.solo.pressed = data.solo;
  }

  setLevel(level) {
    this.#parts.meter.setLevel(level);
  }

  #forward(element, type, field, valueOf) {
    element.addEventListener(type, (event) => {
      if (this.#data) {
        emit(this, "mix-edit", { field, index: this.#data.index, value: valueOf(event) });
      }
    });
  }
}

define("ml-channel-strip", MlChannelStrip);
