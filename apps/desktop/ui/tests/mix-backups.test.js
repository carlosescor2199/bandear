// Mix backups: how the Session panel lists them and words a restore (mix-backups-view.js), its
// restore-with-a-confirm behaviour apart from the DOM, and the console (console-app.js) with the
// fake bridge, which backs the mixes up as the desktop does and restores them onto the musicians
// who are connected.
import test from "node:test";
import assert from "node:assert/strict";

import { createConsoleApp } from "../app/lib/console-app.js";
import { initialState } from "../app/lib/console-state.js";
import { createFakeBridge } from "../app/lib/fake-bridge.js";
import {
  BACKUP_MAX_WAIT_MS,
  BACKUP_QUIET_MS,
  KEPT_BACKUPS,
  createMixBackupsPanel,
  formatBackupTime,
  formatMixRestored,
  mixBackupRows,
  mixBackupsView,
} from "../app/lib/mix-backups-view.js";
import { sessionPanelCommands } from "../app/lib/sessions.js";

const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";

function backup(stamp, changes = {}) {
  return { stamp, savedAt: 1_000, musicians: 2, names: ["Ana", "Rack"], problem: null, ...changes };
}

test("a backup's time is shown to the second in local time", () => {
  const at = new Date(2026, 9, 18, 19, 4, 5).getTime();
  assert.equal(formatBackupTime(at), "18 Oct 2026, 19:04:05");
  assert.equal(formatBackupTime(null), null);
});

test("the backups are listed with when, how many and whose mixes, a damaged one with why", () => {
  const at = new Date(2026, 9, 18, 19, 4, 5).getTime();
  const rows = mixBackupRows([
    backup("20261018T190405000Z", { savedAt: at, musicians: 3, names: ["Ana", "Bass", "Rack"] }),
    backup("20261018T190300000Z", { savedAt: at, musicians: 1, names: ["Ana"] }),
    backup("20261018T190200000Z", {
      savedAt: null,
      musicians: 0,
      names: [],
      problem: "it is damaged and cannot be read",
    }),
  ]);

  assert.deepEqual(rows, [
    {
      stamp: "20261018T190405000Z",
      restorable: true,
      label: "18 Oct 2026, 19:04:05",
      detail: "3 musicians: Ana, Bass and Rack",
    },
    {
      stamp: "20261018T190300000Z",
      restorable: true,
      label: "18 Oct 2026, 19:04:05",
      detail: "1 musician: Ana",
    },
    {
      stamp: "20261018T190200000Z",
      restorable: false,
      label: "Backup 20261018T190200000Z",
      detail: "It cannot be restored: it is damaged and cannot be read.",
    },
  ]);
});

test("the panel's view says whether the backups are listed yet and whether a restore can run", () => {
  const unread = mixBackupsView(initialState);
  assert.deepEqual(unread, { rows: [], unread: "", engineRunning: false });

  const failed = mixBackupsView({ ...initialState, mixBackups: { list: null, error: "denied" } });
  assert.equal(failed.unread, "denied");

  const listed = mixBackupsView({
    ...initialState,
    mixBackups: { list: [backup("20261018T190405000Z")], error: null },
    engine: { ...initialState.engine, status: { musicians: [] } },
  });
  assert.equal(listed.unread, null);
  assert.equal(listed.rows.length, 1);
  assert.equal(listed.engineRunning, true);
});

test("a restore is said in words: who got their mix back and who was skipped and why", () => {
  const labels = { [LEAD]: "Ana", [BASS]: "Bass" };
  const words = formatMixRestored(
    {
      backup: backup("20261018T190405000Z", { savedAt: new Date(2026, 9, 18, 19, 4, 5).getTime() }),
      restored: [
        { address: LEAD, follows: null, mix: {} },
        { address: BASS, follows: "Ana", mix: {} },
      ],
      skipped: [
        { key: "client:away", label: "Keys", reason: "notConnected", error: null },
        { key: "client:gone", label: "Drums", reason: "notHere", error: null },
        { key: "address:10.0.0.9:50000", label: null, reason: "refused", error: "stopping." },
      ],
    },
    { labelAt: (address) => labels[address], nameOf: (skipped) => skipped.label ?? "10.0.0.9:50000" },
  );

  assert.equal(
    words,
    "Restored the mixes backed up at 18 Oct 2026, 19:04:05 to Ana. " +
      "Bass follows Ana's mix, so only their master volume and mute were set. " +
      "Not restored to Keys: not connected, and a phone's own mix wins when it connects. " +
      "Not restored to Drums: not here. " +
      "Not restored to 10.0.0.9:50000: stopping.",
  );
  assert.equal(
    formatMixRestored(
      { backup: backup("x", { savedAt: null }), restored: [], skipped: [] },
      { labelAt: String, nameOf: String },
    ),
    "No musician's mix was restored from the backup.",
  );
});

test("Restore asks first; only the confirming Restore restores, and Cancel or a new list drops it", () => {
  const emitted = [];
  const panel = createMixBackupsPanel({ emit: (type, detail) => emitted.push([type, detail]) });
  panel.setRows(
    mixBackupRows([
      backup("20261018T190405000Z"),
      backup("20261018T190200000Z", { problem: "damaged" }),
    ]),
  );

  panel.ask("20261018T190200000Z");
  assert.equal(panel.asking(), null, "a backup that cannot be restored is not asked about");
  panel.ask("20261018T190405000Z");
  assert.equal(panel.asking()?.stamp, "20261018T190405000Z");
  assert.deepEqual(emitted, [], "nothing restores before the engineer confirms");
  panel.cancel();
  assert.equal(panel.asking(), null);

  panel.ask("20261018T190405000Z");
  panel.confirm();
  assert.deepEqual(emitted, [["mix-backup-restore", { stamp: "20261018T190405000Z" }]]);
  assert.equal(panel.asking(), null);

  panel.ask("20261018T190405000Z");
  panel.setRows(mixBackupRows([backup("20261018T190500000Z")]));
  assert.equal(panel.asking(), null, "a backup no longer listed is no longer asked about");
});

// A console connected to a running fake whose clock the test moves.
async function connected(options = {}) {
  const clock = { now: 1_000_000 };
  const fake = createFakeBridge({ running: true, clock: () => clock.now, ...options });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  return { fake, app, clock };
}

test("the console lists the backups once connected and the fake backs a mix change up once it is still", async () => {
  const { fake, app, clock } = await connected();
  assert.deepEqual(app.store.getState().mixBackups, { list: [], error: null });

  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [11] } });
  clock.now += BACKUP_QUIET_MS - 1;
  const early = await app.loadMixBackups();
  assert.equal(early.ok, true);
  assert.equal(app.store.getState().mixBackups.list.length, 0, "not still long enough yet");

  clock.now += 1;
  await app.loadMixBackups();
  const [first] = app.store.getState().mixBackups.list;
  assert.equal(first.musicians, 4);
  assert.equal(first.problem, null);
  assert.equal(first.names.length, 4, "what the desk calls each of them");

  // Under constant change a backup is taken at the latest the longest wait after the first change.
  for (let step = 0; step < 30; step += 1) {
    await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [step] } });
    clock.now += 1000;
  }
  await app.loadMixBackups();
  assert.equal(app.store.getState().mixBackups.list.length, 2);
  assert.ok(BACKUP_MAX_WAIT_MS < 30_000);
  app.dispose();
});

test("only the newest backups are kept, and an unchanged one is not taken again", async () => {
  const { fake, app, clock } = await connected();
  for (let step = 0; step < KEPT_BACKUPS + 3; step += 1) {
    await fake.invoke("set_mix", { address: LEAD, mix: { volumePercent: step } });
    clock.now += BACKUP_QUIET_MS;
    await app.loadMixBackups();
  }
  assert.equal(app.store.getState().mixBackups.list.length, KEPT_BACKUPS);

  // The same mix again: nothing new to back up.
  await fake.invoke("set_mix", { address: LEAD, mix: { volumePercent: KEPT_BACKUPS + 2 } });
  clock.now += BACKUP_QUIET_MS;
  await app.loadMixBackups();
  const newest = app.store.getState().mixBackups.list[0];
  assert.equal(app.store.getState().mixBackups.list.length, KEPT_BACKUPS);
  assert.ok(newest.savedAt < clock.now, "the newest is still the one before");
  app.dispose();
});

test("a restore puts the backed-up mixes back on the musicians connected and says so", async () => {
  const { fake, app, clock } = await connected();
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [11] } });
  clock.now += BACKUP_QUIET_MS;
  await app.loadMixBackups();
  const [kept] = app.store.getState().mixBackups.list;
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [99] } });

  const restored = await app.restoreMixBackup(kept.stamp);

  assert.equal(restored.ok, true, restored.message);
  assert.match(restored.message, /^Restored the mixes backed up at .+ to /);
  const lead = app.store.getState().musicians.find((musician) => musician.address === LEAD);
  assert.equal(lead.mix.channelGains[0], 11, "the backed-up mix is the one now");
  assert.ok(
    fake.calls.some(({ command }) => command === "restore_mix_backup"),
    "through the desk's command",
  );
  app.dispose();
});

test("a musician not connected is skipped, and a restore with no engine or a bad stamp changes nothing", async () => {
  const { fake, app } = await connected({
    mixBackups: [
      {
        stamp: "20261018T190405000Z",
        savedAt: 5,
        musicians: [
          {
            key: "client:not-here",
            name: "Keys",
            clientId: "not-here",
            address: "192.168.1.99:50000",
            mix: { channels: [], groups: {}, volumePercent: 40, muted: false },
          },
        ],
        problem: null,
      },
      { stamp: "20261018T190300000Z", savedAt: null, musicians: [], problem: "it is damaged" },
    ],
  });

  const listed = app.store.getState().mixBackups.list;
  assert.deepEqual(
    listed.map(({ stamp, problem }) => ({ stamp, problem })),
    [
      { stamp: "20261018T190405000Z", problem: null },
      { stamp: "20261018T190300000Z", problem: "it is damaged" },
    ],
  );
  const away = await app.restoreMixBackup("20261018T190405000Z");
  assert.equal(away.ok, true);
  assert.match(away.message, /Not restored to Keys: not here\./);

  const damaged = await app.restoreMixBackup("20261018T190300000Z");
  assert.equal(damaged.ok, false);
  assert.match(damaged.message, /^The mixes were not restored: .*it is damaged/);

  const outside = await app.restoreMixBackup("../sessions/s1");
  assert.equal(outside.ok, false);
  assert.match(outside.message, /is not a mix backup/);

  await fake.invoke("stop_engine");
  const idle = await app.restoreMixBackup("20261018T190405000Z");
  assert.equal(idle.ok, false);
  assert.match(idle.message, /the engine is not running/);
  app.dispose();
});

test("a failed list keeps the backups known and says why", async () => {
  const { fake, app } = await connected();
  fake.failNext("list_mix_backups", "the folder is locked");
  const outcome = await app.loadMixBackups();
  assert.deepEqual(outcome, {
    ok: false,
    message: "The mix backups could not be listed: the folder is locked.",
  });
  assert.deepEqual(app.store.getState().mixBackups, { list: [], error: "the folder is locked" });
  app.dispose();
});

test("the Session panel's backup events run the console's commands and say a restore", async () => {
  const { fake, app, clock } = await connected();
  await fake.invoke("set_mix", { address: LEAD, mix: { channelGains: [11] } });
  clock.now += BACKUP_QUIET_MS;
  const said = [];
  const answered = [];
  const commands = sessionPanelCommands({
    app,
    panel: { answered: (kind, ok) => answered.push([kind, ok]) },
    said: (outcome) => said.push(outcome),
  });
  assert.ok(commands.types.includes("mix-backups-load"));
  assert.ok(commands.types.includes("mix-backup-restore"));

  await commands.handle("mix-backups-load", {});
  assert.deepEqual(said, [], "a list that worked says nothing");
  const [kept] = app.store.getState().mixBackups.list;
  await commands.handle("mix-backup-restore", { stamp: kept.stamp });
  assert.equal(said.length, 1);
  assert.equal(said[0].ok, true);
  assert.deepEqual(answered.at(-1), ["restore", true]);
  app.dispose();
});
