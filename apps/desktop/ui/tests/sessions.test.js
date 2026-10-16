// Named sessions (sessions.js): the name rule, the list and the words for the desk's answers, and
// the Session panel's behaviour with a stand-in clock and timers, tested without a browser.
import test from "node:test";
import assert from "node:assert/strict";

import { ARMED_FOR_MS } from "../app/lib/mix-tools.js";
import { CONFIRM_DELAY_MS } from "../app/lib/network-view.js";
import {
  checkSessionName,
  createSessionPanel,
  formatSavedAt,
  formatSessionLoaded,
  formatSessionSaved,
  sessionPanelCommands,
  sessionPanelView,
  sessionRows,
  skippedName,
} from "../app/lib/sessions.js";

const AT = new Date(2026, 9, 16, 19, 5).getTime();
const SHOW = { id: "s1", name: "Show", savedAt: AT, mixes: 3, problem: null };
const BROKEN = { id: "broken", name: "broken", savedAt: null, mixes: 0, problem: "the file is not valid JSON" };

test("a session name is trimmed, at most 40 characters and unique in any case", () => {
  assert.equal(checkSessionName("  Show  ", []), "Show");
  assert.throws(() => checkSessionName("   ", []), /needs a name/);
  assert.throws(() => checkSessionName("x".repeat(41), []), /at most 40/);
  assert.equal(checkSessionName("x".repeat(40), []).length, 40);
  assert.throws(() => checkSessionName(" SHOW ", [SHOW]), /already saved/);
  assert.equal(checkSessionName("show", [SHOW], "s1"), "show", "its own name in another case");
});

test("the saved sessions are listed with when they were saved and their mixes, or why not", () => {
  assert.equal(formatSavedAt(AT), "16 Oct 2026, 19:05");
  assert.equal(formatSavedAt(null), null);

  assert.deepEqual(sessionRows([SHOW, { ...SHOW, id: "s2", name: "Solo", mixes: 1, savedAt: null }, BROKEN]), [
    { id: "s1", name: "Show", loadable: true, label: "Show", detail: "Saved 16 Oct 2026, 19:05 · 3 musicians' mixes" },
    { id: "s2", name: "Solo", loadable: true, label: "Solo", detail: "1 musician's mix" },
    {
      id: "broken",
      name: "broken",
      loadable: false,
      label: "broken (cannot be loaded)",
      detail: "It cannot be loaded: the file is not valid JSON.",
    },
  ]);
});

test("the panel says what the desk said about its session, and when nothing could be listed", () => {
  const state = {
    sessions: { list: null, error: "access denied" },
    sessionNotice: "the session file x has a `groups` section that cannot be read.",
    engine: { status: null },
  };

  const view = sessionPanelView(state);

  assert.equal(
    view.notice,
    "When the console opened its saved session: the session file x has a `groups` section that cannot be read.",
  );
  assert.equal(view.unread, "access denied");
  assert.deepEqual(view.rows, []);
  assert.equal(view.engineRunning, false);
  const listed = sessionPanelView({ ...state, sessions: { list: [SHOW], error: null }, sessionNotice: null });
  assert.equal(listed.notice, null);
  assert.equal(listed.unread, null);
  assert.equal(listed.rows.length, 1);
});

test("a save says whether the musicians' mixes were kept", () => {
  assert.equal(
    formatSessionSaved({ session: SHOW, engineRunning: true }),
    'Saved the session as "Show", with 3 musicians\' mixes.',
  );
  assert.equal(
    formatSessionSaved({ session: { ...SHOW, mixes: 0 }, engineRunning: true }),
    'Saved the session as "Show". No musician was listed, so no mix was kept.',
  );
  assert.equal(
    formatSessionSaved({ session: { ...SHOW, mixes: 0 }, engineRunning: false }),
    'Saved the session as "Show" without mixes: the engine is not running.',
  );
});

function loaded(changes = {}) {
  return {
    session: SHOW,
    applied: true,
    saved: true,
    saveError: null,
    mixes: [],
    skipped: [],
    refused: [],
    damaged: [],
    audio: null,
    audioLoaded: false,
    audioSkipped: null,
    presets: [],
    ...changes,
  };
}

const NAMES = {
  labelAt: (address) => ({ a: "Ana", b: "Rack", c: "Bass" })[address] ?? address,
  nameOf: (skipped) => skipped.label ?? "a phone",
};

test("a load says what applies, what was pushed to whom, and what went to nobody and why", () => {
  const message = formatSessionLoaded(
    loaded({
      mixes: [
        { address: "a", follows: null },
        { address: "b", follows: null },
        { address: "c", follows: "Ana" },
      ],
      skipped: [
        { key: "client:x", label: "Keys", reason: "notConnected", error: null },
        { key: "client:y", label: "Drums", reason: "notHere", error: null },
        { key: "client:z", label: null, reason: "notHere", error: null },
        { key: "address:q", label: "Pad", reason: "refused", error: "the engine is stopping" },
      ],
      refused: ["Rack following Rack's mix was not applied: a mirror cannot go round in a circle"],
      damaged: ["its `presets` section cannot be read (expected a list)"],
      audioLoaded: true,
    }),
    NAMES,
  );

  assert.equal(
    message,
    'Loaded "Show": its channels, groups, presets, labels, limits, locks and mirrors apply now. ' +
      "Pushed the saved mixes to Ana and Rack. " +
      "Bass follows Ana's mix, so only their master volume and mute were set. " +
      "Not pushed to Keys: not connected, and a phone's own mix wins when it connects. " +
      "Not pushed to Drums and a phone: not here. " +
      "Not pushed to Pad: the engine is stopping. " +
      "Not applied: Rack following Rack's mix was not applied: a mirror cannot go round in a circle. " +
      "Part of the saved session could not be read and was left out: its `presets` section cannot be read (expected a list). " +
      "Its audio input is used from the next start.",
  );
});

test("a load with no engine applies at the next start and pushes no mix", () => {
  const message = formatSessionLoaded(
    loaded({
      applied: false,
      skipped: [
        { key: "client:x", label: null, reason: "notRunning", error: null },
        { key: "client:y", label: null, reason: "notRunning", error: null },
      ],
      audioSkipped: "ASIO is kept only with the ASIO driver the engineer picked, and none is named",
    }),
    NAMES,
  );

  assert.equal(
    message,
    'Loaded "Show": it applies at the next start. ' +
      "The 2 saved mixes were not pushed: the engine is not running. " +
      "Its audio input was not taken: ASIO is kept only with the ASIO driver the engineer picked, " +
      "and none is named. The desk keeps its own.",
  );
  assert.match(
    formatSessionLoaded(loaded({ saved: false, saveError: "the disk is full." }), NAMES),
    /It runs, but was not saved: the disk is full\.$/,
  );
});

test("a skipped mix names its musician by label, by the phone listed, or by address", () => {
  const musicians = [
    { address: "192.168.1.3:50000", clientId: "lead", clientName: "Lead vocal", name: "Lead vocal" },
  ];

  assert.equal(skippedName({ key: "client:lead", label: "Ana" }, musicians), "Ana");
  assert.equal(skippedName({ key: "client:lead", label: null }, musicians), "Lead vocal");
  assert.equal(skippedName({ key: "address:192.168.1.40:50000", label: null }, musicians), "192.168.1.40:50000");
  assert.equal(skippedName({ key: "client:gone", label: null }, musicians), "a phone");
});

// The panel with a stand-in clock and timers, and every event it sent.
function panel() {
  const sent = [];
  let clock = 1000;
  const timers = new Map();
  let next = 0;
  const controller = createSessionPanel({
    emit: (type, detail) => sent.push([type, detail]),
    now: () => clock,
    setTimer: (run, ms) => {
      next += 1;
      timers.set(next, { run, at: clock + ms });
      return next;
    },
    clearTimer: (id) => timers.delete(id),
  });
  return {
    controller,
    sent,
    advance(ms) {
      clock += ms;
      for (const [id, timer] of [...timers]) {
        if (timer.at <= clock) {
          timers.delete(id);
          timer.run();
        }
      }
    },
  };
}

const ROWS = sessionRows([SHOW, { ...SHOW, id: "s2", name: "Rehearsal" }, BROKEN]);

test("a load goes at once, or asks first while edits are still on their way", () => {
  const { controller, sent } = panel();
  controller.setRows(ROWS);
  assert.equal(controller.chosen(), "s1", "the newest is chosen to begin with");

  controller.load(0);
  assert.deepEqual(sent, [["session-load", { id: "s1" }]]);

  controller.choose("s2");
  controller.load(3);
  assert.equal(sent.length, 1, "nothing is loaded while it asks");
  assert.deepEqual(controller.asking(), { id: "s2", unsaved: 3, name: "Rehearsal" });
  controller.cancelLoad();
  assert.equal(controller.asking(), null);
  assert.equal(sent.length, 1, "Cancel loads nothing");

  controller.load(1);
  controller.confirmLoad();
  assert.deepEqual(sent.at(-1), ["session-load", { id: "s2" }]);
  assert.equal(controller.asking(), null);

  controller.load(2);
  controller.choose("s1");
  assert.equal(controller.asking(), null, "choosing another session drops the question");
  controller.load(2);
  controller.answered("load", false);
  assert.equal(controller.asking(), null);
});

test("a session that cannot be loaded is neither loaded nor renamed, and the choice stays listed", () => {
  const { controller, sent } = panel();
  controller.setRows(ROWS);
  controller.choose("broken");

  controller.load(0);
  controller.rename("Better");
  assert.equal(controller.loadReady(), false);
  assert.equal(controller.renameReady("Better"), false);
  assert.deepEqual(sent, []);

  controller.setRows(ROWS.filter((row) => row.id !== "broken"));
  assert.equal(controller.chosen(), "s1", "a session no longer listed gives way to the first");
  controller.choose("nowhere");
  assert.equal(controller.chosen(), "s1");
  controller.rename("  Late show ");
  controller.save("  ");
  controller.save(" Encore ");
  assert.deepEqual(sent, [
    ["session-rename", { id: "s1", name: "Late show" }],
    ["session-save", { name: "Encore" }],
  ]);
  assert.deepEqual(controller.answered("save", true), { clearSave: true, clearRename: false });
  assert.deepEqual(controller.answered("rename", false), { clearSave: false, clearRename: false });
});

test("a delete asks first, works only after a pause and disarms itself", () => {
  const { controller, sent, advance } = panel();
  controller.setRows(ROWS);

  controller.arm();
  assert.deepEqual(controller.deleteState(), { armed: true, ready: false });
  assert.equal(controller.confirmDelete(), false, "a double click keeps the session");
  advance(CONFIRM_DELAY_MS);
  assert.deepEqual(controller.deleteState(), { armed: true, ready: true });
  assert.equal(controller.confirmDelete(), true);
  assert.deepEqual(sent, [["session-delete", { id: "s1" }]]);

  controller.arm();
  advance(ARMED_FOR_MS);
  assert.deepEqual(controller.deleteState(), { armed: false, ready: false }, "it disarms itself");
  controller.arm();
  controller.choose("s2");
  assert.deepEqual(controller.deleteState(), { armed: false, ready: false }, "and on another choice");
  controller.arm();
  controller.keep();
  advance(CONFIRM_DELAY_MS);
  assert.equal(controller.confirmDelete(), false);
  assert.equal(sent.length, 1);
});

test("the screen answers the panel after every command, also one that fails", async () => {
  const answered = [];
  const said = [];
  const app = {
    saveSession: async (name) => ({ ok: true, message: `saved ${name}` }),
    loadSession: async () => {
      throw new Error("the bridge is gone");
    },
    renameSession: async () => ({ ok: false, message: "taken" }),
    deleteSession: async () => ({ ok: true, message: "deleted" }),
    loadSessions: async () => null,
  };
  const commands = sessionPanelCommands({
    app,
    panel: { answered: (kind, ok) => answered.push([kind, ok]) },
    said: (outcome) => said.push(outcome.message),
  });

  await commands.handle("session-save", { name: "Show" });
  await assert.rejects(commands.handle("session-load", { id: "s1" }), /the bridge is gone/);
  await commands.handle("session-rename", { id: "s1", name: "Show" });
  await commands.handle("sessions-load", {});

  assert.deepEqual(answered, [
    ["save", true],
    ["load", false],
    ["rename", false],
    ["list", false],
  ]);
  assert.deepEqual(said, ["saved Show", "taken"]);
});
