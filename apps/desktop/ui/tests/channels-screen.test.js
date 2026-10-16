// The Channels and groups screen's wiring (channelsScreenCommands, channels-view.js), without a
// DOM: a clicked Shown or group box stays as clicked until its edit is answered, even while statuses
// that still carry the old value come in, and then shows what the desk has.
import test from "node:test";
import assert from "node:assert/strict";

import { channelsScreenCommands } from "../app/lib/channels-view.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { createFakeBridge } from "../app/lib/fake-bridge.js";

function settle() {
  return new Promise((resolve) => setImmediate(resolve));
}

// The fake bridge with `deferred` commands that do nothing until `release()`: not even their
// effect happens before then, as a slow desk would answer.
function deferring(fake, deferred) {
  const waiting = [];
  return {
    calls: fake.calls,
    invoke(command, args) {
      if (!deferred.includes(command)) {
        return fake.invoke(command, args);
      }
      return new Promise((resolve, reject) => {
        waiting.push(() => fake.invoke(command, args).then(resolve, reject));
      });
    },
    listen: (event, handler) => fake.listen(event, handler),
    setTitle: (title) => fake.setTitle(title),
    waiting: () => waiting.length,
    release() {
      for (const run of waiting.splice(0)) {
        run();
      }
    },
  };
}

async function screen(deferred) {
  const fake = createFakeBridge({ running: true });
  const bridge = deferring(fake, deferred);
  const app = createConsoleApp({ bridge });
  await app.ready;
  let renders = 0;
  const commands = channelsScreenCommands({
    app,
    changed: () => {
      renders += 1;
    },
  });
  return { fake, bridge, app, commands, renders: () => renders };
}

test("a clicked Shown box stays as clicked until its edit is answered, then shows the desk's", async () => {
  const { fake, bridge, app, commands, renders } = await screen(["set_channels"]);
  assert.equal(commands.view().rows[1].hidden, false);

  const editing = commands.handle("channel-shown", { index: 1, shown: false });
  await settle();

  assert.equal(bridge.waiting(), 1, "the edit is on its way");
  assert.equal(app.store.getState().engine.status.channels[1].hidden, false, "the desk has not changed");
  assert.equal(commands.view().rows[1].hidden, true, "the box shows as clicked");
  assert.ok(renders() >= 1, "the screen was asked to draw the held box at once");
  fake.emit("status", await fake.invoke("engine_status"));
  assert.equal(commands.view().rows[1].hidden, true, "a status with the old value does not flip it back");

  bridge.release();
  const outcome = await editing;

  assert.equal(outcome.ok, true, outcome.message);
  assert.equal(commands.view().outcome, outcome);
  assert.equal(app.store.getState().engine.status.channels[1].hidden, true);
  assert.equal(commands.view().rows[1].hidden, true, "now the desk's own value");
  app.dispose();
});

test("a refused Shown edit lets the box show the desk's value again", async () => {
  const { fake, bridge, app, commands } = await screen(["set_channels"]);
  fake.failNext("set_channels", "the disk is full");

  const editing = commands.handle("channel-shown", { index: 2, shown: false });
  await settle();
  assert.equal(commands.view().rows[2].hidden, true);
  bridge.release();
  const outcome = await editing;

  assert.equal(outcome.ok, false);
  assert.match(outcome.message, /the disk is full/);
  assert.equal(commands.view().rows[2].hidden, false, "the box goes back to what the desk has");
  app.dispose();
});

test("a clicked group box stays as clicked until its edit is answered", async () => {
  const { bridge, app, commands } = await screen(["set_groups"]);
  const box = () =>
    commands
      .view()
      .cards.find((card) => card.id === "drums")
      .choices.find((choice) => choice.key === "4");
  assert.equal(box().checked, false, "CH 4 is in no group yet");

  const editing = commands.handle("group-member", { id: "drums", key: "4", numbers: [4], member: true });
  await settle();

  assert.equal(bridge.waiting(), 1);
  assert.equal(box().checked, true, "held as clicked while the desk still has it out");
  bridge.release();
  await editing;
  assert.equal(box().checked, true);
  assert.deepEqual(
    app.store.getState().engine.status.groups.find((group) => group.id === "drums").channels,
    [0, 1, 2, 3],
  );
  app.dispose();
});
