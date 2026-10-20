// The link health panel's logic (link-health-view.js): following a musician's history with a
// cursor, the window of the chosen range, the chart's geometry (breaks at gaps, reconnects, event
// marks, scales and the shared time axis), the summary line, the tooltip and the table.
import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_RANGE_MS,
  LINK_RANGES,
  createHistoryFollower,
  cursorOf,
  extrasReported,
  formatClock,
  healthLayout,
  historyTable,
  historyUpdate,
  linkHealthView,
  linkSummary,
  musicianKey,
  nearestIndex,
  niceMax,
  sampleTooltip,
  windowOf,
} from "../app/lib/link-health-view.js";

const UTC = "UTC";
const T0 = Date.UTC(2026, 9, 18, 21, 30, 0);

/** A clean second `seq` (its monotonic time `seq` seconds), with `changes` on top. */
function sample(seq, changes = {}) {
  return {
    seq,
    monoMs: seq * 1000,
    atMs: T0 + seq * 1000,
    gap: false,
    reconnected: false,
    health: "good",
    intervalMs: 1000,
    packetsReceived: 199,
    packetsLost: 1,
    lossPercent: 0.5,
    underruns: 0,
    receiveTimeouts: 0,
    rttMs: 10,
    jitterMs: 2,
    cushionMs: 40,
    backlogMs: 35,
    recovered: null,
    ...changes,
  };
}

function gap(seq) {
  return sample(seq, {
    gap: true,
    health: "unknown",
    intervalMs: null,
    packetsReceived: null,
    packetsLost: null,
    lossPercent: null,
    underruns: null,
    receiveTimeouts: null,
    jitterMs: null,
    cushionMs: null,
    backlogMs: null,
  });
}

function seconds(from, to, make = sample) {
  const list = [];
  for (let seq = from; seq <= to; seq += 1) {
    list.push(make(seq));
  }
  return list;
}

const read = (historyId, samples) => ({ musicianKey: "client:a", historyId, samples, maxSamples: 600, periodMs: 1000 });

const MUSICIAN = {
  address: "192.168.1.12:50000",
  clientId: "a",
  name: "Bass",
  controlConnected: true,
  linkHealth: "warning",
  lossPercent: 0.8,
  underrunsPerSecond: 0.2,
  receiveTimeoutsPerSecond: 0,
  packetsPerSecond: 200,
  capabilities: ["stats.v2"],
};

test("a musician's key is their phone's id, or their address", () => {
  assert.equal(musicianKey({ clientId: "a", address: "1.2.3.4:5" }), "client:a");
  assert.equal(musicianKey({ clientId: null, address: "1.2.3.4:5" }), "address:1.2.3.4:5");
});

test("the ranges are 1, 5 and 10 minutes, ten by default", () => {
  assert.deepEqual(
    LINK_RANGES.map((range) => range.value),
    [60_000, 300_000, 600_000],
  );
  assert.equal(DEFAULT_RANGE_MS, 600_000);
});

test("new samples are appended after the cursor, bounded to the ring", () => {
  const first = historyUpdate(null, read(7, seconds(1, 600)), null);
  assert.equal(first.refetch, false);
  assert.equal(first.buffer.samples.length, 600);
  assert.equal(cursorOf(first.buffer), 600);
  assert.equal(cursorOf(null), null);

  const next = historyUpdate(first.buffer, read(7, seconds(601, 603)), 600);
  assert.equal(next.buffer.samples.length, 600, "never more than the ring");
  assert.equal(next.buffer.samples[0].seq, 4, "the oldest went");
  assert.equal(cursorOf(next.buffer), 603);
  assert.equal(next.changed, true);

  const repeated = historyUpdate(next.buffer, read(7, seconds(602, 603)), 601);
  assert.equal(repeated.buffer.samples.length, 600, "a sample already held is not added twice");
  assert.equal(repeated.changed, false);
});

test("another history id (an engine restart) asks for the whole history again", () => {
  const held = historyUpdate(null, read(7, seconds(1, 50)), null).buffer;

  const partial = historyUpdate(held, read(8, seconds(3, 4)), 50);
  assert.equal(partial.refetch, true, "after the cursor of another history is not the whole of it");
  assert.equal(partial.buffer, held, "kept until the whole one arrives");

  const whole = historyUpdate(held, read(8, seconds(1, 4)), null);
  assert.equal(whole.refetch, false);
  assert.deepEqual(
    whole.buffer.samples.map((s) => s.seq),
    [1, 2, 3, 4],
  );
  assert.equal(whole.buffer.historyId, 8);
});

test("the follower reads once, then only what is new, every interval, and stops", async () => {
  const calls = [];
  let scheduled = null;
  const answers = [read(1, seconds(1, 3)), read(1, seconds(4, 4)), read(2, seconds(9, 9)), read(2, seconds(1, 9))];
  const changes = [];
  const follower = createHistoryFollower({
    read: async (key, since) => {
      calls.push([key, since]);
      return answers.shift();
    },
    onChange: (buffer, error) => changes.push([buffer?.samples.length ?? 0, error]),
    schedule: (run, ms) => {
      scheduled = { run, ms };
      return 1;
    },
    cancel: () => {
      scheduled = null;
    },
  });

  await follower.follow("client:a");
  assert.equal(scheduled.ms, 1000);
  await scheduled.run();
  await scheduled.run();

  assert.deepEqual(calls, [
    ["client:a", null],
    ["client:a", 3],
    ["client:a", 4],
    ["client:a", null],
  ]);
  assert.deepEqual(changes, [
    [3, null],
    [4, null],
    [9, null],
  ]);

  follower.stop();
  assert.equal(scheduled, null, "no more reads");
});

test("a failed read is said and the follower keeps trying with what it had", async () => {
  let scheduled = null;
  let fail = false;
  const changes = [];
  const follower = createHistoryFollower({
    read: async () => {
      if (fail) {
        throw "the engine is not running";
      }
      return read(1, seconds(1, 2));
    },
    onChange: (buffer, error) => changes.push([buffer?.samples.length ?? 0, error]),
    schedule: (run) => {
      scheduled = run;
      return 1;
    },
    cancel: () => {},
  });

  await follower.follow("client:a");
  fail = true;
  await scheduled();

  assert.deepEqual(changes.at(-1), [2, "the engine is not running"]);
  assert.notEqual(scheduled, null, "tries again");
});

test("following another musician drops the first one's history and ignores a late answer", async () => {
  let release;
  const changes = [];
  const follower = createHistoryFollower({
    read: (key) =>
      key === "client:a"
        ? new Promise((resolve) => {
            release = () => resolve(read(1, seconds(1, 5)));
          })
        : Promise.resolve(read(2, seconds(1, 2))),
    onChange: (buffer) => changes.push(buffer.historyId),
    schedule: () => 1,
    cancel: () => {},
  });

  const first = follower.follow("client:a");
  await follower.follow("client:b");
  release();
  await first;

  assert.deepEqual(changes, [2], "only the musician followed now");
  assert.equal(follower.buffer.historyId, 2);
});

test("the window is the range up to the newest sample, on the monotonic clock", () => {
  const samples = seconds(1, 600);
  const window = windowOf(samples, 60_000);
  assert.equal(window.endMs, 600_000);
  assert.equal(window.startMs, 540_000);
  assert.equal(window.samples.length, 60, "the newest minute");
  assert.equal(window.samples[0].seq, 541);
  assert.deepEqual(windowOf([], 60_000), { samples: [], startMs: null, endMs: null });
});

test("a nice scale rounds up and never goes below its floor", () => {
  assert.equal(niceMax(0, 1), 1);
  assert.equal(niceMax(0.3, 1), 1);
  assert.equal(niceMax(1.3, 1), 2);
  assert.equal(niceMax(3.1, 1), 5);
  assert.equal(niceMax(23, 10), 25);
  assert.equal(niceMax(61, 10), 100);
});

test("the lines break at gaps and reconnects, never drawn across them", () => {
  const samples = [
    ...seconds(1, 10),
    ...seconds(11, 13, gap),
    ...seconds(14, 20),
    sample(40, { reconnected: true }),
    ...seconds(41, 45),
  ];
  const layout = healthLayout(windowOf(samples, 60_000), { width: 672, extras: { jitter: true, cushion: true } });

  const loss = layout.panels.find((panel) => panel.id === "loss");
  const moves = loss.path.match(/M/g).length;
  assert.equal(moves, 3, "three runs: before the gap, after it, after the reconnect");
  assert.equal(layout.gaps.length, 1, "one band of seconds without reports");
  assert.ok(layout.gaps[0].width > 0);
  assert.equal(layout.reconnects.length, 1);
  assert.equal(layout.reconnects[0].label, "Reconnected");
  // The plot spans the range: 60 s over the plot's width.
  const { plot } = layout;
  assert.equal(plot.left + plot.width, 672 - 12);
  const reconnect = layout.reconnects[0].x;
  assert.ok(Math.abs(reconnect - (plot.left + plot.width * ((40_000 - (45_000 - 60_000)) / 60_000))) < 0.01);
});

test("an isolated second between gaps is a dot, a value missing in a sample breaks the line", () => {
  const samples = [...seconds(1, 3, gap), sample(4), ...seconds(5, 7, gap), sample(8), sample(9, { rttMs: null }), sample(10)];
  const layout = healthLayout(windowOf(samples, 60_000), { width: 600, extras: { jitter: true, cushion: true } });

  const loss = layout.panels.find((panel) => panel.id === "loss");
  assert.equal(loss.dots.length, 1, "the second alone");
  const rtt = layout.panels.find((panel) => panel.id === "rtt");
  assert.equal(rtt.dots.length, 3, "4, 8 and 10 stand alone");
  assert.equal(rtt.path, "", "no two neighbours to join");
});

test("the panels are small multiples on one time axis: loss, round trip, then the phone's extras", () => {
  const samples = seconds(1, 30, (seq) => sample(seq, { lossPercent: seq === 10 ? 3.2 : 0, rttMs: 12 }));
  const window = windowOf(samples, 60_000);

  const withExtras = healthLayout(window, { width: 600, extras: { jitter: true, cushion: true } });
  assert.deepEqual(
    withExtras.panels.map((panel) => panel.id),
    ["loss", "rtt", "jitter", "cushion"],
  );
  const loss = withExtras.panels[0];
  assert.equal(loss.title, "Loss (%)");
  assert.deepEqual(
    loss.ticks.map((tick) => tick.label),
    ["0", "5"],
  );
  for (const panel of withExtras.panels.slice(1)) {
    assert.ok(panel.top > withExtras.panels[0].top, "stacked, each with its own scale");
  }

  const older = healthLayout(window, { width: 600, extras: { jitter: false, cushion: false } });
  assert.deepEqual(
    older.panels.map((panel) => panel.id),
    ["loss", "rtt"],
  );
  assert.ok(older.height < withExtras.height);
});

test("underruns and timeouts are event marks in their own lanes", () => {
  const samples = seconds(1, 20, (seq) =>
    sample(seq, { underruns: seq % 5 === 0 ? 2 : 0, receiveTimeouts: seq === 7 ? 1 : 0 }),
  );
  const layout = healthLayout(windowOf(samples, 60_000), { width: 600, extras: { jitter: false, cushion: false } });

  const [underruns, timeouts] = layout.lanes;
  assert.equal(underruns.label, "Underruns");
  assert.equal(underruns.marks.length, 4);
  assert.equal(underruns.marks[0].count, 2);
  assert.equal(timeouts.label, "Timeouts");
  assert.equal(timeouts.marks.length, 1);
  assert.ok(timeouts.top > underruns.top);
});

test("the time axis counts back from the newest second and ends at its local time", () => {
  const samples = seconds(1, 600);
  const ten = healthLayout(windowOf(samples, 600_000), {
    width: 800,
    extras: { jitter: false, cushion: false },
    rangeMs: 600_000,
    timeZone: UTC,
  });
  assert.deepEqual(
    ten.timeTicks.map((tick) => tick.label),
    ["−10 min", "−8 min", "−6 min", "−4 min", "−2 min", "21:40:00"],
  );
  const one = healthLayout(windowOf(samples, 60_000), {
    width: 800,
    extras: { jitter: false, cushion: false },
    rangeMs: 60_000,
    timeZone: UTC,
  });
  assert.deepEqual(
    one.timeTicks.map((tick) => tick.label),
    ["−60 s", "−45 s", "−30 s", "−15 s", "21:40:00"],
  );
  assert.equal(one.timeTicks.at(-1).anchor, "end", "the newest label stays inside the chart");
});

test("a reconnect label near the right edge reads leftwards, and close ones are not labelled twice", () => {
  const samples = [...seconds(1, 50), sample(58, { reconnected: true }), sample(59, { reconnected: true }), sample(60)];
  const layout = healthLayout(windowOf(samples, 60_000), { width: 600, extras: { jitter: false, cushion: false } });

  assert.equal(layout.reconnects.length, 2, "both are marked");
  assert.equal(layout.reconnects[0].anchor, "end");
  assert.equal(layout.reconnects[1].label, null, "too close to the last label");
});

test("the hover finds the nearest second to the pointer", () => {
  const window = windowOf(seconds(1, 60), 60_000);
  const layout = healthLayout(window, { width: 612, extras: { jitter: false, cushion: false } });
  const { left, width } = layout.plot;

  assert.equal(nearestIndex(window, layout, left + width), 59, "the newest");
  assert.equal(nearestIndex(window, layout, left + width - width / 60 - 1), 58);
  assert.equal(nearestIndex(window, layout, left - 40), -1, "outside the plot");
  assert.equal(nearestIndex({ samples: [], startMs: null, endMs: null }, layout, left), -1);
});

test("the summary gives the level, the window's loss, the round trip now and on average, and the underruns", () => {
  const samples = [
    ...seconds(1, 8, (seq) => sample(seq, { packetsReceived: 198, packetsLost: 2, rttMs: 10 })),
    sample(9, { packetsReceived: 196, packetsLost: 4, underruns: 1, rttMs: 20 }),
    ...seconds(10, 11, gap),
    sample(12, { reconnected: true, packetsReceived: 200, packetsLost: 0, underruns: 2, receiveTimeouts: 1, rttMs: 30 }),
  ];
  const summary = linkSummary(windowOf(samples, 300_000), 300_000, MUSICIAN);

  assert.equal(summary.level, "warning");
  assert.equal(summary.label, "Warning");
  assert.deepEqual(summary.parts, [
    "Loss 1.00% over 5 min",
    "Round trip 30.0 ms now, 13.0 ms on average",
    "3 underruns in 5 min",
    "1 timeout",
    "2 s without reports",
    "1 reconnect",
  ]);
  assert.equal(summary.text, summary.parts.join(" · "));
});

test("the level is the one the musicians table shows", () => {
  const lost = { ...MUSICIAN, linkHealth: "bad", lossPercent: null, underrunsPerSecond: null };
  const summary = linkSummary(windowOf([sample(1)], 60_000), 60_000, lost);
  assert.deepEqual([summary.level, summary.label], ["bad", "Link lost"]);
});

test("a quiet window says so in the summary", () => {
  const samples = seconds(1, 5, (seq) => sample(seq, { packetsLost: 0, packetsReceived: 200, rttMs: null }));
  const summary = linkSummary(windowOf(samples, 60_000), 60_000, { ...MUSICIAN, linkHealth: "good", underrunsPerSecond: 0 });

  assert.deepEqual(summary.parts, ["Loss 0.00% over 1 min", "Round trip not measured yet", "No underruns in 1 min"]);
  assert.deepEqual(linkSummary(windowOf([], 60_000), 60_000, MUSICIAN).parts, ["No link reports yet"]);
});

test("the tooltip leads with the values and the sample's local time", () => {
  const tip = sampleTooltip(sample(5, { underruns: 1, reconnected: true }), { extras: { jitter: true, cushion: true }, timeZone: UTC });
  assert.equal(tip.time, "21:30:05");
  assert.equal(tip.title, "Good");
  assert.deepEqual(tip.lines, [
    ["Loss", "0.50%"],
    ["Round trip", "10.0 ms"],
    ["Jitter", "2.0 ms"],
    ["Cushion", "40.0 ms"],
    ["Underruns", "1"],
    ["Timeouts", "0"],
  ]);
  assert.equal(tip.note, "Reconnected: the time before it went unmeasured");

  const missing = sampleTooltip(gap(6), { extras: { jitter: false, cushion: false }, timeZone: UTC });
  assert.equal(missing.title, "No report");
  assert.deepEqual(missing.lines, [["Round trip", "10.0 ms"]]);
  assert.equal(missing.note, "No report arrived for this second");
});

test("the table lists the window's seconds, the newest first", () => {
  const rows = historyTable(windowOf([sample(1), gap(2), sample(3, { jitterMs: null })], 60_000), {
    extras: { jitter: true, cushion: true },
    timeZone: UTC,
  });
  assert.equal(rows.length, 3);
  assert.deepEqual(rows[0], {
    key: 3,
    time: "21:30:03",
    level: "Good",
    loss: "0.50%",
    rtt: "10.0 ms",
    jitter: "—",
    cushion: "40.0 ms",
    underruns: "0",
    timeouts: "0",
    note: "",
  });
  assert.equal(rows[1].level, "No report");
  assert.equal(rows[1].note, "Gap");
});

test("jitter and cushion are shown for a phone that announces stats.v2 or reports them", () => {
  assert.deepEqual(extrasReported({ capabilities: ["stats.v2"] }, []), { jitter: true, cushion: true });
  assert.deepEqual(extrasReported({ capabilities: [] }, [sample(1, { jitterMs: null, cushionMs: null })]), {
    jitter: false,
    cushion: false,
  });
  assert.deepEqual(extrasReported({ capabilities: [] }, [sample(1, { cushionMs: null })]), {
    jitter: true,
    cushion: false,
  });
});

test("the panel's view: a title with the range, the range buttons, the summary and a note for missing extras", () => {
  const buffer = { historyId: 1, samples: seconds(1, 30, (seq) => sample(seq, { jitterMs: null, cushionMs: null })) };
  const view = linkHealthView({
    musician: { ...MUSICIAN, capabilities: [] },
    buffer,
    rangeMs: 300_000,
    error: null,
    showTable: false,
    timeZone: UTC,
  });

  assert.equal(view.title, "Bass · last 5 minutes");
  assert.deepEqual(
    view.ranges.map((range) => [range.label, range.pressed]),
    [
      ["1 min", false],
      ["5 min", true],
      ["10 min", false],
    ],
  );
  assert.equal(view.summary.level, "warning");
  assert.equal(view.extrasNote, "Jitter and cushion: not reported by this phone (it needs a newer app).");
  assert.equal(view.table, null, "only when asked for");
  assert.equal(view.message, null);
  assert.ok(view.description.startsWith("Link health of Bass over the last 5 minutes: Warning. "));

  const failing = linkHealthView({
    musician: MUSICIAN,
    buffer: null,
    rangeMs: 60_000,
    error: "the engine is not running",
    showTable: true,
    timeZone: UTC,
  });
  assert.equal(failing.message, "The link history could not be read: the engine is not running.");
  assert.equal(failing.extrasNote, null);
  assert.deepEqual(failing.table, []);
  assert.equal(failing.empty, "No link reports from this phone yet: the chart starts with its first report.");
});

test("a clock time is local to the given zone, to the second", () => {
  assert.equal(formatClock(T0 + 65_000, UTC), "21:31:05");
});
