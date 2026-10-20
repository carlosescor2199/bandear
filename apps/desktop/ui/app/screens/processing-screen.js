// The Processing screen (the design's processing board, one strip per input as the engineer chose):
// the editor of one input's channel processing. A channel picker with previous and next, the
// strip's actions (A/B bypass, Reset, Copy to…, Presets), the input and processed meters, the
// equaliser with its response curve and handles, the gate and the compressor with their
// gain-reduction meters and the compressor's static curve.
//
// It is a container: processingEditor (processing-editor.js) holds the open strip, sends the
// changes and keeps the last outcome; processing-view.js works out every number, word and shape;
// this screen builds the controls, draws what they work out at most once per animation frame and
// routes the meters straight to the meters. It opens on the channel the hash names
// ("#/processing/5") and keeps the hash on the channel open.

import "../components/eq-graph.js";
import "../components/meter.js";
import "../components/reduction-meter.js";
import { processingEditor } from "../lib/processing-editor.js";
import {
  BAND_FIELDS,
  BAND_NAMES,
  COMPRESSOR_FIELDS,
  COMPRESSOR_REDUCTION_SCALE_DB,
  GATE_FIELDS,
  GATE_REDUCTION_SCALE_DB,
  HIGH_PASS_FIELDS,
  SLIDER_STEPS,
  bandHandles,
  curveDescription,
  curvePath,
  dragHandle,
  fieldPosition,
  fieldValue,
  formatDb,
  formatRatio,
  gridLines,
  handleKey,
  nudgeQ,
  transferPath,
} from "../lib/processing-view.js";
import { processingHash, routeChannel } from "../lib/router.js";

const GRAPH = Object.freeze({ width: 600, height: 200 });
const TRANSFER_SIZE = 60;

const LAYOUT = `
  <header class="screen-header">
    <h1 tabindex="-1">Processing</h1>
    <p>Each input's processing is heard in every mix. Edit one channel, then copy it where it fits.</p>
  </header>
  <div class="processing-note" data-note hidden>
    <p data-note-text></p>
    <button type="button" class="processing-button" data-read hidden>Read again</button>
  </div>
  <div class="processing-messages">
    <p class="processing-message" role="status" data-outcome></p>
    <p class="processing-message processing-message--error" role="alert" data-failure></p>
  </div>
  <div class="processing-body" data-body hidden>
    <div class="processing-bar">
      <div class="processing-group">
        <button type="button" class="processing-button processing-step" data-prev aria-label="Previous channel" title="Previous channel">&lsaquo;</button>
        <label for="processing-channel">Channel</label>
        <select id="processing-channel" data-channel></select>
        <button type="button" class="processing-button processing-step" data-next aria-label="Next channel" title="Next channel">&rsaquo;</button>
        <span class="processing-state" data-state></span>
      </div>
      <div class="processing-group">
        <button type="button" class="processing-button" data-bypass aria-pressed="false"
          title="Hear the channel with no processing; press again to put it back">Bypass (A/B)</button>
        <button type="button" class="processing-button" data-reset title="Take every part of this strip off">Reset</button>
        <details class="processing-copy" data-copy-box>
          <summary class="processing-button">Copy to…</summary>
          <div class="processing-popover">
            <fieldset>
              <legend data-copy-legend></legend>
              <div class="processing-copy-list" data-copy-list></div>
            </fieldset>
            <button type="button" class="processing-button processing-button--primary" data-copy>Copy</button>
          </div>
        </details>
      </div>
      <div class="processing-group">
        <label for="processing-preset">Preset</label>
        <select id="processing-preset" data-preset></select>
        <button type="button" class="processing-button" data-apply>Apply</button>
        <button type="button" class="processing-button" data-delete>Delete</button>
      </div>
      <form class="processing-group" data-save>
        <label for="processing-preset-name">Save as</label>
        <input id="processing-preset-name" class="processing-text" type="text" maxlength="40" autocomplete="off" spellcheck="false" placeholder="Preset name" data-save-name />
        <button type="submit" class="processing-button">Save</button>
      </form>
    </div>
    <section class="processing-levels" aria-label="Levels">
      <span class="processing-level-label" id="processing-input-label">Input</span>
      <ml-meter orientation="horizontal" data-input></ml-meter>
      <span class="processing-level-label" id="processing-processed-label">Processed</span>
      <ml-meter orientation="horizontal" data-processed></ml-meter>
    </section>
    <p class="processing-bypassed" data-bypassed hidden></p>
    <fieldset class="processing-sections" data-sections>
      <legend class="ml-visually-hidden">Processing controls</legend>
      <section class="processing-card processing-card--eq" aria-labelledby="processing-eq-title">
        <div class="processing-card-head">
          <h2 id="processing-eq-title">Equaliser</h2>
          <p>High-pass, then a low shelf, two peaks and a high shelf. Drag a handle; wheel for Q.</p>
        </div>
        <ml-eq-graph></ml-eq-graph>
        <div class="processing-bands" data-bands></div>
      </section>
      <section class="processing-card" aria-labelledby="processing-gate-title" data-gate>
        <div class="processing-card-head">
          <h2 id="processing-gate-title">Gate</h2>
        </div>
        <div class="processing-reduction">
          <span>Reduction</span>
          <ml-reduction-meter label="Gate gain reduction" scale="${GATE_REDUCTION_SCALE_DB}" data-gate-reduction></ml-reduction-meter>
        </div>
      </section>
      <section class="processing-card" aria-labelledby="processing-compressor-title" data-compressor>
        <div class="processing-card-head">
          <h2 id="processing-compressor-title">Compressor</h2>
        </div>
        <div class="processing-reduction">
          <span>Reduction</span>
          <ml-reduction-meter label="Compressor gain reduction" scale="${COMPRESSOR_REDUCTION_SCALE_DB}" data-compressor-reduction></ml-reduction-meter>
        </div>
        <figure class="processing-transfer">
          <svg viewBox="0 0 ${TRANSFER_SIZE} ${TRANSFER_SIZE}" role="img" data-transfer>
            <path class="processing-transfer-grid" d="M0 ${TRANSFER_SIZE}L${TRANSFER_SIZE} 0"></path>
            <path class="processing-transfer-curve" data-transfer-curve></path>
          </svg>
          <figcaption>Output against input, −60 to 0 dB</figcaption>
        </figure>
      </section>
    </fieldset>
  </div>
`;

let nextId = 0;

// A slider row of `field` at `path` ("gate.thresholdDb"): its label, the range input and the value
// in words. `{ row, input, output, field, path }`.
function fieldRow(field, path, name) {
  nextId += 1;
  const id = `processing-field-${nextId}`;
  const row = document.createElement("div");
  row.className = "processing-field";
  row.innerHTML = `<label for="${id}"></label><input id="${id}" type="range" min="0" max="${SLIDER_STEPS}" step="1" /><output for="${id}"></output>`;
  row.querySelector("label").textContent = field.label;
  const input = row.querySelector("input");
  input.setAttribute("aria-label", `${name} ${field.label.toLowerCase()}`);
  input.dataset.path = path;
  return { row, input, output: row.querySelector("output"), field, path };
}

// An On checkbox for the part at `path` ("gate", "eq.bands.1"), named `name`.
function onSwitch(path, name) {
  const label = document.createElement("label");
  label.className = "processing-on";
  label.innerHTML = `<input type="checkbox" /><span>On</span>`;
  const input = label.querySelector("input");
  input.setAttribute("aria-label", `${name} on`);
  input.dataset.path = `${path}.enabled`;
  return { label, input };
}

/** Mounts the Processing screen in `main` for the console `app`; returns the unmount function. */
export function mountProcessingScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "processing-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const $ = (selector) => screen.querySelector(selector);
  const parts = {
    note: $("[data-note]"),
    noteText: $("[data-note-text]"),
    read: $("[data-read]"),
    outcome: $("[data-outcome]"),
    failure: $("[data-failure]"),
    body: $("[data-body]"),
    channel: $("[data-channel]"),
    prev: $("[data-prev]"),
    next: $("[data-next]"),
    state: $("[data-state]"),
    bypass: $("[data-bypass]"),
    reset: $("[data-reset]"),
    copyBox: $("[data-copy-box]"),
    copyLegend: $("[data-copy-legend]"),
    copyList: $("[data-copy-list]"),
    copy: $("[data-copy]"),
    preset: $("[data-preset]"),
    apply: $("[data-apply]"),
    remove: $("[data-delete]"),
    save: $("[data-save]"),
    saveName: $("[data-save-name]"),
    input: $("[data-input]"),
    processed: $("[data-processed]"),
    bypassed: $("[data-bypassed]"),
    sections: $("[data-sections]"),
    graph: $("ml-eq-graph"),
    bands: $("[data-bands]"),
    gate: $("[data-gate]"),
    compressor: $("[data-compressor]"),
    gateReduction: $("[data-gate-reduction]"),
    compressorReduction: $("[data-compressor-reduction]"),
    transfer: $("[data-transfer]"),
    transferCurve: $("[data-transfer-curve]"),
  };

  const commands = processingEditor({
    app,
    channel: routeChannel(window.location.hash),
    changed: () => scheduleRender(),
  });

  // Every slider and On switch of the strip, built once.
  const rows = [];
  const switches = [];
  function addPart(container, path, name, fields, before = null) {
    const toggle = onSwitch(path, name);
    switches.push(toggle);
    const list = fields.map((field) => fieldRow(field, `${path}.${field.key}`, name));
    rows.push(...list);
    container.insertBefore(toggle.label, before);
    for (const { row } of list) {
      container.insertBefore(row, before);
    }
  }
  const panel = (title) => {
    const element = document.createElement("div");
    element.className = "processing-band";
    const heading = document.createElement("h3");
    heading.textContent = title;
    element.append(heading);
    parts.bands.append(element);
    return element;
  };
  addPart(panel("High-pass"), "eq.highPass", "High-pass", HIGH_PASS_FIELDS);
  BAND_NAMES.forEach((name, index) => addPart(panel(name), `eq.bands.${index}`, name, BAND_FIELDS));
  for (const [section, path, name, fields] of [
    [parts.gate, "gate", "Gate", GATE_FIELDS],
    [parts.compressor, "compressor", "Compressor", COMPRESSOR_FIELDS],
  ]) {
    const head = section.querySelector(".processing-card-head");
    const toggle = onSwitch(path, name);
    switches.push(toggle);
    head.append(toggle.label);
    const list = fields.map((field) => fieldRow(field, `${path}.${field.key}`, name));
    rows.push(...list);
    head.after(...list.map(({ row }) => row));
  }

  let view = commands.view();

  function readPath(settings, path) {
    return path.split(".").reduce((part, key) => part?.[key], settings);
  }

  function renderOptions(select, options, value) {
    const signature = JSON.stringify(options);
    if (select.dataset.signature !== signature) {
      select.dataset.signature = signature;
      select.replaceChildren(...options.map((option) => new Option(option.label, option.value)));
    }
    if (value !== null && select.value !== String(value)) {
      select.value = String(value);
    }
  }

  function renderCopyList() {
    const targets = view.choices.filter((choice) => choice.channel !== view.channel);
    const signature = JSON.stringify([view.channel, targets]);
    if (parts.copyList.dataset.signature === signature) {
      return;
    }
    parts.copyList.dataset.signature = signature;
    parts.copyList.replaceChildren(
      ...targets.map((choice) => {
        const label = document.createElement("label");
        label.className = "processing-on";
        const box = document.createElement("input");
        box.type = "checkbox";
        box.value = String(choice.channel);
        const text = document.createElement("span");
        text.textContent = choice.label;
        label.append(box, text);
        return label;
      }),
    );
    parts.copyLegend.textContent = `Copy ${view.choice?.label ?? `CH ${view.channel}`} to`;
  }

  function render() {
    view = commands.view();
    const failure = view.outcome && !view.outcome.ok ? view.outcome.message : view.error ?? "";
    parts.outcome.textContent = view.outcome?.ok ? view.outcome.message : "";
    parts.failure.textContent = failure;

    const blocked = !view.known || view.unread;
    parts.note.hidden = !blocked;
    parts.body.hidden = blocked;
    parts.read.hidden = view.known ? !view.unread : true;
    parts.noteText.textContent = !view.known
      ? "The engine is not running, so this desk does not know its inputs yet. Start it from the Mix screen."
      : "The channel processing could not be read.";
    if (blocked) {
      return;
    }
    if (routeChannel(window.location.hash) !== view.channel) {
      history.replaceState(null, "", processingHash(view.channel));
    }

    renderOptions(
      parts.channel,
      view.choices.map((choice) => ({ value: choice.channel, label: choice.label })),
      view.channel,
    );
    const at = view.choices.findIndex((choice) => choice.channel === view.channel);
    parts.prev.disabled = at <= 0;
    parts.next.disabled = at === view.choices.length - 1;
    parts.state.textContent = view.bypassed
      ? "Bypassed"
      : view.active
        ? "Processing on"
        : "No processing";
    parts.state.dataset.state = view.bypassed ? "bypassed" : view.active ? "on" : "off";

    parts.bypass.setAttribute("aria-pressed", String(view.bypassed));
    parts.bypass.disabled = !view.canBypass;
    for (const control of [parts.reset, parts.copy, parts.apply]) {
      control.disabled = view.bypassed;
    }
    parts.bypassed.hidden = !view.bypassed;
    parts.bypassed.textContent = view.bypassed
      ? `You hear ${view.choice.label} with no processing. Its settings are kept below; press Bypass (A/B) again to put them back. Leaving this screen puts them back too.`
      : "";
    parts.sections.disabled = view.bypassed;

    renderCopyList();
    const presetOptions = view.presets.map((preset) => ({ value: preset.name, label: preset.label }));
    renderOptions(parts.preset, presetOptions, null);
    const chosen = view.presets.find((preset) => preset.name === parts.preset.value) ?? null;
    parts.apply.disabled = view.bypassed || chosen === null;
    parts.remove.disabled = chosen === null || chosen.factory;
    parts.remove.title = chosen?.factory
      ? "Built-in presets cannot be deleted"
      : "Delete this preset of yours";

    for (const { input, output, field, path } of rows) {
      const value = readPath(view.settings, path);
      const position = String(fieldPosition(field, value));
      if (input.value !== position) {
        input.value = position;
      }
      const text = field.format(value);
      if (output.textContent !== text) {
        output.textContent = text;
      }
      input.setAttribute("aria-valuetext", text);
    }
    for (const { input } of switches) {
      input.checked = Boolean(readPath(view.settings, input.dataset.path));
    }

    const { eq, compressor } = view.settings;
    parts.graph.data = {
      ...GRAPH,
      curve: curvePath(view.curve.points, GRAPH.width, GRAPH.height),
      grid: gridLines(GRAPH.width, GRAPH.height),
      handles: bandHandles(eq, GRAPH.width, GRAPH.height),
      description: curveDescription(eq),
      disabled: view.bypassed,
    };
    parts.transferCurve.setAttribute("d", transferPath(compressor, TRANSFER_SIZE));
    parts.transfer.setAttribute(
      "aria-label",
      `Compressor curve: threshold ${formatDb(compressor.thresholdDb)}, ratio ${formatRatio(
        compressor.ratio,
      )}, knee ${formatDb(compressor.kneeDb)}, makeup ${formatDb(compressor.makeupDb)}`,
    );
    reflectLevels();
  }

  let frame = 0;
  function scheduleRender() {
    if (frame === 0) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        render();
      });
    }
  }

  // The meters of the open strip, from the last `meters` payload.
  let lastMeters = null;
  function reflectLevels() {
    if (!lastMeters || parts.body.hidden) {
      return;
    }
    const indexes = view.sides.map((side) => side - 1);
    const peak = (list) => Math.max(0, ...indexes.map((index) => list?.[index] ?? 0));
    parts.input.setLevel(peak(lastMeters.channels));
    parts.processed.setLevel(peak(lastMeters.processed ?? lastMeters.channels));
    parts.gateReduction.setReduction(peak(lastMeters.gateReductionDb));
    parts.compressorReduction.setReduction(peak(lastMeters.compressorReductionDb));
  }
  parts.input.setAttribute("label", "Input level");
  parts.processed.setAttribute("label", "Processed level");

  // Every change of the strip goes through the editor; an equaliser change also asks for its curve.
  function edited(path, value) {
    commands.edit(path, value).then(scheduleRender);
    if (path.startsWith("eq.")) {
      commands.requestCurve();
    }
  }
  // The equaliser as it is now, a gesture before the next frame included, for the next gesture.
  const currentEq = () => commands.view().settings.eq;
  function editedEq(eq) {
    if (eq) {
      commands.editEq(eq).then(scheduleRender);
      commands.requestCurve();
    }
  }

  parts.sections.addEventListener("input", (event) => {
    const row = rows.find((candidate) => candidate.input === event.target);
    if (row) {
      edited(row.path, fieldValue(row.field, Number(row.input.value)));
    }
  });
  parts.sections.addEventListener("change", (event) => {
    const toggle = switches.find((candidate) => candidate.input === event.target);
    if (toggle) {
      edited(toggle.input.dataset.path, toggle.input.checked);
    }
  });
  parts.graph.addEventListener("eq-drag", ({ detail }) =>
    editedEq(dragHandle(currentEq(), detail.target, detail.x, detail.y, detail.width, detail.height)),
  );
  parts.graph.addEventListener("eq-q", ({ detail }) =>
    editedEq(nudgeQ(currentEq(), detail.target, detail.steps)),
  );
  parts.graph.addEventListener("eq-key", ({ detail }) =>
    editedEq(handleKey(currentEq(), detail.target, detail.key, { shift: detail.shift })),
  );

  function opened(channel) {
    commands.open(channel);
    commands.requestCurve();
    render();
  }
  parts.channel.addEventListener("change", () => opened(Number(parts.channel.value)));
  parts.prev.addEventListener("click", () => opened(commands.step(-1)));
  parts.next.addEventListener("click", () => opened(commands.step(1)));
  parts.read.addEventListener("click", () => commands.readAgain().then(scheduleRender));

  const done = (promise) => promise.then(scheduleRender);
  parts.bypass.addEventListener("click", () => done(commands.toggleBypass()));
  parts.reset.addEventListener("click", () => {
    done(commands.reset());
    commands.requestCurve();
  });
  parts.copy.addEventListener("click", () => {
    const targets = [...parts.copyList.querySelectorAll("input:checked")].map((box) => Number(box.value));
    done(
      commands.copyTo(targets).then((outcome) => {
        if (outcome.ok) {
          parts.copyBox.open = false;
          parts.copyList.dataset.signature = "";
        }
      }),
    );
  });
  parts.preset.addEventListener("change", () => render());
  parts.apply.addEventListener("click", () => {
    done(commands.applyPreset(parts.preset.value));
    commands.requestCurve();
  });
  parts.remove.addEventListener("click", () => done(commands.deletePreset(parts.preset.value)));
  parts.save.addEventListener("submit", (event) => {
    event.preventDefault();
    done(
      commands.savePreset(parts.saveName.value).then((outcome) => {
        if (outcome.ok) {
          parts.preset.value = parts.saveName.value.trim();
          parts.saveName.value = "";
        }
      }),
    );
  });

  const unsubscribe = app.store.subscribe(scheduleRender);
  const stopMeters = app.onMeters((meters) => {
    lastMeters = meters;
    reflectLevels();
  });
  // The presets may not be read yet (a failed read when connecting).
  if (app.store.getState().stripPresets === null) {
    app.loadStripPresets();
  }
  render();
  commands.requestCurve();

  return () => {
    unsubscribe();
    stopMeters();
    cancelAnimationFrame(frame);
    commands.dispose();
    screen.remove();
  };
}
