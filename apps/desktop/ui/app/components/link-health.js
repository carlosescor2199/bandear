// <ml-link-health>: one musician's link over the last minutes, as a chart of small multiples on one
// time axis (loss, round trip, and the phone's jitter and cushion when it reports them), underruns
// and timeouts as event marks, seconds without reports as shaded breaks and reconnects as labelled
// markers, with a summary line above it and the same numbers as a table on request. For a phone
// that reports them, the packets a redundant copy recovered are counted in the summary, the readout
// and the table; while redundancy is on for a phone that does not, a note says so.
//
// Property: `data` = the view from linkHealthView (link-health-view.js), or null to hide.
// Events (bubbling, composed): `link-range` { rangeMs } when a range is picked, `link-table`
//   { shown } when the table is shown or hidden, `link-close` when the panel is closed.
//
// The chart reads its width from the page and lays itself out with healthLayout. Hovering it, or
// moving through it with the arrow keys once it has focus, shows a crosshair on the nearest second
// and its values and local time in a readout; the summary is the chart's text alternative and the
// table holds every value without hovering.

import "./link-state.js";
import { healthLayout, nearestIndex, sampleTooltip } from "../lib/link-health-view.js";
import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const SVG = "http://www.w3.org/2000/svg";

const styles = sheet(`
  :host {
    display: block;
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-surface);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  h2 {
    margin: 0;
    font-size: var(--ml-text-lg);
    font-weight: var(--ml-weight-bold);
  }
  .grow {
    flex-grow: 1;
  }
  button {
    min-height: var(--ml-target);
    padding: 0 12px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-surface-raised);
    color: var(--ml-text);
    font: inherit;
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  .ranges {
    display: inline-flex;
    align-self: flex-start;
    overflow: hidden;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
  }
  .ranges button {
    min-width: 64px;
    border: 0;
    border-radius: 0;
    background: var(--ml-surface-raised);
    font-family: var(--ml-font-mono);
  }
  .ranges button + button {
    border-left: 1px solid var(--ml-border);
  }
  .ranges button[aria-pressed="true"] {
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
    margin: 0;
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-md);
  }
  .message {
    margin: 0;
    color: var(--ml-danger-text);
    font-size: var(--ml-text-md);
  }
  .empty,
  .note,
  .key {
    margin: 0;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  .chart {
    position: relative;
    min-width: 0;
  }
  svg {
    display: block;
    width: 100%;
    overflow: visible;
    font-family: var(--ml-font-sans);
    font-size: var(--ml-text-xs);
  }
  .refetching svg {
    opacity: 0.6;
  }
  .title {
    fill: var(--ml-text-secondary);
    font-weight: var(--ml-weight-medium);
  }
  .tick,
  .axis,
  .lane-label {
    fill: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
  }
  .lane-label {
    font-family: var(--ml-font-sans);
  }
  .grid {
    stroke: var(--ml-border-subtle);
    stroke-width: 1;
  }
  .baseline {
    stroke: var(--ml-border);
    stroke-width: 1;
  }
  .line {
    fill: none;
    stroke: var(--ml-blue);
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  /* More than a second every two pixels: a thinner line keeps the seconds apart. */
  .dense .line {
    stroke-width: 1.25;
  }
  .dot {
    fill: var(--ml-blue);
    stroke: var(--ml-surface);
    stroke-width: 2;
  }
  .gap {
    fill: var(--ml-surface-raised);
  }
  .event {
    fill: var(--ml-text-secondary);
  }
  .reconnect {
    stroke: var(--ml-text-muted);
    stroke-width: 1;
    stroke-dasharray: 4 3;
  }
  .reconnect-label {
    fill: var(--ml-text-secondary);
    font-weight: var(--ml-weight-medium);
  }
  .crosshair {
    stroke: var(--ml-text);
    stroke-width: 1;
    pointer-events: none;
  }
  svg:focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 4px;
  }
  .readout {
    position: absolute;
    top: 0;
    z-index: 1;
    min-width: 150px;
    padding: 8px 10px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-surface-sunken);
    box-shadow: 0 4px 12px rgb(0 0 0 / 40%);
    font-size: var(--ml-text-sm);
    pointer-events: none;
  }
  .readout-head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 4px;
  }
  .readout-title {
    font-weight: var(--ml-weight-bold);
  }
  .readout-time {
    color: var(--ml-text-muted);
    font-family: var(--ml-font-mono);
  }
  .readout-line {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .readout-line span:first-child {
    color: var(--ml-text-secondary);
  }
  .readout-line strong {
    font-family: var(--ml-font-mono);
    font-weight: var(--ml-weight-bold);
  }
  .readout-note {
    margin-top: 4px;
    color: var(--ml-text-muted);
  }
  .table-wrap {
    max-height: 320px;
    overflow: auto;
    border: 1px solid var(--ml-border-subtle);
    border-radius: var(--ml-radius-md);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--ml-text-sm);
  }
  th {
    position: sticky;
    top: 0;
    padding: 6px 8px;
    background: var(--ml-surface);
    color: var(--ml-text-muted);
    font-weight: var(--ml-weight-medium);
    text-align: right;
    white-space: nowrap;
  }
  td {
    padding: 4px 8px;
    border-top: 1px solid var(--ml-border-subtle);
    font-family: var(--ml-font-mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  th:nth-child(-n + 2),
  td:nth-child(-n + 2),
  th:last-child,
  td:last-child {
    text-align: left;
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  [hidden] {
    display: none !important;
  }
`);

const TABLE_COLUMNS = Object.freeze([
  ["time", "Time"],
  ["level", "Level"],
  ["loss", "Loss"],
  ["rtt", "Round trip"],
  ["jitter", "Jitter", "jitter"],
  ["cushion", "Cushion", "cushion"],
  ["underruns", "Underruns"],
  ["timeouts", "Timeouts"],
  ["recovered", "Recovered", "recovered"],
  ["note", "Note"],
]);

function svgElement(name, attributes = {}, text = null) {
  const element = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) {
    element.setAttribute(key, String(value));
  }
  if (text !== null) {
    element.textContent = text;
  }
  return element;
}

class MlLinkHealth extends HTMLElement {
  #data = null;
  #parts;
  #layout = null;
  #width = 0;
  #hoverSeq = null;
  #resize;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<section class="panel" aria-labelledby="title">
        <div class="head">
          <h2 id="title"></h2>
          <div class="grow"></div>
          <button type="button" class="close">Close</button>
        </div>
        <div class="ranges" role="group" aria-label="Range"></div>
        <p class="summary"><ml-link-state></ml-link-state><span class="summary-text"></span></p>
        <p class="message" role="alert" hidden></p>
        <p class="empty" hidden></p>
        <div class="chart">
          <svg tabindex="0" role="img" aria-describedby="description">
            <g class="data"></g>
            <line class="crosshair" visibility="hidden"></line>
          </svg>
          <div class="readout" hidden></div>
          <p id="description" hidden></p>
        </div>
        <p class="note" hidden></p>
        <p class="note recovered-note" hidden></p>
        <p class="key">
          A line per measure, one point a second. Shaded: seconds with no report. A vertical line:
          the phone reconnected. A tick: an underrun or a timeout in that second.
        </p>
        <div>
          <button type="button" class="table-toggle" aria-expanded="false" aria-controls="table">Show as table</button>
        </div>
        <div class="table-wrap" id="table" hidden>
          <table>
            <thead><tr></tr></thead>
            <tbody></tbody>
          </table>
        </div>
      </section>`,
    );
    this.#parts = {
      title: root.querySelector("h2"),
      close: root.querySelector(".close"),
      ranges: root.querySelector(".ranges"),
      level: root.querySelector("ml-link-state"),
      summary: root.querySelector(".summary-text"),
      message: root.querySelector(".message"),
      empty: root.querySelector(".empty"),
      chart: root.querySelector(".chart"),
      svg: root.querySelector("svg"),
      plot: root.querySelector(".data"),
      crosshair: root.querySelector(".crosshair"),
      readout: root.querySelector(".readout"),
      description: root.querySelector("#description"),
      note: root.querySelector(".note"),
      recoveredNote: root.querySelector(".recovered-note"),
      toggle: root.querySelector(".table-toggle"),
      tableWrap: root.querySelector(".table-wrap"),
      tableHead: root.querySelector("thead tr"),
      tableBody: root.querySelector("tbody"),
    };
    const parts = this.#parts;
    parts.close.addEventListener("click", () => emit(this, "link-close", {}));
    parts.toggle.addEventListener("click", () =>
      emit(this, "link-table", { shown: parts.tableWrap.hidden }),
    );
    parts.svg.addEventListener("pointermove", (event) => {
      const box = parts.svg.getBoundingClientRect();
      this.#hoverAt(event.clientX - box.left);
    });
    parts.svg.addEventListener("pointerleave", () => {
      if (this.shadowRoot.activeElement !== parts.svg) {
        this.#hide();
      }
    });
    parts.svg.addEventListener("focus", () => this.#step(0, true));
    parts.svg.addEventListener("blur", () => this.#hide());
    parts.svg.addEventListener("keydown", (event) => this.#onKey(event));
    this.#resize = new ResizeObserver(() => {
      const width = Math.round(parts.chart.clientWidth);
      if (width > 0 && width !== this.#width) {
        this.#width = width;
        this.#drawChart();
      }
    });
  }

  connectedCallback() {
    this.#resize.observe(this.#parts.chart);
  }

  disconnectedCallback() {
    this.#resize.disconnect();
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    this.hidden = data === null;
    if (data === null) {
      this.#hide();
      return;
    }
    const parts = this.#parts;
    setText(parts.title, data.title);
    reflect(parts.close, "aria-label", `Close the link health of ${data.title.split(" · ")[0]}`);
    this.#renderRanges(data.ranges);
    reflect(parts.level, "level", data.summary.level);
    reflect(parts.level, "label", data.summary.label);
    setText(parts.summary, data.summary.text);
    parts.message.hidden = data.message === null;
    setText(parts.message, data.message ?? "");
    parts.empty.hidden = data.empty === null;
    setText(parts.empty, data.empty ?? "");
    parts.chart.hidden = data.empty !== null;
    setText(parts.description, data.description);
    reflect(parts.svg, "aria-label", data.title);
    parts.note.hidden = data.extrasNote === null;
    setText(parts.note, data.extrasNote ?? "");
    parts.recoveredNote.hidden = data.recoveredNote === null || data.recoveredNote === undefined;
    setText(parts.recoveredNote, data.recoveredNote ?? "");
    const shown = data.table !== null;
    parts.tableWrap.hidden = !shown;
    reflect(parts.toggle, "aria-expanded", shown ? "true" : "false");
    setText(parts.toggle, shown ? "Hide the table" : "Show as table");
    if (shown) {
      this.#renderTable(data.table, data.extras);
    }
    if (this.#width === 0) {
      this.#width = Math.round(parts.chart.clientWidth);
    }
    this.#drawChart();
  }

  #renderRanges(ranges) {
    const container = this.#parts.ranges;
    if (container.children.length !== ranges.length) {
      container.replaceChildren(
        ...ranges.map((range) => {
          const button = document.createElement("button");
          button.type = "button";
          button.addEventListener("click", () => emit(this, "link-range", { rangeMs: range.value }));
          return button;
        }),
      );
    }
    ranges.forEach((range, index) => {
      const button = container.children[index];
      setText(button, range.label);
      reflect(button, "aria-pressed", range.pressed ? "true" : "false");
      reflect(button, "aria-label", `Last ${range.label}`);
    });
  }

  #renderTable(rows, extras) {
    const columns = TABLE_COLUMNS.filter(([, , extra]) => !extra || extras[extra]);
    const head = columns.map(([, label]) => label).join("|");
    if (this.#parts.tableHead.dataset.columns !== head) {
      this.#parts.tableHead.dataset.columns = head;
      this.#parts.tableHead.replaceChildren(
        ...columns.map(([, label]) => {
          const th = document.createElement("th");
          th.scope = "col";
          th.textContent = label;
          return th;
        }),
      );
    }
    this.#parts.tableBody.replaceChildren(
      ...rows.map((row) => {
        const tr = document.createElement("tr");
        for (const [field] of columns) {
          const td = document.createElement("td");
          td.textContent = row[field];
          tr.append(td);
        }
        return tr;
      }),
    );
  }

  #drawChart() {
    const data = this.#data;
    if (!data || data.empty !== null || this.#width <= 0) {
      return;
    }
    const layout = healthLayout(data.window, {
      width: this.#width,
      extras: data.extras,
      rangeMs: data.rangeMs,
      timeZone: data.timeZone,
    });
    this.#layout = layout;
    const { svg, plot } = this.#parts;
    svg.setAttribute("viewBox", `0 0 ${layout.width} ${layout.height}`);
    svg.setAttribute("height", String(layout.height));
    const marks = [];
    const right = layout.plot.left + layout.plot.width;
    for (const panel of layout.panels) {
      marks.push(svgElement("text", { class: "title", x: layout.plot.left, y: panel.titleY }, panel.title));
      for (const [index, tick] of panel.ticks.entries()) {
        marks.push(
          svgElement("line", {
            class: index === 0 ? "baseline" : "grid",
            x1: layout.plot.left,
            x2: right,
            y1: tick.y,
            y2: tick.y,
          }),
          svgElement(
            "text",
            { class: "tick", x: layout.plot.left - 8, y: tick.y + 4, "text-anchor": "end" },
            tick.label,
          ),
        );
      }
      for (const band of layout.gaps) {
        marks.push(
          svgElement("rect", { class: "gap", x: band.x, y: panel.top, width: band.width, height: panel.height }),
        );
      }
      if (panel.path) {
        marks.push(svgElement("path", { class: "line", d: panel.path }));
      }
      for (const dot of panel.dots) {
        marks.push(svgElement("circle", { class: "dot", cx: dot.x, cy: dot.y, r: 4 }));
      }
    }
    for (const lane of layout.lanes) {
      const middle = lane.top + lane.height / 2;
      marks.push(
        svgElement(
          "text",
          { class: "lane-label", x: layout.plot.left - 8, y: middle + 4, "text-anchor": "end" },
          lane.label,
        ),
        svgElement("line", { class: "grid", x1: layout.plot.left, x2: right, y1: middle, y2: middle }),
      );
      for (const mark of lane.marks) {
        marks.push(
          svgElement("rect", { class: "event", x: mark.x - 1, y: middle - 6, width: 2, height: 12, rx: 1 }),
        );
      }
    }
    for (const reconnect of layout.reconnects) {
      marks.push(
        svgElement("line", {
          class: "reconnect",
          x1: reconnect.x,
          x2: reconnect.x,
          y1: layout.plot.top - 2,
          y2: layout.plot.bottom,
        }),
      );
      if (reconnect.label) {
        marks.push(
          svgElement(
            "text",
            {
              class: "reconnect-label",
              x: reconnect.anchor === "end" ? reconnect.x - 4 : reconnect.x + 4,
              y: layout.plot.top - 4,
              "text-anchor": reconnect.anchor,
            },
            reconnect.label,
          ),
        );
      }
    }
    for (const tick of layout.timeTicks) {
      marks.push(
        svgElement("text", { class: "axis", x: tick.x, y: layout.plot.axisY, "text-anchor": tick.anchor }, tick.label),
      );
    }
    plot.replaceChildren(...marks);
    plot.classList.toggle("dense", data.window.samples.length > layout.plot.width / 2);
    this.#showSeq(this.#hoverSeq);
  }

  #hoverAt(x) {
    if (!this.#layout || !this.#data) {
      return;
    }
    const index = nearestIndex(this.#data.window, this.#layout, x);
    if (index < 0) {
      this.#hide();
      return;
    }
    this.#showSeq(this.#data.window.samples[index].seq);
  }

  #onKey(event) {
    const moves = { ArrowLeft: -1, ArrowRight: 1, PageUp: -10, PageDown: 10 };
    if (event.key in moves) {
      event.preventDefault();
      this.#step(moves[event.key], false);
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const samples = this.#data?.window.samples ?? [];
      const sample = event.key === "Home" ? samples[0] : samples.at(-1);
      this.#showSeq(sample?.seq ?? null);
    } else if (event.key === "Escape") {
      this.#hide();
    }
  }

  // Moves the readout `by` seconds; with nothing shown yet, it starts at the newest second.
  #step(by, keep) {
    const samples = this.#data?.window.samples ?? [];
    if (samples.length === 0) {
      return;
    }
    const at = samples.findIndex((sample) => sample.seq === this.#hoverSeq);
    if (at < 0) {
      this.#showSeq(samples.at(-1).seq);
      return;
    }
    if (!keep) {
      const next = Math.min(samples.length - 1, Math.max(0, at + by));
      this.#showSeq(samples[next].seq);
    }
  }

  #showSeq(seq) {
    const { crosshair, readout } = this.#parts;
    const data = this.#data;
    const layout = this.#layout;
    const sample = seq === null ? undefined : data?.window.samples.find((candidate) => candidate.seq === seq);
    if (!sample || !layout) {
      this.#hide();
      return;
    }
    this.#hoverSeq = seq;
    const x =
      layout.plot.left +
      ((sample.monoMs - layout.domain.startMs) / layout.domain.spanMs) * layout.plot.width;
    crosshair.setAttribute("x1", String(x));
    crosshair.setAttribute("x2", String(x));
    crosshair.setAttribute("y1", String(layout.plot.top));
    crosshair.setAttribute("y2", String(layout.plot.bottom));
    crosshair.setAttribute("visibility", "visible");
    const tip = sampleTooltip(sample, { extras: data.extras, timeZone: data.timeZone });
    const head = document.createElement("div");
    head.className = "readout-head";
    const title = document.createElement("span");
    title.className = "readout-title";
    title.textContent = tip.title;
    const time = document.createElement("span");
    time.className = "readout-time";
    time.textContent = tip.time;
    head.append(title, time);
    const lines = tip.lines.map(([label, value]) => {
      const line = document.createElement("div");
      line.className = "readout-line";
      const name = document.createElement("span");
      name.textContent = label;
      const strong = document.createElement("strong");
      strong.textContent = value;
      line.append(name, strong);
      return line;
    });
    const nodes = [head, ...lines];
    if (tip.note) {
      const note = document.createElement("div");
      note.className = "readout-note";
      note.textContent = tip.note;
      nodes.push(note);
    }
    readout.replaceChildren(...nodes);
    readout.hidden = false;
    // Beside the crosshair, on the side with room.
    const width = readout.offsetWidth;
    const left = x + 12 + width > layout.width ? x - 12 - width : x + 12;
    readout.style.left = `${Math.max(0, left)}px`;
    readout.style.top = `${layout.plot.top}px`;
  }

  #hide() {
    this.#hoverSeq = null;
    this.#parts.crosshair.setAttribute("visibility", "hidden");
    this.#parts.readout.hidden = true;
  }
}

define("ml-link-health", MlLinkHealth);
