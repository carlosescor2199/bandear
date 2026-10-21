// <ml-master-strip>: a musician's master: the stereo output meters, the level, the phone's level
// cap (the loudest the phone may play; the musician sets it too), the engineer's limit (the loudest
// this desk lets the musician hear; their phone cannot change it), a line saying the lower of the
// two wins, Mute, and the mix's master limiter: On, its ceiling and release as sliders with their
// values in words, its gain reduction and a line saying what it does.
//
// Property: `data`, from masterStrip (mix-view.js):
//   { volume, cap, limit, muted, readout, ceilingNote }. The engineer's local mix has no cap or
//   limit (both null, and no note): the strip then shows the level and Mute alone.
//   `limiter`, from masterLimiter (limiter.js), or absent to hide the limiter: { enabled, ceiling,
//   release, state, note, ceilingPosition, releasePosition }.
// Method: `setLevels(left, right)`, the output peaks from the `meters` event.
//   `setLimiterReduction(db)`, the limiter's gain reduction from the `meters` event (0 while off).
// Event: `mix-edit` { field, index: null, value } (bubbling, composed) for the level
//   ("volumePercent") and the cap ("maxLevelPercent") as they move, Mute ("muted"), and the limit
//   ("engineerLimitPercent") when a gesture on it ends: each limit change is kept by the desk, so a
//   drag sends its last value only.
//   `limiter-edit` { field, value } (bubbling, composed): "enabled" with true or false, or
//   "ceilingDb" / "releaseMs" with the slider's position (0-1,000; limiterChange turns it into the
//   setting) as it moves, with `key` true when a key moved the slider (an arrow, Page Up/Down, Home
//   or End), false for the pointer.

import "./fader.js";
import "./reduction-meter.js";
import "./meter.js";
import "./toggle.js";
import { LIMITER_REDUCTION_SCALE_DB } from "../lib/limiter.js";
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
  /* The master limiter, under Mute: its own group with a rule above it. */
  .limiter {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    padding-top: 8px;
    border-top: 1px solid var(--ml-border);
  }
  .limiter[hidden] {
    display: none;
  }
  .limiter-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .limiter-state {
    font-size: var(--ml-text-2xs);
    color: var(--ml-text-secondary);
  }
  .limiter-field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 6px;
    font-size: var(--ml-text-2xs);
    color: var(--ml-text-secondary);
  }
  .limiter-field output {
    font-family: var(--ml-font-mono);
    color: var(--ml-text);
    text-align: right;
  }
  .limiter-field input {
    grid-column: 1 / -1;
    width: 100%;
    min-height: 24px;
    margin: 0;
    accent-color: var(--ml-accent);
  }
  .limiter ml-reduction-meter {
    grid-template-columns: minmax(0, 1fr) 56px;
  }
`);

// The keys a range input moves by.
const SLIDER_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Home", "End"]);

class MlMasterStrip extends HTMLElement {
  #data = null;
  #parts;
  #limiter;
  // The limiter slider under the pointer, if any.
  #dragging = null;
  // The limiter slider a key is moving, until its `input` event.
  #keyed = null;

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
        <div class="limiter" role="group" aria-label="Limiter" data-limiter hidden>
          <div class="limiter-head">
            <ml-toggle wide label="Limiter on" data-limiter-on>Limiter</ml-toggle>
            <span class="limiter-state" data-limiter-state></span>
          </div>
          <label class="limiter-field">
            <span>Ceiling</span>
            <output data-limiter-value="ceilingDb"></output>
            <input type="range" min="0" max="1000" step="1" aria-label="Limiter ceiling" aria-describedby="limiter-note" data-limiter-field="ceilingDb" />
          </label>
          <label class="limiter-field">
            <span>Release</span>
            <output data-limiter-value="releaseMs"></output>
            <input type="range" min="0" max="1000" step="1" aria-label="Limiter release" data-limiter-field="releaseMs" />
          </label>
          <ml-reduction-meter label="Limiter gain reduction" scale="${LIMITER_REDUCTION_SCALE_DB}"></ml-reduction-meter>
          <p class="note" id="limiter-note" data-limiter-note></p>
        </div>
      </div>`,
    );
    const [left, right] = root.querySelectorAll("ml-meter");
    const [level, cap, limit] = root.querySelectorAll("ml-fader");
    this.#limiter = {
      group: root.querySelector("[data-limiter]"),
      on: root.querySelector("[data-limiter-on]"),
      state: root.querySelector("[data-limiter-state]"),
      note: root.querySelector("[data-limiter-note]"),
      reduction: root.querySelector("ml-reduction-meter"),
      fields: [...root.querySelectorAll("[data-limiter-field]")].map((input) => ({
        input,
        key: input.dataset.limiterField,
        output: root.querySelector(`[data-limiter-value="${input.dataset.limiterField}"]`),
      })),
    };
    this.#wireLimiter();
    this.#parts = {
      left,
      right,
      level,
      cap,
      limit,
      readout: root.querySelector(".readout"),
      note: root.querySelector(".note"),
      mute: root.querySelector("ml-toggle[kind=mute]"),
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
    this.#showLimiter(data.limiter ?? null);
  }

  setLevels(left, right) {
    this.#parts.left.setLevel(left);
    this.#parts.right.setLevel(right);
  }

  setLimiterReduction(db) {
    this.#limiter.reduction.setReduction(this.#data?.limiter?.enabled ? db : 0);
  }

  #showLimiter(view) {
    const limiter = this.#limiter;
    limiter.group.hidden = view === null;
    if (view === null) {
      return;
    }
    limiter.on.pressed = view.enabled;
    setText(limiter.state, view.enabled ? "On" : "Off");
    setText(limiter.note, view.note);
    const shown = {
      ceilingDb: { position: view.ceilingPosition, text: view.ceiling },
      releaseMs: { position: view.releasePosition, text: view.release },
    };
    for (const { input, key, output } of limiter.fields) {
      // A thumb under the pointer stays where it is held; it lands on the rounded value after.
      if (this.#dragging !== input && input.value !== String(shown[key].position)) {
        input.value = String(shown[key].position);
      }
      reflect(input, "aria-valuetext", shown[key].text);
      setText(output, shown[key].text);
    }
    if (!view.enabled) {
      limiter.reduction.setReduction(0);
    }
  }

  // On and the two sliders send `limiter-edit`; the sliders' own `input` events stay inside, so
  // nothing outside takes them for a fader's.
  #wireLimiter() {
    const limiter = this.#limiter;
    limiter.on.addEventListener("change", (event) => {
      event.stopPropagation();
      this.#limiterEdit("enabled", event.detail.pressed);
    });
    for (const { input, key } of limiter.fields) {
      input.addEventListener("keydown", (event) => {
        this.#keyed = SLIDER_KEYS.has(event.key) ? input : null;
      });
      input.addEventListener("keyup", () => {
        this.#keyed = null;
      });
      input.addEventListener("input", (event) => {
        event.stopPropagation();
        this.#limiterEdit(key, Number(input.value), this.#keyed === input);
      });
      input.addEventListener("pointerdown", () => {
        this.#dragging = input;
        this.#keyed = null;
      });
      const release = () => {
        if (this.#dragging === input) {
          this.#dragging = null;
          if (this.#data) {
            this.#showLimiter(this.#data.limiter ?? null);
          }
        }
      };
      input.addEventListener("pointerup", release);
      input.addEventListener("pointercancel", release);
      input.addEventListener("change", release);
    }
  }

  #limiterEdit(field, value, key = false) {
    if (this.#data?.limiter) {
      emit(this, "limiter-edit", { field, value, key });
    }
  }

  #edit(field, value) {
    if (this.#data) {
      emit(this, "mix-edit", { field, index: null, value });
    }
  }
}

define("ml-master-strip", MlMasterStrip);
