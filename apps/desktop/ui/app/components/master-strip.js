// <ml-master-strip>: a musician's master: the stereo output meters, the level, the phone's level
// cap (the loudest the phone may play; the musician sets it too), the engineer's limit (the loudest
// this desk lets the musician hear; their phone cannot change it), a line saying the lower of the
// two wins, and Mute.
//
// Property: `data`, from masterStrip (mix-view.js):
//   { volume, cap, limit, muted, readout, ceilingNote }. The engineer's local mix has no cap or
//   limit (both null, and no note): the strip then shows the level and Mute alone.
// Method: `setLevels(left, right)`, the output peaks from the `meters` event.
// Event: `mix-edit` { field, index: null, value } (bubbling, composed) for the level
//   ("volumePercent") and the cap ("maxLevelPercent") as they move, Mute ("muted"), and the limit
//   ("engineerLimitPercent") when a gesture on it ends: each limit change is kept by the desk, so a
//   drag sends its last value only.

import "./fader.js";
import "./meter.js";
import "./toggle.js";
import { showMasterValues, wireMasterControls } from "../lib/master-controls.js";
import { stripStyles } from "./strip-styles.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    width: 188px;
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
  .column[hidden],
  .note[hidden] {
    display: none;
  }
  .sub {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-2xs);
  }
  /* Level, cap and limit: the line wraps rather than run out of the strip. */
  .readout {
    white-space: normal;
    text-align: center;
  }
  .note {
    margin: 0;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-2xs);
    line-height: 1.35;
    text-align: center;
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
          <div class="column" data-cap title="The musician's own level cap; they can change it on their phone too">
            <ml-fader data-field="maxLevelPercent" compact label="Phone level cap" aria-describedby="ceiling"></ml-fader>
            <span class="sub" aria-hidden="true">CAP</span>
          </div>
          <div class="column" data-limit title="Your limit for this musician; their phone cannot change it">
            <ml-fader data-field="engineerLimitPercent" compact label="Engineer limit" aria-describedby="ceiling"></ml-fader>
            <span class="sub" aria-hidden="true">LIMIT</span>
          </div>
        </div>
        <output class="readout"></output>
        <!-- Describes both caps: it says which one wins. -->
        <p class="note" id="ceiling"></p>
        <ml-toggle kind="mute" wide label="Mute master">Mute</ml-toggle>
      </div>`,
    );
    const [left, right] = root.querySelectorAll("ml-meter");
    const [level, cap, limit] = root.querySelectorAll("ml-fader");
    this.#parts = {
      left,
      right,
      level,
      cap,
      limit,
      readout: root.querySelector(".readout"),
      note: root.querySelector(".note"),
      mute: root.querySelector("ml-toggle"),
      capColumn: root.querySelector("[data-cap]"),
      limitColumn: root.querySelector("[data-limit]"),
    };
    // The limit goes out when a gesture ends, the rest as it moves (see master-controls.js).
    wireMasterControls(this.#parts, (field, value) => this.#edit(field, value));
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    reflect(parts.level, "tone", data.muted ? "muted" : "accent");
    showMasterValues(parts, data);
    parts.capColumn.hidden = data.cap === null;
    parts.limitColumn.hidden = data.limit === null;
    setText(parts.readout, data.readout);
    parts.note.hidden = !data.ceilingNote;
    setText(parts.note, data.ceilingNote ?? "");
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
