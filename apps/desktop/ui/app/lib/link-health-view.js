// What the link health panel of the Network and system screen shows, worked out from a musician's
// link history (the `link_history` command, see bridge.js). Pure functions, no DOM, plus one small
// follower that reads the history once and then only what is new.
//
// - The history is followed by its cursor: the newest `seq` held is passed as `sinceSeq`, and only
//   the samples after it come back, appended to at most the ring's size. A `historyId` not seen
//   before is a new history (the engine restarted, or another phone took the row over): what was
//   held goes and the history is read whole.
// - The chart draws on the monotonic clock (`monoMs`), never on the wall clock (`atMs`, shown as
//   local time in the axis's last label, the tooltip and the table): the window is the chosen range
//   up to the newest sample.
// - Loss and round trip, then the phone's jitter and cushion when it reports them, are small
//   multiples, each with its own scale, on one shared time axis: never two scales on one chart.
//   A line never crosses a gap (a second with no report), a missing value or a reconnect: it
//   breaks there, and a second standing alone is a dot. Underruns and timeouts are event marks in
//   their own lanes; a reconnect is a vertical marker with a label.

import { linkState } from "./format.js";

/** The ranges the panel offers, `value` in milliseconds. */
export const LINK_RANGES = Object.freeze([
  Object.freeze({ value: 60_000, label: "1 min", long: "last minute" }),
  Object.freeze({ value: 300_000, label: "5 min", long: "last 5 minutes" }),
  Object.freeze({ value: 600_000, label: "10 min", long: "last 10 minutes" }),
]);

/** The range the panel opens with: the whole ring. */
export const DEFAULT_RANGE_MS = 600_000;

/** How often an open panel asks for what is new, in milliseconds: one sample's period. */
export const HISTORY_POLL_MS = 1000;

/** How many samples a history keeps when the answer does not say. */
const RING_SAMPLES = 600;

/** The time one sample stands for, in milliseconds. */
const PERIOD_MS = 1000;

const DASH = "—";
const MINUS = "−";

const LEVEL_WORDS = Object.freeze({
  good: "Good",
  warning: "Warning",
  bad: "Poor",
  unknown: "No report",
});

/** The key the desk names a musician by: their phone's id, or their address. */
export function musicianKey(musician) {
  return musician.clientId ? `client:${musician.clientId}` : `address:${musician.address}`;
}

/**
 * The listed musician the panel follows by `key`, last seen at `address`, or null. A phone listed by
 * its address that then registers is keyed by its client id from then on: when an `address:` key
 * is gone, the row at that same address that now has a client id is the same musician. A phone
 * that already had a client id is never followed to another one at its old address.
 */
export function followedMusician(musicians, key, address) {
  if (key === null) {
    return null;
  }
  const same = musicians.find((musician) => musicianKey(musician) === key);
  if (same || !key.startsWith("address:")) {
    return same ?? null;
  }
  return musicians.find((musician) => musician.clientId && musician.address === address) ?? null;
}

/** The cursor to ask with for what is new: the newest `seq` held, or null to read the whole. */
export function cursorOf(buffer) {
  return buffer?.samples.at(-1)?.seq ?? null;
}

/**
 * What `read`, the answer to a read made with the cursor `askedSince`, does to the history held in
 * `buffer` (`{ historyId, samples }` or null): `{ buffer, changed, refetch }`. The same history
 * gets the samples after its newest appended, keeping at most the ring's size; another history
 * read with a cursor asks for a whole read (`refetch`), keeping `buffer` until it comes.
 */
export function historyUpdate(buffer, read, askedSince) {
  const same = buffer !== null && buffer.historyId === read.historyId;
  if (!same && askedSince !== null) {
    return { buffer, changed: false, refetch: true };
  }
  const newest = same ? cursorOf(buffer) : null;
  const fresh = read.samples.filter((sample) => newest === null || sample.seq > newest);
  const ring = read.maxSamples ?? RING_SAMPLES;
  const samples = [...(same ? buffer.samples : []), ...fresh].slice(-ring);
  return {
    buffer: { historyId: read.historyId, samples },
    changed: !same || fresh.length > 0,
    refetch: false,
  };
}

/**
 * Follows one musician's history: `follow(key)` reads it whole, then every `intervalMs` only what
 * is new (`read(key, sinceSeq)` answers as `linkHistory` does); `stop()` ends it. `onChange(buffer,
 * error)` hears of every new sample and of each failed read (`error` a sentence fragment, the
 * history kept as it was); a failed read is tried again at the next interval. An answer for a
 * musician no longer followed is dropped.
 */
export function createHistoryFollower({
  read,
  onChange,
  schedule = setTimeout,
  cancel = clearTimeout,
  intervalMs = HISTORY_POLL_MS,
}) {
  let key = null;
  let buffer = null;
  let error = null;
  let timer = null;
  let generation = 0;

  function stop() {
    generation += 1;
    if (timer !== null) {
      cancel(timer);
      timer = null;
    }
  }

  async function poll() {
    const mine = generation;
    timer = null;
    try {
      const since = cursorOf(buffer);
      let update = historyUpdate(buffer, await read(key, since), since);
      if (mine === generation && update.refetch) {
        update = historyUpdate(buffer, await read(key, null), null);
      }
      if (mine !== generation) {
        return;
      }
      const said = error;
      buffer = update.buffer;
      error = null;
      if (update.changed || said !== null) {
        onChange(buffer, null);
      }
    } catch (failure) {
      if (mine !== generation) {
        return;
      }
      const text = String(failure?.message ?? failure);
      if (text !== error) {
        error = text;
        onChange(buffer, error);
      }
    }
    if (mine === generation) {
      timer = schedule(poll, intervalMs);
    }
  }

  return {
    /** Follows the musician `next` names (null for nobody); the same one again changes nothing. */
    follow(next) {
      if (next === key) {
        return Promise.resolve();
      }
      stop();
      key = next;
      buffer = null;
      error = null;
      return key === null ? Promise.resolve() : poll();
    },
    stop() {
      stop();
      key = null;
      buffer = null;
      error = null;
    },
    get buffer() {
      return buffer;
    },
  };
}

/**
 * The samples of `rangeMs` up to the newest one, on the monotonic clock: `{ samples, startMs,
 * endMs }`, the times null when there are none.
 */
export function windowOf(samples, rangeMs) {
  const newest = samples.at(-1);
  if (!newest) {
    return { samples: [], startMs: null, endMs: null };
  }
  const endMs = newest.monoMs;
  const startMs = endMs - rangeMs;
  let first = samples.length - 1;
  while (first > 0 && samples[first - 1].monoMs > startMs) {
    first -= 1;
  }
  return { samples: samples.slice(first), startMs, endMs };
}

const NICE_STEPS = [1, 2, 2.5, 5, 10];

/** The top of a scale for values up to `value`: a round number, never below `floor`. */
export function niceMax(value, floor) {
  const top = Math.max(value, floor);
  const magnitude = 10 ** Math.floor(Math.log10(top));
  const step = NICE_STEPS.find((nice) => nice * magnitude >= top - 1e-9) ?? 10;
  return step * magnitude;
}

// The chart's anatomy, in CSS pixels.
const PAD_LEFT = 72;
const PAD_RIGHT = 12;
const MARKER_ROW = 16;
const TITLE_ROW = 18;
const PANEL_HEIGHT = 56;
const PANEL_GAP = 6;
const LANE_HEIGHT = 18;
const AXIS_HEIGHT = 22;
// How far apart two reconnect labels must be, and how close to the edge one turns leftwards.
const LABEL_ROOM = 90;
// How close to the right edge a relative time tick may come before the newest time's label.
const END_LABEL_ROOM = 64;

const PANELS = Object.freeze([
  Object.freeze({ id: "loss", field: "lossPercent", title: "Loss (%)", floor: 1 }),
  Object.freeze({ id: "rtt", field: "rttMs", title: "Round trip (ms)", floor: 10 }),
  Object.freeze({ id: "jitter", field: "jitterMs", title: "Jitter (ms)", floor: 5, extra: "jitter" }),
  Object.freeze({ id: "cushion", field: "cushionMs", title: "Cushion (ms)", floor: 50, extra: "cushion" }),
]);

const LANES = Object.freeze([
  Object.freeze({ id: "underruns", field: "underruns", label: "Underruns" }),
  Object.freeze({ id: "timeouts", field: "receiveTimeouts", label: "Timeouts" }),
]);

const round1 = (value) => Math.round(value * 10) / 10;

function tickLabel(value) {
  return Number.isInteger(value) ? String(value) : String(round1(value));
}

// The value a panel reads from `sample`, or null where it has none (a gap has none at all).
function valueOf(sample, field) {
  if (sample.gap) {
    return null;
  }
  const value = sample[field];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

// The runs of samples a line joins: broken at a missing value, a reconnect and a missing second.
function runsOf(samples, field) {
  const runs = [];
  let run = [];
  let previous = null;
  for (const sample of samples) {
    const value = valueOf(sample, field);
    const broken =
      value === null ||
      sample.reconnected ||
      (previous !== null && sample.monoMs - previous.monoMs > 1.5 * PERIOD_MS);
    if (broken && run.length > 0) {
      runs.push(run);
      run = [];
    }
    if (value !== null) {
      run.push({ sample, value });
    }
    previous = sample;
  }
  if (run.length > 0) {
    runs.push(run);
  }
  return runs;
}

/**
 * The chart's geometry for `window` (see windowOf) at `width` pixels: `{ width, height, plot,
 * domain, panels, lanes, gaps, reconnects, timeTicks }`. `extras` says which of the phone's own
 * numbers have panels (`{ jitter, cushion }`); `rangeMs` is the window's span and `timeZone` the
 * zone of the newest time's label (the system's by default).
 *
 * - `plot`: `{ left, width, top, bottom }`, the area every panel and lane shares in x.
 * - `panels`: `{ id, title, titleY, top, height, max, ticks: [{ y, label }], path, dots }`: `path`
 *   an SVG path of every run of two or more seconds, `dots` the seconds standing alone.
 * - `lanes`: `{ id, label, top, height, marks: [{ x, count }] }` for underruns and timeouts.
 * - `gaps`: `[{ x, width }]`, the seconds without reports; `reconnects`: `[{ x, label, anchor }]`,
 *   `label` null where an earlier one is too close to label again.
 * - `timeTicks`: `[{ x, label, anchor }]`, counting back from the newest second, the last its
 *   local time.
 */
export function healthLayout(window, { width, extras, rangeMs, timeZone } = {}) {
  const span = rangeMs ?? (window.endMs !== null ? window.endMs - window.startMs : DEFAULT_RANGE_MS);
  const plotWidth = Math.max(1, width - PAD_LEFT - PAD_RIGHT);
  const startMs = window.endMs !== null ? window.endMs - span : 0;
  const xOf = (monoMs) => round1(PAD_LEFT + ((monoMs - startMs) / span) * plotWidth);
  const samples = window.samples;

  let y = MARKER_ROW;
  const panels = [];
  for (const spec of PANELS) {
    if (spec.extra && !extras?.[spec.extra]) {
      continue;
    }
    const top = y + TITLE_ROW;
    const values = samples.map((sample) => valueOf(sample, spec.field)).filter((v) => v !== null);
    const max = niceMax(values.length > 0 ? Math.max(...values) : 0, spec.floor);
    const yOf = (value) => round1(top + PANEL_HEIGHT - (Math.min(value, max) / max) * PANEL_HEIGHT);
    const runs = runsOf(samples, spec.field);
    const path = runs
      .filter((run) => run.length > 1)
      .map((run) =>
        run
          .map((point, index) => `${index === 0 ? "M" : "L"}${xOf(point.sample.monoMs)} ${yOf(point.value)}`)
          .join(""),
      )
      .join("");
    const dots = runs
      .filter((run) => run.length === 1)
      .map(([point]) => ({ x: xOf(point.sample.monoMs), y: yOf(point.value) }));
    panels.push({
      id: spec.id,
      title: spec.title,
      titleY: top - 6,
      top,
      height: PANEL_HEIGHT,
      max,
      ticks: [
        { y: yOf(0), label: "0" },
        { y: yOf(max), label: tickLabel(max) },
      ],
      path,
      dots,
    });
    y = top + PANEL_HEIGHT + PANEL_GAP;
  }

  const lanes = LANES.map((spec, index) => ({
    id: spec.id,
    label: spec.label,
    top: y + index * LANE_HEIGHT,
    height: LANE_HEIGHT,
    marks: samples
      .filter((sample) => (valueOf(sample, spec.field) ?? 0) > 0)
      .map((sample) => ({ x: xOf(sample.monoMs), count: sample[spec.field] })),
  }));
  const bottom = y + LANES.length * LANE_HEIGHT;

  const gaps = [];
  let band = null;
  for (const sample of samples) {
    if (sample.gap && band && sample.seq === band.last.seq + 1) {
      band.last = sample;
    } else if (sample.gap) {
      band = { first: sample, last: sample };
      gaps.push(band);
    } else {
      band = null;
    }
  }
  const gapBands = gaps.map(({ first, last }) => {
    const x0 = Math.max(PAD_LEFT, xOf(first.monoMs - PERIOD_MS));
    return { x: x0, width: round1(Math.max(1, xOf(last.monoMs) - x0)) };
  });

  const reconnects = [];
  let labelled = -Infinity;
  for (const sample of samples.filter((candidate) => candidate.reconnected)) {
    const x = xOf(sample.monoMs);
    const room = x - labelled >= LABEL_ROOM;
    if (room) {
      labelled = x;
    }
    reconnects.push({
      x,
      label: room ? "Reconnected" : null,
      anchor: x > PAD_LEFT + plotWidth - LABEL_ROOM ? "end" : "start",
    });
  }

  const timeTicks = [];
  if (window.endMs !== null) {
    const step = span <= 60_000 ? 15_000 : span <= 300_000 ? 60_000 : 120_000;
    const count = Math.floor(span / step);
    const right = PAD_LEFT + plotWidth;
    for (let k = count; k >= 1; k -= 1) {
      const x = xOf(window.endMs - k * step);
      if (right - x < END_LABEL_ROOM) {
        continue;
      }
      const back = k * step;
      timeTicks.push({
        x,
        label: span <= 60_000 ? `${MINUS}${back / 1000} s` : `${MINUS}${back / 60_000} min`,
        anchor: x <= PAD_LEFT + 0.5 ? "start" : "middle",
      });
    }
    timeTicks.push({ x: right, label: formatClock(samples.at(-1).atMs, timeZone), anchor: "end" });
  }

  return {
    width,
    height: bottom + AXIS_HEIGHT,
    plot: { left: PAD_LEFT, width: plotWidth, top: MARKER_ROW, bottom, axisY: bottom + 15 },
    domain: { startMs, spanMs: span },
    panels,
    lanes,
    gaps: gapBands,
    reconnects,
    timeTicks,
  };
}

/** The index in `window.samples` of the second nearest the pointer at `x`; -1 off the plot. */
export function nearestIndex(window, layout, x) {
  const { samples } = window;
  const { left, width } = layout.plot;
  if (samples.length === 0 || x < left - 8 || x > left + width + 8) {
    return -1;
  }
  const fraction = Math.min(1, Math.max(0, (x - left) / width));
  const monoMs = layout.domain.startMs + fraction * layout.domain.spanMs;
  let low = 0;
  let high = samples.length - 1;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (samples[middle].monoMs < monoMs) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  if (low > 0 && monoMs - samples[low - 1].monoMs < samples[low].monoMs - monoMs) {
    return low - 1;
  }
  return low;
}

const clocks = new Map();

/** `atMs` (Unix milliseconds) as a local time to the second, "21:34:05", in `timeZone`. */
export function formatClock(atMs, timeZone) {
  const zone = timeZone ?? "";
  let clock = clocks.get(zone);
  if (!clock) {
    clock = new Intl.DateTimeFormat("en-GB", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
      ...(timeZone ? { timeZone } : {}),
    });
    clocks.set(zone, clock);
  }
  return clock.format(atMs);
}

function formatMs(value) {
  return typeof value === "number" ? `${round1(value).toFixed(1)} ms` : DASH;
}

function formatLoss(value) {
  return typeof value === "number" ? `${value.toFixed(2)}%` : DASH;
}

function formatCount(value) {
  return typeof value === "number" ? String(value) : DASH;
}

function plural(count, one, many) {
  return `${count} ${count === 1 ? one : many}`;
}

function rangeWords(rangeMs) {
  const range = LINK_RANGES.find((candidate) => candidate.value === rangeMs);
  return range ?? { label: `${Math.round(rangeMs / 60_000)} min`, long: `last ${Math.round(rangeMs / 60_000)} minutes` };
}

/**
 * What the phone reports of its own numbers: `{ jitter, cushion, recovered }`, `recovered` the packets
 * a redundant copy recovered (a phone that announced "redundancy.recovered").
 */
export function extrasReported(musician, samples) {
  const capabilities = musician?.capabilities ?? [];
  const announced = capabilities.includes("stats.v2");
  const some = (field) => samples.some((sample) => valueOf(sample, field) !== null);
  return {
    jitter: announced || some("jitterMs"),
    cushion: announced || some("cushionMs"),
    recovered: capabilities.includes("redundancy.recovered") || some("recovered"),
  };
}

// The link level and word the musicians table shows for `musician` (see linkState in format.js).
function levelOf(musician) {
  const { level, label } = linkState(musician);
  return { level, label };
}

/**
 * The line above the chart: `{ level, label, parts, text }`, the musician's link level now, then
 * the loss over the window, the round trip now and on average, the underruns in the window and,
 * when there were any, the timeouts, the seconds without reports and the reconnects. For a phone
 * that reports recoveries, the packets a redundant copy recovered in the window, said while
 * redundancy is on or when there were any.
 */
export function linkSummary(window, rangeMs, musician) {
  const { level, label } = levelOf(musician);
  const reports = window.samples.filter((sample) => !sample.gap);
  if (window.samples.length === 0) {
    return { level, label, parts: ["No link reports yet"], text: "No link reports yet" };
  }
  const words = rangeWords(rangeMs).label;
  const sum = (field) => reports.reduce((total, sample) => total + (sample[field] ?? 0), 0);
  const lost = sum("packetsLost");
  const expected = lost + sum("packetsReceived");
  const parts = [
    expected > 0 ? `Loss ${formatLoss((lost * 100) / expected)} over ${words}` : `Loss ${DASH} over ${words}`,
  ];
  const trips = reports.map((sample) => sample.rttMs).filter((value) => typeof value === "number");
  const now = window.samples.findLast((sample) => typeof sample.rttMs === "number")?.rttMs;
  if (trips.length > 0 && typeof now === "number") {
    const average = trips.reduce((total, value) => total + value, 0) / trips.length;
    parts.push(`Round trip ${formatMs(now)} now, ${formatMs(average)} on average`);
  } else {
    parts.push("Round trip not measured yet");
  }
  const underruns = sum("underruns");
  parts.push(underruns === 0 ? `No underruns in ${words}` : `${plural(underruns, "underrun", "underruns")} in ${words}`);
  const timeouts = sum("receiveTimeouts");
  if (timeouts > 0) {
    parts.push(plural(timeouts, "timeout", "timeouts"));
  }
  const gaps = window.samples.length - reports.length;
  if (gaps > 0) {
    parts.push(`${gaps} s without reports`);
  }
  const reconnects = window.samples.filter((sample) => sample.reconnected).length;
  if (reconnects > 0) {
    parts.push(plural(reconnects, "reconnect", "reconnects"));
  }
  if (reports.some((sample) => typeof sample.recovered === "number")) {
    const recovered = sum("recovered");
    if (recovered > 0) {
      parts.push(`${plural(recovered, "packet", "packets")} recovered by redundancy in ${words}`);
    } else if (musician?.redundancy?.on) {
      parts.push(`No packets recovered in ${words}`);
    }
  }
  return { level, label, parts, text: parts.join(" · ") };
}

/**
 * The hover readout of `sample`: `{ time, title, lines: [[label, value]], note }`, the values the
 * phone has (the extras only where shown), its local time, and a note for a gap or a reconnect.
 */
export function sampleTooltip(sample, { extras, timeZone } = {}) {
  const time = formatClock(sample.atMs, timeZone);
  if (sample.gap) {
    return {
      time,
      title: LEVEL_WORDS.unknown,
      lines: typeof sample.rttMs === "number" ? [["Round trip", formatMs(sample.rttMs)]] : [],
      note: "No report arrived for this second",
    };
  }
  const lines = [
    ["Loss", formatLoss(sample.lossPercent)],
    ["Round trip", formatMs(sample.rttMs)],
  ];
  if (extras?.jitter) {
    lines.push(["Jitter", formatMs(sample.jitterMs)]);
  }
  if (extras?.cushion) {
    lines.push(["Cushion", formatMs(sample.cushionMs)]);
  }
  lines.push(["Underruns", formatCount(sample.underruns)], ["Timeouts", formatCount(sample.receiveTimeouts)]);
  if (extras?.recovered) {
    lines.push(["Recovered", formatCount(sample.recovered)]);
  }
  return {
    time,
    title: LEVEL_WORDS[sample.health] ?? LEVEL_WORDS.unknown,
    lines,
    note: sample.reconnected ? "Reconnected: the time before it went unmeasured" : null,
  };
}

/** The window's seconds as table rows, the newest first, every value as text. */
export function historyTable(window, { timeZone } = {}) {
  return window.samples
    .slice()
    .reverse()
    .map((sample) => ({
      key: sample.seq,
      time: formatClock(sample.atMs, timeZone),
      level: sample.gap ? LEVEL_WORDS.unknown : (LEVEL_WORDS[sample.health] ?? LEVEL_WORDS.unknown),
      loss: formatLoss(sample.gap ? null : sample.lossPercent),
      rtt: formatMs(sample.rttMs),
      jitter: formatMs(sample.gap ? null : sample.jitterMs),
      cushion: formatMs(sample.gap ? null : sample.cushionMs),
      underruns: formatCount(sample.gap ? null : sample.underruns),
      timeouts: formatCount(sample.gap ? null : sample.receiveTimeouts),
      recovered: formatCount(sample.gap ? null : sample.recovered),
      note: sample.gap ? "Gap" : sample.reconnected ? "Reconnected" : "",
    }));
}

/**
 * The panel's view for `musician` (a status row) and the history held for them, `buffer`:
 * `{ title, ranges, rangeMs, summary, description, window, extras, extrasNote, recoveredNote, table,
 * message, empty, timeZone }`. `recoveredNote` says, while the musician's redundancy is on, that their
 * phone does not report the packets a copy recovered (null otherwise). `table` is the rows only when
 * `showTable`; `message` says why the last read
 * failed; `empty` says why there is no chart yet.
 */
export function linkHealthView({ musician, buffer, rangeMs, error, showTable, timeZone }) {
  const samples = buffer?.samples ?? [];
  const window = windowOf(samples, rangeMs);
  const words = rangeWords(rangeMs);
  const name = musician.name || musician.address;
  const extras = extrasReported(musician, samples);
  const missing = [extras.jitter ? null : "Jitter", extras.cushion ? null : "cushion"].filter(Boolean);
  const extrasNote =
    missing.length === 0
      ? null
      : `${missing.join(" and ").replace(/^c/, "C")}: not reported by this phone (it needs a newer app).`;
  const recoveredNote =
    musician.redundancy?.on && !extras.recovered
      ? "Recovered packets: not reported by this phone (it needs a newer app)."
      : null;
  const summary = linkSummary(window, rangeMs, musician);
  return {
    title: `${name} · ${words.long}`,
    ranges: LINK_RANGES.map((range) => ({ value: range.value, label: range.label, pressed: range.value === rangeMs })),
    rangeMs,
    summary,
    description: `Link health of ${name} over the ${words.long}: ${summary.label}. ${summary.text}.`,
    window,
    extras,
    extrasNote,
    recoveredNote,
    table: showTable ? historyTable(window, { timeZone }) : null,
    message: error ? `The link history could not be read: ${error}.` : null,
    empty:
      window.samples.length === 0
        ? "No link reports from this phone yet: the chart starts with its first report."
        : null,
    timeZone,
  };
}
