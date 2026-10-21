// The Effects screen: the shared reverb and delay every input can send to (from the Processing
// editor) and every mix can return (from its master strip). The reverb's size, damping, pre-delay
// and width; the delay's time, or a note value at a tempo, its feedback, its high cut and
// ping-pong.
//
// A container: effectsView (effects.js) works out every value and word; this screen builds the
// controls, draws what the view says at most once per animation frame and sends each change
// through the console's editEffect (the whole effect, the latest change winning).

import { DELAY_NOTES, EFFECT_RANGES, clampDelay, clampReverb, effectsView, readTempo } from "../lib/effects.js";
import { frameScheduler } from "../lib/scheduling.js";

// The sliders of each effect: the setting, its words in the view, its label and its range.
const REVERB_FIELDS = Object.freeze([
  ["sizePercent", "size", "Size"],
  ["dampingPercent", "damping", "Damping"],
  ["preDelayMs", "preDelay", "Pre-delay"],
  ["widthPercent", "width", "Width"],
]);
const DELAY_FIELDS = Object.freeze([
  ["timeMs", "time", "Time"],
  ["feedbackPercent", "feedback", "Feedback"],
  ["highCutHz", "highCut", "High cut"],
]);
// Each slider's step.
const STEPS = Object.freeze({ highCutHz: 100 });

function sliderRow(effect, [key, , label]) {
  const id = `effects-${effect}-${key}`;
  const [low, high] = EFFECT_RANGES[key];
  return `<div class="processing-field">
      <label for="${id}">${label}</label>
      <input id="${id}" type="range" min="${low}" max="${high}" step="${STEPS[key] ?? 1}" aria-label="${effect === "reverb" ? "Reverb" : "Delay"} ${label.toLowerCase()}" data-effect="${effect}" data-key="${key}" />
      <output for="${id}" data-output="${effect}.${key}"></output>
    </div>`;
}

const LAYOUT = `
  <div class="effects-head">
    <p>One reverb and one delay, shared by every mix. Send inputs to them from the <a href="#/processing">Processing</a> editor; each mix returns them on its master strip. They add no latency to the dry mix.</p>
  </div>
  <p class="effects-note" data-note hidden>The engine is not running. Start it from the Mix screen to change the effects.</p>
  <p class="effects-outcome" role="status" aria-live="polite" data-outcome></p>
  <fieldset class="effects-cards" data-cards>
    <legend class="ml-visually-hidden">Effects</legend>
    <section class="processing-card" aria-labelledby="effects-reverb-title">
      <div class="processing-card-head"><h2 id="effects-reverb-title">Reverb</h2></div>
      ${REVERB_FIELDS.map((field) => sliderRow("reverb", field)).join("")}
    </section>
    <section class="processing-card" aria-labelledby="effects-delay-title">
      <div class="processing-card-head">
        <h2 id="effects-delay-title">Delay</h2>
        <label class="processing-on"><input type="checkbox" aria-label="Delay ping-pong" data-ping-pong /><span>Ping-pong</span></label>
      </div>
      <form class="effects-tempo" data-tempo-form>
        <label for="effects-tempo">Tempo</label>
        <input id="effects-tempo" class="processing-text" type="text" inputmode="decimal" placeholder="bpm" autocomplete="off" data-tempo />
        <label for="effects-note">Note</label>
        <select id="effects-note" class="processing-text" data-note-value>
          ${DELAY_NOTES.map((note) => `<option value="${note.value}">${note.label}</option>`).join("")}
        </select>
        <button type="submit" class="processing-button">Set tempo</button>
      </form>
      ${DELAY_FIELDS.map((field) => sliderRow("delay", field)).join("")}
      <p class="effects-repeats" data-repeats></p>
    </section>
  </fieldset>
`;

/** Mounts the Effects screen in `main` for the console `app`; returns the unmount function. */
export function mountEffectsScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "effects-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const $ = (selector) => screen.querySelector(selector);
  const parts = {
    note: $("[data-note]"),
    outcome: $("[data-outcome]"),
    cards: $("[data-cards]"),
    pingPong: $("[data-ping-pong]"),
    tempoForm: $("[data-tempo-form]"),
    tempo: $("[data-tempo]"),
    noteValue: $("[data-note-value]"),
    repeats: $("[data-repeats]"),
    sliders: [...screen.querySelectorAll("input[type=range]")],
  };
  let outcome = null;
  let dragging = null;
  let tempoTouched = false;

  const frames = frameScheduler(() => render());
  const scheduleRender = () => frames.schedule();

  function render() {
    if (frames.disposed) {
      return;
    }
    const view = effectsView(app.store.getState());
    parts.note.hidden = view.running;
    parts.cards.disabled = !view.running;
    parts.outcome.textContent = outcome?.message ?? "";
    parts.outcome.dataset.ok = String(outcome?.ok ?? true);
    for (const input of parts.sliders) {
      const effect = view[input.dataset.effect];
      const key = input.dataset.key;
      const field = (input.dataset.effect === "reverb" ? REVERB_FIELDS : DELAY_FIELDS).find(
        ([candidate]) => candidate === key,
      );
      if (dragging !== input && input.value !== String(effect[key])) {
        input.value = String(effect[key]);
      }
      const text = effect[field[1]];
      const output = $(`[data-output="${input.dataset.effect}.${key}"]`);
      if (output.textContent !== text) {
        output.textContent = text;
      }
      input.setAttribute("aria-valuetext", text);
    }
    const timeSlider = parts.sliders.find((input) => input.dataset.key === "timeMs");
    timeSlider.disabled = view.delay.synced;
    timeSlider.title = view.delay.synced ? "The tempo sets the time; clear the tempo to set it here" : "";
    parts.pingPong.checked = view.delay.pingPong;
    if (!tempoTouched && document.activeElement !== parts.tempo) {
      parts.tempo.value = view.delay.synced ? String(view.delay.tempoBpm) : "";
    }
    if (document.activeElement !== parts.noteValue && parts.noteValue.value !== view.delay.note) {
      parts.noteValue.value = view.delay.note;
    }
    parts.repeats.textContent = `Repeats every ${view.delay.timeInForce}.`;
  }

  // Sends `effect` with `change` merged into what it shows now.
  function edit(effect, change) {
    const view = effectsView(app.store.getState());
    const current = effect === "reverb" ? clampReverb(view.reverb) : clampDelay(view.delay);
    app.editEffect(effect, { ...current, ...change }).then((said) => {
      outcome = said;
      scheduleRender();
    });
    scheduleRender();
  }

  for (const input of parts.sliders) {
    input.addEventListener("input", () => edit(input.dataset.effect, { [input.dataset.key]: Number(input.value) }));
    input.addEventListener("pointerdown", () => {
      dragging = input;
    });
    const release = () => {
      dragging = dragging === input ? null : dragging;
      scheduleRender();
    };
    input.addEventListener("pointerup", release);
    input.addEventListener("change", release);
  }
  parts.pingPong.addEventListener("change", () => edit("delay", { pingPong: parts.pingPong.checked }));
  parts.tempo.addEventListener("input", () => {
    tempoTouched = true;
  });
  parts.noteValue.addEventListener("change", () => edit("delay", { note: parts.noteValue.value }));
  parts.tempoForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const read = readTempo(parts.tempo.value);
    if (!read.ok) {
      outcome = { ok: false, message: read.message };
      scheduleRender();
      return;
    }
    tempoTouched = false;
    edit("delay", { tempoBpm: read.tempoBpm, note: parts.noteValue.value });
  });

  const unsubscribe = app.store.subscribe(scheduleRender);
  render();
  return () => {
    unsubscribe();
    frames.dispose();
    screen.remove();
  };
}
