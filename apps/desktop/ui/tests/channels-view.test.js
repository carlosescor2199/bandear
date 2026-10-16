import test from "node:test";
import assert from "node:assert/strict";

import { pairUnits } from "../app/lib/channels.js";
import {
  KIND_OPTIONS,
  addGroup,
  channelRows,
  channelTableOf,
  deleteGroup,
  groupCards,
  groupDefinitionsOf,
  linkChoice,
  renameChannel,
  renameGroup,
  setChannelHidden,
  setChannelKind,
  setChannelLink,
  setGroupMembers,
} from "../app/lib/channels-view.js";
import { actions, consoleReducer, initialState } from "../app/lib/console-state.js";
import { sampleStatus } from "../app/lib/fake-bridge.js";

function detail(name, kind = "other", { hidden = false, link = false } = {}) {
  return { name, kind, hidden, link };
}

// A status whose inputs carry `details` (one per input) and whose groups are `groups`.
function statusWith(details, groups = []) {
  const status = sampleStatus();
  status.sourceChannels = details.length;
  status.captureFormat = { ...status.captureFormat, channels: details.length };
  status.groups = groups;
  status.channels = details.map((entry, index) => {
    const group = groups.find((candidate) => !candidate.invalid && candidate.channels.includes(index));
    return {
      number: index + 1,
      ...entry,
      group: group?.name ?? null,
      groupId: group?.id ?? null,
      level: 0,
    };
  });
  return status;
}

function stateOf(status) {
  return [actions.status(status)].reduce(consoleReducer, initialState);
}

const running = stateOf(sampleStatus());

test("the kind picker offers every engine kind, by name", () => {
  assert.deepEqual(
    KIND_OPTIONS.map(({ value, label }) => `${value}:${label}`),
    [
      "voice:Voice",
      "guitar:Guitar",
      "bass:Bass",
      "keys:Keys",
      "drums:Drums",
      "percussion:Percussion",
      "horns:Horns",
      "strings:Strings",
      "playback:Playback",
      "click:Click",
      "other:Other",
    ],
  );
});

test("a row per input with its name, instrument, group and whether musicians see it", () => {
  const rows = channelRows(running);

  assert.equal(rows.length, 6);
  const { stereo, ...kick } = rows[0];
  assert.deepEqual(kick, {
    index: 0,
    number: 1,
    input: "CH 1",
    name: "Kick",
    label: "Kick",
    kind: "drums",
    kindLabel: "Drums",
    hidden: false,
    group: { id: "drums", name: "Drums", tone: "teal", invalid: false },
  });
  assert.equal(stereo.linked, false);
  assert.equal(rows[3].group, null, "CH 4 is in no group");
  assert.deepEqual(rows[4].group, { id: "vocals", name: "Vocals", tone: "blue", invalid: false });
  assert.deepEqual(channelRows(stateOf(statusWith([detail("")]))).map((row) => row.label), ["CH 1"]);
  assert.deepEqual(channelRows(initialState), [], "no engine, no inputs to list");
});

test("an input can pair with the next one only when that one exists, is free and is shown alike", () => {
  const channels = statusWith([
    detail("Keys L", "keys", { link: true }),
    detail("Keys R", "keys"),
    detail("Guitar", "guitar"),
    detail("OH L", "drums", { link: true }),
    detail("OH R", "drums"),
    detail("Spare", "other", { hidden: true }),
  ]).channels;

  assert.deepEqual(linkChoice(channels, 0), {
    linked: true,
    first: 0,
    partner: 1,
    text: "Linked with CH 2",
    label: "Stereo pair CH 1 and CH 2",
    available: true,
    reason: null,
  });
  assert.deepEqual(linkChoice(channels, 1), {
    linked: true,
    first: 0,
    partner: 0,
    text: "Linked with CH 1",
    label: "Stereo pair CH 1 and CH 2",
    available: true,
    reason: null,
  });
  assert.deepEqual(linkChoice(channels, 2), {
    linked: false,
    first: 2,
    partner: 3,
    text: "Mono",
    label: "Stereo pair CH 3 and CH 4",
    available: false,
    reason: "CH 4 is already paired with CH 5.",
  });
  assert.equal(linkChoice(channels, 4).linked, true, "CH 5 is OH R");
  assert.deepEqual(linkChoice(channels, 5), {
    linked: false,
    first: 5,
    partner: null,
    text: "Mono",
    label: "Stereo pair CH 6 and the next input",
    available: false,
    reason: "CH 6 is the last input, so there is no next input to pair it with.",
  });

  const mixed = statusWith([detail("A"), detail("B", "other", { hidden: true }), detail("C")]).channels;
  assert.equal(
    linkChoice(mixed, 0).reason,
    "CH 2 is hidden and CH 1 is shown: show or hide both before pairing them.",
  );
  assert.equal(linkChoice(mixed, 1).available, false, "CH 2 is hidden, CH 3 shown");
  const free = statusWith([detail("A"), detail("B"), detail("C")]).channels;
  assert.equal(linkChoice(free, 1).available, true);
  assert.equal(linkChoice(free, 1).reason, null);
});

test("an edit of one input keeps the whole table, past the device included, and leaves it as it was", () => {
  const table = [
    detail("Kick", "drums"),
    detail("Snare", "drums"),
    detail(""),
    detail("Rack tom", "drums"),
  ];

  const renamed = renameChannel(table, 0, "  Kick in  ", 2);
  assert.deepEqual(renamed[0], detail("Kick in", "drums"), "names are trimmed");
  assert.deepEqual(renamed.slice(1), table.slice(1), "details past the device are kept");
  assert.equal(table[0].name, "Kick", "the table given is left as it was");

  assert.deepEqual(renameChannel([], 2, "Bass", 4), [
    detail(""),
    detail(""),
    detail("Bass"),
    detail(""),
  ]);
  assert.deepEqual(setChannelKind(table, 2, "keys", 4)[2], detail("", "keys"));
  assert.throws(() => renameChannel(table, 1, "x".repeat(33), 4), /CH 2/);
  assert.throws(() => setChannelKind(table, 1, "kazoo", 4), /kazoo/);
});

test("hiding or showing one side of a pair hides or shows both", () => {
  const table = [detail("Keys L", "keys", { link: true }), detail("Keys R", "keys"), detail("Vox")];

  assert.deepEqual(
    setChannelHidden(table, 1, true, 3).map((entry) => entry.hidden),
    [true, true, false],
  );
  assert.deepEqual(
    setChannelHidden(setChannelHidden(table, 0, true, 3), 0, false, 3).map((entry) => entry.hidden),
    [false, false, false],
  );
  assert.deepEqual(
    setChannelHidden(table, 2, true, 3).map((entry) => entry.hidden),
    [false, false, true],
  );
  // A pair whose second side the device lacks is still a pair in the table.
  assert.deepEqual(
    setChannelHidden(table, 0, true, 1).map((entry) => entry.hidden),
    [true, true, false],
  );
});

test("linking pairs an input with the next; unlinking works from either side", () => {
  const table = [detail("A"), detail("B"), detail("C"), detail("D")];

  const linked = setChannelLink(table, 1, true, 4);
  assert.deepEqual(
    linked.map((entry) => entry.link),
    [false, true, false, false],
  );
  assert.deepEqual(
    setChannelLink(linked, 2, false, 4).map((entry) => entry.link),
    [false, false, false, false],
    "from the second side",
  );
  assert.deepEqual(
    setChannelLink(linked, 1, false, 4).map((entry) => entry.link),
    [false, false, false, false],
  );
  assert.throws(() => setChannelLink(linked, 0, true, 4), /CH 2/, "CH 2 is already in a pair");
  assert.throws(() => setChannelLink(table, 3, true, 4), /CH 4/, "the last input");
});

test("a status's details and groups are what the desk sends back", () => {
  const status = sampleStatus();

  assert.deepEqual(channelTableOf(status)[0], detail("Kick", "drums"));
  assert.equal(channelTableOf(status).length, 6);
  assert.deepEqual(channelTableOf(null), []);
  assert.deepEqual(groupDefinitionsOf(status), [
    { id: "drums", name: "Drums", channels: [1, 2, 3] },
    { id: "vocals", name: "Vocals", channels: [5, 6] },
  ]);
  assert.deepEqual(groupDefinitionsOf(null), []);
});

test("a new group gets a free name, and there are at most 16", () => {
  const groups = groupDefinitionsOf(sampleStatus());

  const added = addGroup(groups);
  assert.deepEqual(added.at(-1), { name: "Group 3", channels: [] });
  assert.equal(groups.length, 2, "the list given is left as it was");
  assert.deepEqual(addGroup([{ id: "a", name: "Group 1", channels: [] }]).at(-1).name, "Group 2");
  const full = Array.from({ length: 16 }, (_, index) => ({
    id: `g${index}`,
    name: `G ${index}`,
    channels: [],
  }));
  assert.throws(() => addGroup(full), /16/);
});

test("groups are renamed, given members and deleted by id", () => {
  const groups = groupDefinitionsOf(sampleStatus());

  assert.equal(renameGroup(groups, "vocals", "  Voices ")[1].name, "Voices");
  assert.throws(() => renameGroup(groups, "vocals", "  "), /name/);

  const moved = setGroupMembers(groups, "vocals", [3], true);
  assert.deepEqual(
    moved.map(({ id, channels }) => [id, channels]),
    [
      ["drums", [1, 2]],
      ["vocals", [3, 5, 6]],
    ],
    "an input is in one group: choosing it here moves it",
  );
  assert.deepEqual(
    setGroupMembers(groups, "drums", [1], false)[0].channels,
    [2, 3],
  );
  assert.deepEqual(
    setGroupMembers(groups, "drums", [4, 5], true).map(({ channels }) => channels),
    [[1, 2, 3, 4, 5], [6]],
    "a pair joins with both inputs",
  );
  assert.deepEqual(
    deleteGroup(groups, "drums").map(({ id }) => id),
    ["vocals"],
  );
});

test("a group card lists its members by name, a pair once, and offers every input", () => {
  const status = statusWith(
    [
      detail("Kick", "drums"),
      detail("OH L", "drums", { link: true }),
      detail("OH R", "drums"),
      detail(""),
      detail("Spare", "other", { hidden: true }),
    ],
    [
      { id: "kit", name: "Drums", channels: [0, 1, 2], invalid: false },
      { id: "rack", name: "Rack", channels: [3, 8], invalid: true },
    ],
  );

  const cards = groupCards(stateOf(status));

  assert.deepEqual(
    cards.map(({ id, name, tone, invalid, members, count }) => ({
      id,
      name,
      tone,
      invalid,
      members,
      count,
    })),
    [
      {
        id: "kit",
        name: "Drums",
        tone: "teal",
        invalid: false,
        members: ["Kick", "OH L (stereo)"],
        count: "2 channels",
      },
      {
        id: "rack",
        name: "Rack",
        tone: "blue",
        invalid: true,
        members: ["CH 4", "CH 9 (not on this device)"],
        count: "2 channels",
      },
    ],
  );
  assert.deepEqual(cards[0].choices, [
    { key: "1", numbers: [1], label: "CH 1 Kick", checked: true, owner: null, hidden: false },
    { key: "2", numbers: [2, 3], label: "CH 2-3 OH L", checked: true, owner: null, hidden: false },
    { key: "4", numbers: [4], label: "CH 4", checked: false, owner: "Rack", hidden: false },
    { key: "5", numbers: [5], label: "CH 5 Spare", checked: false, owner: null, hidden: true },
  ]);
});

test("the units of a device's inputs: a pair of two shown or two hidden inputs, else one input", () => {
  assert.deepEqual(
    pairUnits([
      detail("A", "keys", { link: true }),
      detail("B"),
      detail("C", "other", { link: true }),
      detail("D", "other", { hidden: true }),
      detail("E", "other", { link: true }),
    ]),
    [[0, 1], [2], [3], [4]],
  );
});
