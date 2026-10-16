import test from "node:test";
import assert from "node:assert/strict";

import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";
import {
  channelEdits,
  channelStrips,
  groupStrips,
  stripLevel,
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

test("a channel strip per input, named as the engineer named it, with its kind, group, levels and buttons", () => {
  const strips = channelStrips(running, LEAD);

  assert.equal(strips.length, 6);
  assert.deepEqual(strips[0], {
    index: 0,
    sides: [0],
    stereo: false,
    label: "Kick",
    kind: "drums",
    group: { id: "drums", name: "Drums", tone: "teal", invalid: false },
    gain: 62,
    pan: 50,
    muted: false,
    solo: false,
    tone: "default",
    readout: "62%",
    locked: false,
    lockText: "Unlocked",
  });
  assert.deepEqual(
    strips.map((strip) => strip.label),
    ["Kick", "Snare", "Overheads", "Bass", "Lead vox", "Backing vox"],
  );
  assert.equal(strips[3].group, null, "CH 4 is in no group");
  assert.deepEqual(strips[4].group, { id: "vocals", name: "Vocals", tone: "blue", invalid: false });
  assert.equal(strips[5].pan, 40);
});

// The sample with Keys on CH 2-3 as a pair panned `pans`, a hidden spare on CH 4 and CH 5 unnamed.
function pairedState(pans = [0, 100]) {
  const status = sampleStatus();
  const details = [
    ["Kick", "drums", false, false],
    ["Keys L", "keys", false, true],
    ["Keys R", "keys", false, false],
    ["Spare", "other", true, false],
    ["", "other", false, false],
    ["Backing vox", "voice", false, false],
  ];
  status.channels = status.channels.map((channel, index) => {
    const [name, kind, hidden, link] = details[index];
    return { ...channel, name, kind, hidden, link };
  });
  const lead = status.musicians[0];
  lead.pans = [50, ...pans, ...lead.pans.slice(3)];
  lead.channelGains = [62, 70, 70, 58, 88, 52, ...lead.channelGains.slice(6)];
  lead.channelMuted = [false, true, true, ...lead.channelMuted.slice(3)];
  return reduce([actions.status(status)]);
}

test("a hidden input has no strip, and a pair is one stereo strip whose pan is its balance", () => {
  const strips = channelStrips(pairedState([20, 100]), LEAD);

  assert.deepEqual(
    strips.map(({ index, sides, stereo, label }) => [index, sides, stereo, label]),
    [
      [0, [0], false, "Kick"],
      [1, [1, 2], true, "Keys L"],
      [4, [4], false, "CH 5"],
      [5, [5], false, "Backing vox"],
    ],
  );
  const keys = strips[1];
  assert.equal(keys.kind, "keys");
  assert.equal(keys.pan, 60, "the balance the two pans make");
  assert.equal(keys.gain, 70);
  assert.equal(keys.muted, true, "the pair's level, mute and solo are its first side's");
  assert.equal(keys.readout, "muted");
  assert.equal(channelStrips(pairedState([0, 100]), LEAD)[1].pan, 50, "a new pair is centred");
});

test("a stereo strip's level, mute and solo write both inputs, and its balance both pans", () => {
  const pair = { sides: [1, 2], stereo: true };
  const single = { sides: [3], stereo: false };

  assert.deepEqual(channelEdits(pair, "channelGains", 40), {
    "channelGains.1": 40,
    "channelGains.2": 40,
  });
  assert.deepEqual(channelEdits(pair, "channelSolo", true), {
    "channelSolo.1": true,
    "channelSolo.2": true,
  });
  assert.deepEqual(channelEdits(pair, "pans", 70), { "pans.1": 40, "pans.2": 100 });
  assert.deepEqual(channelEdits(single, "pans", 30), { "pans.3": 30 });
  assert.deepEqual(channelEdits(single, "channelMuted", true), { "channelMuted.3": true });
});

test("a strip's meter shows its input, or the louder side of a pair", () => {
  assert.equal(stripLevel([0], [10, 20, 30]), 10);
  assert.equal(stripLevel([1, 2], [10, 20, 30]), 30);
  assert.equal(stripLevel([4], [10, 20, 30]), 0, "an input the meters do not cover yet");
});

test("a channel strip finds its group by id, so two groups may share a name", () => {
  const status = sampleStatus();
  status.groups = [
    { id: "kit-a", name: "Drums", channels: [0], invalid: false },
    { id: "kit-b", name: "Drums", channels: [1], invalid: false },
  ];
  status.channels = status.channels.map((channel, index) => ({
    ...channel,
    group: index < 2 ? "Drums" : null,
    groupId: ["kit-a", "kit-b"][index] ?? null,
  }));

  const strips = channelStrips(reduce([actions.status(status)]), LEAD);

  assert.deepEqual(strips[0].group, { id: "kit-a", name: "Drums", tone: "teal", invalid: false });
  assert.deepEqual(strips[1].group, { id: "kit-b", name: "Drums", tone: "blue", invalid: false });
  assert.equal(strips[2].group, null);
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
    { index: 0, id: "drums", name: "Drums", tone: "teal", invalid: false, level: 100, readout: "100%" },
    { index: 1, id: "vocals", name: "Vocals", tone: "blue", invalid: false, level: 70, readout: "70%" },
  ]);
});

test("the master strip carries the musician's level, level cap, the engineer's limit and mute", () => {
  assert.deepEqual(masterStrip(running, LEAD), {
    volume: 85,
    cap: 90,
    limit: 100,
    muted: false,
    readout: "85% · cap 90% · limit 100%",
    ceilingNote: "The lower of the phone's cap and your limit wins: at most 90%.",
  });
  const muted = reduce([actions.mixEdit(LEAD, { muted: true })], running);
  assert.equal(masterStrip(muted, LEAD).readout, "muted · cap 90% · limit 100%");
});

test("without that musician there is nothing to strip", () => {
  assert.deepEqual(channelStrips(running, "10.0.0.1:50000"), []);
  assert.deepEqual(groupStrips(running, "10.0.0.1:50000"), []);
  assert.equal(masterStrip(running, "10.0.0.1:50000"), null);
});

test("the mix area says why it is empty", () => {
  const nobody = sampleStatus();
  nobody.musicians = [];

  assert.deepEqual(mixNotice(initialState), { kind: "connecting" });
  assert.deepEqual(mixNotice(reduce([actions.connected()])), { kind: "stopped" });
  assert.deepEqual(mixNotice(reduce([actions.connectionFailed("no events")], running)), {
    kind: "disconnected",
    message: "no events",
  });
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
    controls: null,
    command: null,
    audio: null,
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
    controls: null,
    command: null,
    audio: null,
  });

  const unstopped = reduce(
    [actions.engine("stopping"), actions.commandFailed("stop", "could not stop the capture")],
    running,
  );
  assert.equal(
    mixMessages(unstopped).command,
    "The engine could not be stopped: could not stop the capture.",
  );
});

test("the console says when the engine runs on another input than the saved one", () => {
  const message = "The saved input Volt was not found, so the engine started on the default.";
  const fellBack = reduce([actions.audioNotice({ kind: "fallback", message })], running);

  assert.equal(mixMessages(fellBack).audio, message);
  assert.equal(mixMessages(running).audio, null);
});
