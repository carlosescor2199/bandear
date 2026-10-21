// The master limiter on every mix: its settings held in range, the master strip's view of it (the
// change still on its way over the status's, the ceiling note, the slider positions), a control's
// change (a slider that always moves at least one step), the gain reduction from the meters, the
// outcome in words (limiter.js), the bridge's `setMusicianLimiter` and `setLocalLimiter`, the fake
// bridge's commands (kept per musician and in the local section, as the desktop does, with gain
// reduction on a hot mix) and the console's command.
import test from "node:test";
import assert from "node:assert/strict";

import { createEngineApi } from "../app/lib/bridge.js";
import { createConsoleApp } from "../app/lib/console-app.js";
import { LOCAL_MIX } from "../app/lib/console-state.js";
import { createFakeBridge, sampleStatus, syntheticMeters } from "../app/lib/fake-bridge.js";
import {
  CEILING_FIELD,
  LIMITER_REDUCTION_SCALE_DB,
  RELEASE_FIELD,
  clampLimiter,
  defaultLimiter,
  formatCeiling,
  formatLimiterSet,
  limiterChange,
  limiterPosition,
  limiterReduction,
  limiterView,
  masterLimiter,
  sameLimiter,
} from "../app/lib/limiter.js";

const NOW = Date.UTC(2026, 9, 20, 18, 0, 0);
const LEAD = "192.168.1.3:50000";
const BASS = "192.168.1.12:50000";
const LEAD_KEY = "client:5f0c2a8e-1b7d-4c3a-9e6f-2d8b4a7c1e90";
const BASS_KEY = "client:a3d9e7b1-6c2f-4e8a-b5d0-7f1c9e3a2b64";

test("a limiter's settings are held in range, anything unreadable taking the default", () => {
  assert.deepEqual(defaultLimiter(), { enabled: false, ceilingDb: 0, releaseMs: 50 });
  assert.deepEqual(clampLimiter({ enabled: true, ceilingDb: -30, releaseMs: 5000 }), {
    enabled: true,
    ceilingDb: -24,
    releaseMs: 1000,
  });
  assert.deepEqual(clampLimiter({ enabled: 1, ceilingDb: 3, releaseMs: 0 }), {
    enabled: true,
    ceilingDb: 0,
    releaseMs: 1,
  });
  assert.deepEqual(clampLimiter({ ceilingDb: "x", releaseMs: null }), defaultLimiter());
  assert.deepEqual(clampLimiter(null), defaultLimiter());
  assert.equal(sameLimiter({ enabled: true, ceilingDb: -3, releaseMs: 80 }, { enabled: true, ceilingDb: -3, releaseMs: 80 }), true);
  assert.equal(sameLimiter({ enabled: true, ceilingDb: -3, releaseMs: 80 }, { enabled: false, ceilingDb: -3, releaseMs: 80 }), false);
});

test("a ceiling is said in dBFS with a true minus", () => {
  assert.equal(formatCeiling(0), "0 dBFS");
  assert.equal(formatCeiling(-6), "−6 dBFS");
  assert.equal(formatCeiling(-6.5), "−6.5 dBFS");
  assert.equal(formatCeiling(-0.04), "0 dBFS");
});

test("the strip's view says the state, the ceilings that win and the reduction only while on", () => {
  const on = limiterView({ enabled: true, ceilingDb: -3, releaseMs: 120 }, 4.25, 90);
  assert.equal(on.state, "Limiter on");
  assert.equal(on.ceiling, "−3 dBFS");
  assert.equal(on.release, "120 ms");
  assert.equal(on.note, "Peaks are held at −3 dBFS or the mix's ceiling (90%), whichever is lower. No added latency.");
  assert.deepEqual(on.reduction, { fraction: 4.25 / LIMITER_REDUCTION_SCALE_DB, text: "−4.3 dB" });
  assert.equal(on.ceilingPosition, limiterPosition(CEILING_FIELD, -3));
  assert.equal(on.releasePosition, limiterPosition(RELEASE_FIELD, 120));

  const local = limiterView({ enabled: true, ceilingDb: 0, releaseMs: 50 }, 0, null);
  assert.ok(local.note.includes("the mix's full scale"), local.note);

  const off = limiterView({ enabled: false, ceilingDb: -3, releaseMs: 120 }, 6, 90);
  assert.equal(off.state, "Limiter off");
  assert.equal(off.note, "Off: the mix is only clipped at its ceiling.");
  assert.deepEqual(off.reduction, { fraction: 0, text: "0.0 dB" });
});

test("a control's change gives the whole limiter, and a slider always moves at least one step", () => {
  const settings = { enabled: false, ceilingDb: -6, releaseMs: 50 };
  assert.deepEqual(limiterChange(settings, "enabled", true), { ...settings, enabled: true });
  // A drag lands on the value at its position, rounded as the strip shows it.
  assert.deepEqual(limiterChange(settings, "ceilingDb", 500), { ...settings, ceilingDb: -12 });
  assert.deepEqual(limiterChange(settings, "releaseMs", 1000), { ...settings, releaseMs: 1000 });
  assert.deepEqual(limiterChange(settings, "releaseMs", 0), { ...settings, releaseMs: 1 });
  // An arrow key moves the position by one of 1,000 steps: less than the rounding, so it moves the
  // value by one unit (0.5 dB, 1 ms) in its direction instead of staying put.
  const ceilingAt = limiterPosition(CEILING_FIELD, -6);
  assert.equal(limiterChange(settings, "ceilingDb", ceilingAt + 1).ceilingDb, -5.5);
  assert.equal(limiterChange(settings, "ceilingDb", ceilingAt - 1).ceilingDb, -6.5);
  const short = { ...settings, releaseMs: 1 };
  const releaseAt = limiterPosition(RELEASE_FIELD, 1);
  assert.equal(limiterChange(short, "releaseMs", releaseAt + 1).releaseMs, 2);
  assert.equal(limiterChange(short, "releaseMs", releaseAt - 1).releaseMs, 1, "held at the end");
  assert.equal(limiterChange({ ...settings, ceilingDb: 0 }, "ceilingDb", 1000).ceilingDb, 0);
  // The same position changes nothing.
  assert.deepEqual(limiterChange(settings, "ceilingDb", ceilingAt), settings);
  assert.deepEqual(limiterChange(settings, "nothing", 3), settings);
});

test("the gain reduction comes from the meters of the open mix", () => {
  const meters = {
    musicians: { [LEAD]: { left: 40, right: 41, limiterReductionDb: 3.2 }, [BASS]: { left: 1, right: 1 } },
    local: { left: 20, right: 20, limiterReductionDb: 1.5 },
  };
  assert.equal(limiterReduction(meters, LEAD), 3.2);
  assert.equal(limiterReduction(meters, BASS), 0, "a mix that reports none");
  assert.equal(limiterReduction(meters, LOCAL_MIX), 1.5);
  assert.equal(limiterReduction(meters, "10.0.0.9:50000"), 0);
  assert.equal(limiterReduction({ local: { limiterReductionDb: -2 } }, LOCAL_MIX), 0);
  assert.equal(limiterReduction(null, LEAD), 0);
});

test("the outcome is said with the settings in force and a save that failed", () => {
  const answer = {
    musicianKey: LEAD_KEY,
    outcome: "set",
    settings: { enabled: true, ceilingDb: -1.5, releaseMs: 80 },
    saved: true,
    saveError: null,
    message: "This musician's limiter is on.",
  };
  assert.deepEqual(formatLimiterSet(answer, "Lead vocal"), {
    ok: true,
    message: "Lead vocal's limiter is on: peaks are held at −1.5 dBFS, released over 80 ms.",
  });
  assert.deepEqual(
    formatLimiterSet({ ...answer, settings: defaultLimiter(), saved: false, saveError: "disk full" }, "The local mix"),
    {
      ok: true,
      message: "The local mix's limiter is off. It was not saved for the next start: disk full.",
    },
  );
  assert.equal(formatLimiterSet({ outcome: "other", message: "No." }, "Bass").ok, false);
  assert.equal(formatLimiterSet(null, "Bass").message, "Bass's limiter was not set.");
});

test("setMusicianLimiter and setLocalLimiter ask the desktop for each mix's limiter", async () => {
  const calls = [];
  const api = createEngineApi({
    invoke: async (command, args) => {
      calls.push([command, args]);
      return null;
    },
    listen: async () => () => {},
  });
  const settings = { enabled: true, ceilingDb: -2, releaseMs: 60 };

  await api.setMusicianLimiter(BASS_KEY, settings);
  await api.setLocalLimiter(settings);

  assert.deepEqual(calls, [
    ["set_musician_limiter", { musicianKey: BASS_KEY, settings }],
    ["set_local_limiter", { settings }],
  ]);
});

test("the fake keeps each musician's limiter and the local mix's across starts", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  const status = await api.status();
  assert.deepEqual(status.musicians[1].limiter, defaultLimiter());
  assert.deepEqual(status.localLimiter, defaultLimiter());

  const set = await api.setMusicianLimiter(BASS_KEY, { enabled: true, ceilingDb: -40, releaseMs: 80 });
  assert.deepEqual(set, {
    musicianKey: BASS_KEY,
    outcome: "set",
    settings: { enabled: true, ceilingDb: -24, releaseMs: 80 },
    saved: true,
    saveError: null,
    message: "This musician's limiter is on: peaks are held at -24 dBFS, released over 80 ms.",
  });
  const local = await api.setLocalLimiter({ enabled: true, ceilingDb: -3, releaseMs: 200 });
  assert.equal(local.musicianKey, null);
  assert.equal(local.message, "The local mix's limiter is on: peaks are held at -3 dBFS, released over 200 ms.");

  await api.stop();
  await api.start();
  const after = await api.status();
  assert.deepEqual(after.musicians[1].limiter, { enabled: true, ceilingDb: -24, releaseMs: 80 });
  assert.deepEqual(after.musicians[0].limiter, defaultLimiter(), "only Bass's");
  assert.deepEqual(after.localLimiter, { enabled: true, ceilingDb: -3, releaseMs: 200 });

  // The default is kept as nothing.
  await api.setMusicianLimiter(BASS_KEY, defaultLimiter());
  assert.deepEqual((await api.status()).musicians[1].limiter, defaultLimiter());
  assert.deepEqual(fake.savedLocal().limiter, { enabled: true, ceilingDb: -3, releaseMs: 200 });
  await api.setLocalLimiter(defaultLimiter());
  assert.equal(fake.savedLocal().limiter ?? null, null);
  assert.deepEqual((await api.status()).localLimiter, defaultLimiter());

  await assert.rejects(api.setMusicianLimiter("Bass", defaultLimiter()), (error) =>
    String(error).includes("names no phone or address"),
  );
  await assert.rejects(api.setMusicianLimiter("client:nobody", defaultLimiter()), (error) =>
    String(error).includes("is not here"),
  );
  const stopped = createEngineApi(createFakeBridge({ clock: () => NOW }));
  await assert.rejects(stopped.setMusicianLimiter(BASS_KEY, defaultLimiter()), (error) => error === "the engine is not running");
  await assert.rejects(stopped.setLocalLimiter(defaultLimiter()), (error) => error === "the engine is not running");
});

test("a named session keeps the limiters, and loading one replaces them", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const api = createEngineApi(fake);
  await api.setMusicianLimiter(BASS_KEY, { enabled: true, ceilingDb: -6, releaseMs: 50 });
  await api.setLocalLimiter({ enabled: true, ceilingDb: -1, releaseMs: 50 });
  const saved = await api.saveSession("limited");
  await api.setMusicianLimiter(BASS_KEY, defaultLimiter());
  await api.setMusicianLimiter(LEAD_KEY, { enabled: true, ceilingDb: -2, releaseMs: 50 });
  await api.setLocalLimiter(defaultLimiter());

  await api.loadSession(saved.session.id);

  const status = await api.status();
  assert.deepEqual(status.musicians[1].limiter, { enabled: true, ceilingDb: -6, releaseMs: 50 });
  assert.deepEqual(status.musicians[0].limiter, defaultLimiter(), "not kept by the loaded session");
  assert.deepEqual(status.localLimiter, { enabled: true, ceilingDb: -1, releaseMs: 50 });
});

test("the fake's meters show gain reduction on a hot mix while its limiter is on", () => {
  const status = sampleStatus();
  const plain = syntheticMeters(status, 1000);
  assert.equal(plain.musicians[BASS].limiterReductionDb, 0, "off");
  assert.equal(plain.local.limiterReductionDb, 0);

  status.musicians[1].limiter = { enabled: true, ceilingDb: -24, releaseMs: 50 };
  status.localLimiter = { enabled: true, ceilingDb: -24, releaseMs: 50 };
  const limited = syntheticMeters(status, 1000);
  const ceiling = 100 * 10 ** (-24 / 20);
  const bass = limited.musicians[BASS];
  assert.ok(bass.limiterReductionDb > 0, `Bass's mix is hot: ${bass.limiterReductionDb}`);
  assert.ok(bass.left <= ceiling + 1e-9 && bass.right <= ceiling + 1e-9, "held under the ceiling");
  assert.ok(limited.local.limiterReductionDb > 0);
  assert.deepEqual(limited.musicians[LEAD], plain.musicians[LEAD], "another mix is untouched");
});

test("the master strip shows the mix's limiter, the change on its way first", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const app = createConsoleApp({ bridge: fake });
  await app.ready;
  const state = () => app.store.getState();

  const lead = masterLimiter(state(), LEAD);
  assert.equal(lead.pending, false);
  assert.equal(lead.state, "Limiter off");
  assert.equal(lead.ceilingDb, 0);
  // Lead vocal's phone caps at 90%: the note names that ceiling.
  const on = await app.editMixLimiter(LEAD, { enabled: true, ceilingDb: -3, releaseMs: 50 });
  assert.ok(masterLimiter(state(), LEAD).note.includes("the mix's ceiling (90%)"), masterLimiter(state(), LEAD).note);
  assert.equal(on.ok, true);
  assert.equal(masterLimiter(state(), LOCAL_MIX).note, "Off: the mix is only clipped at its ceiling.");
  assert.equal(masterLimiter(state(), "10.0.0.9:50000"), null, "no such mix");

  // Shown at once, before the desk answers; the latest change is the one that stays.
  const first = app.editMixLimiter(BASS, { enabled: true, ceilingDb: -6, releaseMs: 50 });
  const second = app.editMixLimiter(BASS, { enabled: true, ceilingDb: -9, releaseMs: 300 });
  assert.equal(masterLimiter(state(), BASS).pending, true);
  assert.equal(masterLimiter(state(), BASS).ceilingDb, -9);
  await first;
  assert.deepEqual(await second, {
    ok: true,
    message: "Bass's limiter is on: peaks are held at −9 dBFS, released over 300 ms.",
  });
  const bass = masterLimiter(state(), BASS);
  assert.equal(bass.pending, false);
  assert.deepEqual([bass.enabled, bass.ceilingDb, bass.releaseMs], [true, -9, 300], "the status read after it");

  assert.deepEqual(await app.editMixLimiter(LOCAL_MIX, { enabled: true, ceilingDb: -40, releaseMs: 50 }), {
    ok: true,
    message: "The local mix's limiter is on: peaks are held at −24 dBFS, released over 50 ms.",
  });
  assert.equal(masterLimiter(state(), LOCAL_MIX).ceilingDb, -24);

  const gone = await app.editMixLimiter("10.0.0.9:50000", defaultLimiter());
  assert.equal(gone.ok, false);
  assert.equal(gone.message, "10.0.0.9:50000's limiter was not set: 10.0.0.9:50000 is not here.");
  app.dispose?.();
});

test("the console says a limiter the desk refused and shows the status's again", async () => {
  const fake = createFakeBridge({ running: true, clock: () => NOW });
  const refusing = {
    ...fake,
    invoke: (command, args) =>
      command === "set_musician_limiter" ? Promise.reject("the engine is stopping") : fake.invoke(command, args),
  };
  const app = createConsoleApp({ bridge: refusing });
  await app.ready;
  const said = await app.editMixLimiter(BASS, { enabled: true, ceilingDb: -6, releaseMs: 50 });
  assert.deepEqual(said, { ok: false, message: "Bass's limiter was not set: the engine is stopping." });
  const bass = masterLimiter(app.store.getState(), BASS);
  assert.equal(bass.pending, false);
  assert.equal(bass.enabled, false, "the desk's own again");
  app.dispose?.();
});
