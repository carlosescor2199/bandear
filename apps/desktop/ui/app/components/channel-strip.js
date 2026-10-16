// <ml-channel-strip>: one input in a musician's mix, or a stereo pair of inputs as one strip:
// instrument icon and name, group chip, pan (a balance for a pair, marked "Stereo"), input meter,
// fader with its value, Mute and Solo.
//
// Property: `data`, one entry of channelStrips (mix-view.js):
//   { index, sides, stereo, label, kind, group, gain, pan, muted, solo, tone, readout }.
// Method: `setLevel(level)`, the input peak from the `meters` event (a pair's louder side).
// Event: `mix-edit` { field, index, sides, stereo, value } (bubbling, composed) for a move of the
//   fader ("channelGains") or the pan ("pans", a balance for a pair) and a press of Mute
//   ("channelMuted") or Solo ("channelSolo"); the screen writes it to every input in `sides`
//   (channelEdits). The strip does not change its data itself: the screen answers with new data.

import "./fader.js";
import "./group-chip.js";
import "./meter.js";
import "./pan.js";
import "./toggle.js";
import { kindIcon, stereoIcon } from "./kind-icons.js";
import { stripStyles } from "./strip-styles.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    width: 84px;
  }
  .name {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    width: 100%;
    font-size: var(--ml-text-md);
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
  }
  .kind-icon {
    display: inline-flex;
    flex: none;
    color: var(--ml-text-secondary);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip {
    min-height: 20px;
  }
  .stereo {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-2xs);
    font-weight: var(--ml-weight-bold);
    letter-spacing: var(--ml-tracking-caps);
  }
  /* A mono strip keeps the line empty, so every strip's pan and fader line up. */
  .stereo[hidden] {
    visibility: hidden;
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
        <div class="name"><span class="kind-icon"></span><span class="label"></span></div>
        <div class="chip"><ml-group-chip hidden></ml-group-chip></div>
        <div class="stereo" aria-hidden="true" hidden>${stereoIcon(12)}STEREO</div>
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
      name: root.querySelector(".label"),
      kindIcon: root.querySelector(".kind-icon"),
      stereo: root.querySelector(".stereo"),
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
    const kindChanged = this.#data === null || this.#data.kind !== data.kind;
    this.#data = data;
    const parts = this.#parts;
    const { label, stereo } = data;
    reflect(parts.strip, "aria-label", stereo ? `${label}, stereo pair` : label);
    setText(parts.name, label);
    parts.name.title = label;
    if (kindChanged) {
      parts.kindIcon.innerHTML = kindIcon(data.kind, 14);
    }
    parts.stereo.hidden = !stereo;
    parts.chip.hidden = data.group === null;
    reflect(parts.chip, "name", data.group?.name ?? null);
    reflect(parts.chip, "tone", data.group?.tone ?? null);
    reflect(parts.chip, "invalid", data.group?.invalid ?? false);
    reflect(parts.pan, "label", stereo ? `${label} balance` : `${label} pan`);
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
        const { index, sides, stereo } = this.#data;
        emit(this, "mix-edit", { field, index, sides, stereo, value: valueOf(event) });
      }
    });
  }
}

define("ml-channel-strip", MlChannelStrip);
