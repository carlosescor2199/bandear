import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";
import {
  channelStrips,
  groupStrips,
  masterStrip,
  mixMessages,
  mixNotice,
  musicianTabs,
  topBarView,
} from "../app/lib/mix-view.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const DRUMS = "192.168.1.20:50000";

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

const running = reduce([actions.status(sampleStatus())]);

test("the top bar names the device and format, counts the phones needing attention, offers Stop", () => {
  assert.deepEqual(topBarView(running), {
    phase: "running",
    device: "Demo interface",
    format: "48 kHz · 24-bit · 256",
    attention: 2,
    attentionLabel: "2 phones need attention",
    action: "stop",
  });
});

test("while the engine starts or stops the top bar offers nothing; stopped or failed offers Start", () => {
  const starting = reduce([actions.engine("starting")], running);
  assert.equal(topBarView(starting).action, null);
  assert.equal(topBarView(starting).device, null);
  assert.equal(topBarView(reduce([actions.engine("stopping")], running)).action, null);
  assert.equal(topBarView(initialState).action, "start");
  assert.equal(topBarView(reduce([actions.engine("error", "no device")])).action, "start");

  const one = sampleStatus();
  one.musicians = one.musicians.slice(0, 2);
  assert.equal(topBarView(reduce([actions.status(one)])).attentionLabel, "1 phone needs attention");
  one.musicians = one.musicians.slice(0, 1);
  assert.equal(topBarView(reduce([actions.status(one)])).attentionLabel, null);
});

test("a tab per musician, in the engine's order, with the link in words", () => {
  const tabs = musicianTabs(running);

  assert.deepEqual(
    tabs.map(({ address, label, link, selected }) => [address, label, link.label, selected]),
    [
      [LEAD, "Lead vocal", "Good", true],
      [BASS, "Bass", "Warning", false],
      ["192.168.1.17:50000", "Keys", "No reports", false],
      [DRUMS, "Drums", "Link lost", false],
    ],
  );
  assert.equal(tabs[1].link.level, "warning");
});

test("a channel strip per input, named by number, with its group, levels and buttons", () => {
  const strips = channelStrips(running, LEAD);

  assert.equal(strips.length, 6);
  assert.deepEqual(strips[0], {
    index: 0,
    label: "CH 1",
    group: { name: "Drums", tone: "teal", invalid: false },
    gain: 62,
    pan: 50,
    muted: false,
    solo: false,
    tone: "default",
    readout: "62%",
  });
  assert.equal(strips[3].group, null, "CH 4 is in no group");
  assert.deepEqual(strips[4].group, { name: "Vocals", tone: "blue", invalid: false });
  assert.equal(strips[5].pan, 40);
});

test("a muted channel's strip says so in words, not only in colour", () => {
  const strip = channelStrips(running, DRUMS)[3];

  assert.equal(strip.muted, true);
  assert.equal(strip.tone, "muted");
  assert.equal(strip.readout, "muted");
});

test("the strips show local edits still on their way", () => {
  const edited = reduce(
    [actions.mixEdit(LEAD, { "channelGains.1": 33, "channelSolo.2": true })],
    running,
  );

  const strips = channelStrips(edited, LEAD);
  assert.equal(strips[1].gain, 33);
  assert.equal(strips[1].readout, "33%");
  assert.equal(strips[2].solo, true);
});

test("a group strip per configured group, in its chip's tone, with the musician's group level", () => {
  const edited = reduce([actions.mixEdit(LEAD, { "groupLevels.1": 70 })], running);

  assert.deepEqual(groupStrips(edited, LEAD), [
    { index: 0, name: "Drums", tone: "teal", invalid: false, level: 100, readout: "100%" },
    { index: 1, name: "Vocals", tone: "blue", invalid: false, level: 70, readout: "70%" },
  ]);
});

test("the master strip carries the musician's level, level cap and mute", () => {
  assert.deepEqual(masterStrip(running, LEAD), {
    volume: 85,
    cap: 90,
    muted: false,
    readout: "85% · cap 90%",
  });
  const muted = reduce([actions.mixEdit(LEAD, { muted: true })], running);
  assert.equal(masterStrip(muted, LEAD).readout, "muted · cap 90%");
});

test("without that musician there is nothing to strip", () => {
  assert.deepEqual(channelStrips(running, "10.0.0.1:50000"), []);
  assert.deepEqual(groupStrips(running, "10.0.0.1:50000"), []);
  assert.equal(masterStrip(running, "10.0.0.1:50000"), null);
});

test("the mix area says why it is empty", () => {
  const nobody = sampleStatus();
  nobody.musicians = [];

  assert.deepEqual(mixNotice(initialState), { kind: "stopped" });
  assert.deepEqual(mixNotice(reduce([actions.engine("starting")])), { kind: "starting" });
  assert.deepEqual(mixNotice(reduce([actions.engine("stopping")], running)), { kind: "stopping" });
  assert.deepEqual(mixNotice(reduce([actions.engine("error", "No input device.")])), {
    kind: "error",
    message: "No input device.",
  });
  assert.deepEqual(mixNotice(reduce([actions.status(nobody)])), { kind: "no-musicians" });
  assert.deepEqual(mixNotice(reduce([actions.status({ ...sampleStatus(), stopped: true })])), {
    kind: "stopped",
  });
  assert.equal(mixNotice(running), null);
});

test("the console says who was removed, and which musician's change was refused", () => {
  const withoutBass = sampleStatus();
  withoutBass.musicians = withoutBass.musicians.filter((musician) => musician.address !== BASS);
  const removed = reduce([actions.select(BASS), actions.status(withoutBass)], running);

  assert.deepEqual(mixMessages(removed), {
    departed: "Bass was removed; showing Lead vocal's mix.",
    error: null,
  });

  const refused = reduce(
    [
      actions.mixEdit(LEAD, { volumePercent: 10 }),
      actions.mixFailed(LEAD, { volumePercent: 10 }, "the engine is not running"),
    ],
    running,
  );
  assert.deepEqual(mixMessages(refused), {
    departed: null,
    error: "A change to Lead vocal's mix was not applied: the engine is not running.",
  });
});
