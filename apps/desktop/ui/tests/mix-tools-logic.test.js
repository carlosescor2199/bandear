// The Mix tools' behaviour (mix-tools.js) as <ml-mix-tools> uses it, with a stand-in clock and
// stand-in timers, so the element's rules are tested without a browser.
import test from "node:test";
import assert from "node:assert/strict";

import {
  ARMED_FOR_MS,
  createMixTools,
  followingState,
  mixToolCommands,
} from "../app/lib/mix-tools.js";
import { CONFIRM_DELAY_MS } from "../app/lib/network-view.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const KEYS = "192.168.1.17:50000";

function target(address, label, disabled = false) {
  return { address, label, disabled, note: disabled ? "follows Bass" : null };
}

// The tools' data for Lead's mix: Bass and Keys reachable unless `followers` names them.
function toolsData({ address = LEAD, followers = [], canUse = true, presets = [], mirror = "" } = {}) {
  const others = [target(BASS, "Bass"), target(KEYS, "Keys")]
    .filter((other) => other.address !== address)
    .map((other) => (followers.includes(other.address) ? target(other.address, other.label, true) : other));
  return {
    address,
    name: "Lead vocal",
    mirror: {
      value: mirror,
      options: [
        { value: "", label: "Off" },
        { value: BASS, label: "Bass" },
        { value: KEYS, label: "Keys" },
      ],
      note: null,
    },
    copyTargets: others,
    presetTargets: [target(address, "Lead vocal", followers.includes(address)), ...others],
    presets,
    canUse,
  };
}

// The tools with a stand-in clock and timers, and every event they sent.
function tools() {
  const sent = [];
  let clock = 1000;
  let renders = 0;
  const timers = new Map();
  let next = 0;
  const controller = createMixTools({
    emit: (type, detail) => sent.push([type, detail]),
    changed: () => {
      renders += 1;
    },
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
    renders: () => renders,
    // Moves the clock on by `ms` and runs every timer that is due.
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

const VERSE = [{ id: "p1", name: "Verse" }];

test("the Mirror control shows a choice on its way, then the desk's mirror after any answer", () => {
  const { controller, sent } = tools();
  controller.setData(toolsData());

  controller.chooseMirror(BASS);

  assert.deepEqual(sent, [["mirror-change", { leader: BASS }]]);
  assert.equal(controller.mirrorValue(), BASS, "the choice shows while it is on its way");
  controller.answered("mirror", false);
  assert.equal(controller.mirrorValue(), "", "a refused choice gives way to the desk's mirror");

  controller.chooseMirror(KEYS);
  controller.setData({
    ...toolsData(),
    mirror: { value: "", options: [{ value: "", label: "Off" }], note: null },
  });
  assert.equal(controller.mirrorValue(), "", "a choice no longer offered shows the desk's mirror");
  controller.setData(toolsData({ mirror: KEYS }));
  controller.answered("mirror", true);
  assert.equal(controller.mirrorValue(), KEYS);
  controller.chooseMirror(KEYS);
  controller.chooseMirror("away");
  assert.equal(sent.length, 2, "the mirror it has, or a leader away, sends nothing");
});

test("a desk mirror that changes by any route replaces a choice still on its way", () => {
  const { controller } = tools();
  controller.setData(toolsData());
  controller.chooseMirror(BASS);

  controller.setData(toolsData());
  assert.equal(controller.mirrorValue(), BASS, "a status that leaves the desk's mirror keeps it");
  controller.setData(toolsData({ mirror: KEYS }));
  assert.equal(controller.mirrorValue(), KEYS, "the desk's new mirror shows, not the old choice");
  controller.setData(toolsData({ mirror: KEYS }));
  assert.equal(controller.mirrorValue(), KEYS);
});

// The screen's wiring with the real tools and a stand-in console whose mirror command answers when
// the test says so.
function wired() {
  const said = [];
  const handled = [];
  const asked = [];
  const app = {
    selected: LEAD,
    store: { getState: () => ({ selected: app.selected }) },
    setMirror: (address, leader) =>
      new Promise((resolve, reject) => asked.push({ address, leader, resolve, reject })),
  };
  let commands = null;
  const controller = createMixTools({
    emit: (type, detail) => handled.push(commands.handle(type, detail)),
  });
  commands = mixToolCommands({ app, tools: controller, said: (outcome) => said.push(outcome) });
  controller.setData(toolsData());
  return { app, controller, commands, said, handled, asked };
}

test("the screen hands the mirror command's answer to the tools, done or refused", async () => {
  const { controller, commands, said, handled, asked } = wired();
  assert.ok(commands.types.includes("mirror-change"));

  controller.chooseMirror(BASS);
  assert.deepEqual(
    asked.map(({ address, leader }) => [address, leader]),
    [[LEAD, BASS]],
    "the open musician's mirror is asked for",
  );
  assert.equal(controller.mirrorValue(), BASS);
  asked[0].resolve({ ok: false, message: "The mirror was not changed: a circle." });
  await handled[0];
  assert.equal(controller.mirrorValue(), "", "the refusal reached the tools");

  controller.chooseMirror(KEYS);
  asked[1].resolve({ ok: true, message: "Lead vocal follows Keys's mix now." });
  await handled[1];
  assert.equal(controller.mirrorValue(), "", "the desk's mirror until its status shows Keys");
  assert.deepEqual(
    said.map((outcome) => outcome.ok),
    [false, true],
    "both answers are said above the strips",
  );
});

test("a mirror command that fails, or finds no musician open, still frees the Mirror control", async () => {
  const { app, controller, said, handled, asked } = wired();

  controller.chooseMirror(BASS);
  asked[0].reject(new Error("the bridge went away"));
  await assert.rejects(handled[0], /bridge went away/);
  assert.equal(controller.mirrorValue(), "", "no choice stays on show unanswered");

  app.selected = null;
  controller.chooseMirror(KEYS);
  assert.equal(await handled[1], null);
  assert.equal(asked.length, 1, "nothing is asked for without a musician open");
  assert.equal(controller.mirrorValue(), "");
  assert.deepEqual(said, [], "nothing to say");
});

test("ticks go with the open musician, and a musician who starts following loses theirs", () => {
  const { controller, sent } = tools();
  controller.setData(toolsData());
  controller.tick("copy", BASS, true);
  controller.tick("copy", KEYS, true);

  controller.setData(toolsData({ followers: [KEYS] }));
  controller.tick("copy", KEYS, true);
  controller.copy();

  assert.equal(controller.isTicked("copy", KEYS), false, "a follower cannot be ticked");
  assert.deepEqual(sent, [["mix-copy", { to: [BASS] }]]);
  controller.setData(toolsData({ address: BASS }));
  assert.equal(controller.isTicked("copy", KEYS), false);
  assert.equal(controller.copyReady(), false, "nothing ticked for another musician's mix");
});

test("a preset goes to the open musician to begin with, unless their mix follows another's", () => {
  const { controller, sent } = tools();
  controller.setData(toolsData({ presets: VERSE }));
  assert.equal(controller.isTicked("apply", LEAD), true);
  controller.tick("apply", BASS, true);
  controller.apply("p1");
  assert.deepEqual(sent, [["preset-apply", { id: "p1", to: [LEAD, BASS] }]]);

  const following = tools();
  following.controller.setData(toolsData({ presets: VERSE, followers: [LEAD] }));
  assert.equal(following.controller.isTicked("apply", LEAD), false);
  assert.equal(following.controller.applyReady("p1"), false);
});

test("nothing is copied, saved or applied while no engine runs, but a preset can be renamed", () => {
  const { controller, sent } = tools();
  controller.setData(toolsData({ canUse: false, presets: VERSE }));
  controller.tick("copy", BASS, true);

  controller.copy();
  controller.save("Chorus");
  controller.apply("p1");
  controller.rename("p1", "  Bridge ");
  controller.rename("p1", "   ");

  assert.deepEqual(sent, [["preset-rename", { id: "p1", name: "Bridge" }]]);
  assert.equal(controller.saveReady("Chorus"), false);
});

test("a panel closes on Escape and gives the focus back to its button", () => {
  const { controller, sent } = tools();
  controller.setData(toolsData({ presets: null }));

  assert.deepEqual(controller.toggle("copy"), { focus: "panel" });
  assert.equal(controller.open(), "copy");
  assert.deepEqual(controller.escape(), { focus: "copy" });
  assert.equal(controller.open(), null);
  assert.deepEqual(controller.escape(), { focus: null }, "nothing open, nothing to give back");
  controller.toggle("presets");
  controller.toggle("copy");
  assert.equal(controller.open(), "copy", "one panel at a time");
  assert.deepEqual(sent, [["presets-load", {}]], "presets not read yet are asked for");
});

test("a delete asks first, works only after the pause, and disarms itself after ten seconds", () => {
  const clocked = tools();
  const { controller, sent } = clocked;
  controller.setData(toolsData({ presets: VERSE }));

  controller.arm("p1");
  assert.deepEqual(controller.deleteState("p1"), { armed: true, ready: false });
  assert.equal(controller.confirmDelete("p1"), false, "a double click does not delete");
  clocked.advance(CONFIRM_DELAY_MS);
  assert.deepEqual(controller.deleteState("p1"), { armed: true, ready: true });
  assert.equal(clocked.renders(), 1, "the element is asked to show the confirmation ready");
  assert.equal(controller.confirmDelete("p1"), true);
  assert.deepEqual(sent, [["preset-delete", { id: "p1" }]]);

  controller.arm("p1");
  clocked.advance(ARMED_FOR_MS);
  assert.deepEqual(controller.deleteState("p1"), { armed: false, ready: false });
  controller.arm("p1");
  controller.keep();
  clocked.advance(ARMED_FOR_MS);
  assert.equal(controller.confirmDelete("p1"), false, "Keep keeps it");
  controller.arm("p1");
  controller.setData(toolsData({ presets: [] }));
  assert.deepEqual(controller.deleteState("p1"), { armed: false, ready: false }, "a preset gone");
  assert.equal(sent.length, 1);
});

test("an answer empties the field of a done save or rename and unticks a done copy", () => {
  const { controller } = tools();
  controller.setData(toolsData());
  controller.tick("copy", BASS, true);

  assert.deepEqual(controller.answered("save", false), { clearSave: false, clearRename: false });
  assert.deepEqual(controller.answered("save", true), { clearSave: true, clearRename: false });
  assert.deepEqual(controller.answered("rename", true), { clearSave: false, clearRename: true });
  controller.answered("copy", false);
  assert.equal(controller.isTicked("copy", BASS), true, "a refused copy keeps its musicians");
  controller.answered("copy", true);
  assert.equal(controller.isTicked("copy", BASS), false);
});

test("a strip of a mix that follows another says so and takes no move", () => {
  assert.deepEqual(followingState("Lead vocal", "Kick"), {
    disabled: true,
    text: "Follows Lead vocal",
    label: "Kick, follows Lead vocal's mix",
  });
  assert.deepEqual(followingState(null, "Kick"), { disabled: false, text: "", label: "Kick" });
});
