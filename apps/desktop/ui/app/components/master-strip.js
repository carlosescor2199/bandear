// <ml-master-strip>: a musician's master: the stereo output meters, the level, the level cap
// (the loudest the phone may play) and Mute.
//
// Property: `data`, from masterStrip (mix-view.js): { volume, cap, muted, readout }.
// Method: `setLevels(left, right)`, the output peaks from the `meters` event.
// Event: `mix-edit` { field, index: null, value } (bubbling, composed) for the level
//   ("volumePercent"), the cap ("maxLevelPercent") and Mute ("muted").

import "./fader.js";
import "./meter.js";
import "./toggle.js";
import { stripStyles } from "./strip-styles.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    width: 148px;
  }
  .strip {
    border-color: var(--ml-accent);
    background: var(--ml-accent-bg);
  }
  .levels {
    gap: 8px;
  }
  .column {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }
  .column ml-fader {
    flex: 1 1 auto;
  }
  .sub {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-2xs);
  }
`);

class MlMasterStrip extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      [stripStyles, styles],
      `<div class="strip" role="group" aria-label="Master">
        <div class="caption" aria-hidden="true">MASTER</div>
        <div class="levels">
          <ml-meter label="Output left"></ml-meter>
          <ml-meter label="Output right"></ml-meter>
          <div class="column">
            <ml-fader data-field="volumePercent" label="Master level"></ml-fader>
            <span class="sub" aria-hidden="true">LEVEL</span>
          </div>
          <div class="column">
            <ml-fader data-field="maxLevelPercent" compact label="Level cap"></ml-fader>
            <span class="sub" aria-hidden="true">CAP</span>
          </div>
        </div>
        <output class="readout"></output>
        <ml-toggle kind="mute" wide label="Mute master">Mute</ml-toggle>
      </div>`,
    );
    const [left, right] = root.querySelectorAll("ml-meter");
    const [level, cap] = root.querySelectorAll("ml-fader");
    this.#parts = {
      left,
      right,
      level,
      cap,
      readout: root.querySelector(".readout"),
      mute: root.querySelector("ml-toggle"),
    };
    for (const fader of [level, cap]) {
      fader.addEventListener("input", (event) => {
        this.#edit(fader.dataset.field, event.detail.value);
      });
    }
    this.#parts.mute.addEventListener("change", (event) => {
      this.#edit("muted", event.detail.pressed);
    });
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    reflect(parts.level, "tone", data.muted ? "muted" : "accent");
    parts.level.value = data.volume;
    parts.cap.value = data.cap;
    setText(parts.readout, data.readout);
    parts.mute.pressed = data.muted;
  }

  setLevels(left, right) {
    this.#parts.left.setLevel(left);
    this.#parts.right.setLevel(right);
  }

  #edit(field, value) {
    if (this.#data) {
      emit(this, "mix-edit", { field, index: null, value });
    }
  }
}

define("ml-master-strip", MlMasterStrip);
