import test from "node:test";
import assert from "node:assert/strict";

import {
  actions,
  consoleReducer,
  initialState,
  isLiveRun,
  musicianByAddress,
  musiciansNeedingAttention,
  pendingEdits,
  selectedMusician,
  viewMix,
} from "../app/lib/console-state.js";
import { defaultMix } from "../app/lib/mix.js";

const ANA = "192.168.1.3:50000";
const BO = "192.168.1.12:50000";
const CY = "192.168.1.20:50000";

function row(address, extra = {}) {
  return {
    address,
    clientName: null,
    name: null,
    nameIsLocal: false,
    controlConnected: true,
    ...defaultMix(),
    packetsSent: 0,
    packetsDiscarded: 0,
    linkHealth: "good",
    packetsPerSecond: 200,
    underrunsPerSecond: 0,
    lossPercent: 0,
    receiveTimeoutsPerSecond: 0,
    ...extra,
  };
}

function status(musicians, extra = {}) {
  return {
    run: 1,
    deviceName: "Volt 4",
    captureFormat: { channels: 4, sampleRate: 48000, sampleFormat: "I24", bufferSize: "Fixed(256)" },
    controlPort: 50001,
    sampleRate: 48000,
    sourceChannels: 4,
    groups: [],
    channels: [],
    stopped: false,
    samplesReceived: 0,
    packetsSent: 0,
    packetsDiscarded: 0,
    musicians,
    ...extra,
  };
}

function reduce(list, state = initialState) {
  return list.reduce(consoleReducer, state);
}

function mixWith(changes) {
  return { ...defaultMix(), ...changes };
}

const twoMusicians = actions.status(status([row(ANA, { name: "Ana" }), row(BO)]));

test("the console starts idle with no musicians and nobody selected", () => {
  assert.equal(initialState.engine.phase, "idle");
  assert.equal(initialState.engine.status, null);
  assert.deepEqual(initialState.musicians, []);
  assert.equal(initialState.selected, null);
  assert.equal(selectedMusician(initialState), null);
});

test("a status fills in the engine and the musicians in its order, and selects the first", () => {
  const state = reduce([twoMusicians]);

  assert.equal(state.engine.phase, "running");
  assert.equal(state.engine.status.deviceName, "Volt 4");
  assert.equal("musicians" in state.engine.status, false, "musicians live beside the engine");
  assert.deepEqual(
    state.musicians.map((musician) => musician.address),
    [ANA, BO],
  );
  assert.equal(state.selected, ANA);
  assert.equal(selectedMusician(state).name, "Ana");
  assert.equal(musicianByAddress(state, BO).linkHealth, "good");
  assert.deepEqual(musicianByAddress(state, BO).mix, defaultMix());
  assert.equal("volumePercent" in musicianByAddress(state, BO), false, "the mix is nested");
});

test("the selection stays with its musician and falls back to the first when it leaves", () => {
  const picked = reduce([twoMusicians, actions.select(BO)]);
  assert.equal(picked.selected, BO);

  const stillThere = reduce([actions.status(status([row(ANA), row(BO)]))], picked);
  assert.equal(stillThere.selected, BO);

  const left = reduce([actions.status(status([row(ANA)]))], picked);
  assert.equal(left.selected, ANA);

  const nobody = reduce([actions.status(status([]))], picked);
  assert.equal(nobody.selected, null);
});

test("selecting a musician that is not listed changes nothing", () => {
  const state = reduce([twoMusicians]);

  assert.equal(consoleReducer(state, actions.select(CY)), state);
  assert.equal(consoleReducer(state, actions.select(ANA)), state, "already selected");
});

test("a stopped engine's status says so", () => {
  const state = reduce([actions.status(status([], { stopped: true }))]);

  assert.equal(state.engine.phase, "stopped");
});

test("the engine phase follows start and stop, and keeps the error text", () => {
  const starting = reduce([actions.engine("starting")]);
  assert.equal(starting.engine.phase, "starting");
  assert.equal(starting.engine.error, null);

  const failed = reduce([actions.engine("error", "no input device")], starting);
  assert.equal(failed.engine.phase, "error");
  assert.equal(failed.engine.error, "no input device");

  const running = reduce([twoMusicians], failed);
  assert.equal(running.engine.phase, "running");
  assert.equal(running.engine.error, null, "a running engine has no error to show");

  assert.throws(() => consoleReducer(running, actions.engine("dancing")), /phase/);
});

test("a stopping engine stays stopping while its last statuses arrive", () => {
  const state = reduce([twoMusicians, actions.engine("stopping"), twoMusicians]);

  assert.equal(state.engine.phase, "stopping");
});

test("an idle engine forgets its snapshot, musicians and edits, but not whose mix was open", () => {
  const busy = reduce([
    twoMusicians,
    actions.select(BO),
    actions.mixEdit(BO, { volumePercent: 80 }),
    actions.engine("idle"),
  ]);

  assert.equal(busy.engine.phase, "idle");
  assert.equal(busy.engine.status, null);
  assert.deepEqual(busy.musicians, []);
  assert.deepEqual(busy.pending, {});
  assert.equal(busy.selected, BO);
  assert.equal(selectedMusician(busy), null);

  const back = reduce(
    [actions.engine("starting"), actions.status(status([row(ANA), row(BO)], { run: 2 }))],
    busy,
  );
  assert.equal(back.selected, BO, "the same musician opens again after a restart");
});

test("a late status from an engine that stopped does not bring it back", () => {
  const stopped = reduce([twoMusicians, actions.engine("stopping"), actions.engine("idle")]);

  assert.equal(consoleReducer(stopped, twoMusicians), stopped);
  assert.equal(stopped.engine.phase, "idle");
});

test("a late status from the engine a failed start replaced does not bring it back", () => {
  const failed = reduce([
    twoMusicians,
    actions.engine("starting"),
    actions.engine("error", "No input device is available."),
  ]);

  assert.equal(consoleReducer(failed, twoMusicians), failed);
  assert.equal(failed.engine.phase, "error");
  assert.equal(failed.engine.error, "No input device is available.");

  const next = reduce([actions.status(status([row(ANA)], { run: 2 }))], failed);
  assert.equal(next.engine.phase, "running", "a status of the next run is taken");
  assert.equal(next.engine.run, 2);
});

test("a status of the new run is taken before the start's answer, and the old run's are not", () => {
  const restarting = reduce([twoMusicians, actions.engine("starting")]);

  const late = consoleReducer(restarting, twoMusicians);
  assert.equal(late, restarting, "run 1 ended when the restart began");

  const fresh = reduce([actions.status(status([row(BO)], { run: 2 }))], restarting);
  assert.equal(fresh.engine.phase, "running");
  assert.deepEqual(
    fresh.musicians.map((musician) => musician.address),
    [BO],
  );
  assert.equal(consoleReducer(fresh, twoMusicians), fresh, "an older run never wins");
});

test("a late mix change from an ended run is ignored", () => {
  const stopped = reduce([twoMusicians, actions.engine("starting")]);
  const restarted = reduce([actions.status(status([row(BO)], { run: 2 }))], stopped);

  const late = actions.mixChanged({
    run: 1,
    address: BO,
    origin: "phone",
    mix: mixWith({ volumePercent: 5 }),
  });

  assert.equal(consoleReducer(restarted, late), restarted);
});

test("meters are live only for the current run, or for a newer one", () => {
  const running = reduce([twoMusicians]);
  const restarting = reduce([actions.engine("starting")], running);

  assert.equal(isLiveRun(running, 1), true);
  assert.equal(isLiveRun(running, 0), false);
  assert.equal(isLiveRun(running, undefined), false, "a payload without a run is not trusted");
  assert.equal(isLiveRun(restarting, 1), false);
  assert.equal(isLiveRun(restarting, 2), true);
});

function change(address, origin, mix) {
  return actions.mixChanged({ run: 1, address, origin, mix });
}

test("a mix change replaces that musician's mix, from the phone or from this desk", () => {
  const state = reduce([twoMusicians, change(BO, "phone", mixWith({ volumePercent: 30 }))]);

  assert.equal(viewMix(state, BO).volumePercent, 30);
  assert.equal(viewMix(state, ANA).volumePercent, 100);

  const fromDesk = reduce([change(BO, "desktop", mixWith({ volumePercent: 55 }))], state);
  assert.equal(viewMix(fromDesk, BO).volumePercent, 55);
});

test("a mix change for a musician not listed changes nothing", () => {
  const state = reduce([twoMusicians]);

  assert.equal(consoleReducer(state, change(CY, "phone", defaultMix())), state);
  assert.equal(viewMix(state, CY), null);
});

test("local edits show over the server's mix until they are answered", () => {
  const state = reduce([twoMusicians, actions.mixEdit(BO, { "channelGains.1": 40 })]);

  assert.equal(viewMix(state, BO).channelGains[1], 40);
  assert.equal(musicianByAddress(state, BO).mix.channelGains[1], 100, "the server's mix is kept");
  assert.deepEqual(pendingEdits(state, BO), { "channelGains.1": 40 });
  assert.deepEqual(pendingEdits(state, ANA), {});
});

test("a later phone change does not hide an edit still waiting for its answer", () => {
  const phoneMix = mixWith({ volumePercent: 30 });
  phoneMix.channelGains[1] = 90;

  const state = reduce([
    twoMusicians,
    actions.mixEdit(BO, { "channelGains.1": 40 }),
    change(BO, "phone", phoneMix),
  ]);

  assert.equal(viewMix(state, BO).channelGains[1], 40);
  assert.equal(viewMix(state, BO).volumePercent, 30, "the phone's other changes show");
});

test("an answer settles the edits it carried, and an edit made since keeps showing", () => {
  const state = reduce([
    twoMusicians,
    actions.mixEdit(BO, { "channelGains.1": 40 }),
    actions.mixEdit(BO, { "channelGains.1": 45 }),
    actions.mixApplied(BO, { "channelGains.1": 40 }),
  ]);

  assert.equal(viewMix(state, BO).channelGains[1], 45);
  assert.deepEqual(pendingEdits(state, BO), { "channelGains.1": 45 });

  const engineMix = mixWith({});
  engineMix.channelGains[1] = 45;
  const settled = reduce(
    [
      actions.mixChanged({ run: 1, address: BO, origin: "desktop", mix: engineMix }),
      actions.mixApplied(BO, { "channelGains.1": 45 }),
    ],
    state,
  );
  assert.equal(BO in settled.pending, false);
  assert.equal(viewMix(settled, BO).channelGains[1], 45);
});

test("an answer never replaces the engine's mix, so a phone change that came after it stays", () => {
  const phoneMix = mixWith({ volumePercent: 30 });
  phoneMix.channelGains[1] = 90;
  const answerMix = mixWith({});
  answerMix.channelGains[1] = 40;

  const state = reduce([
    twoMusicians,
    actions.mixEdit(BO, { "channelGains.1": 40 }),
    // The engine applied the desk's change, then the phone's; the events come in that order...
    actions.mixChanged({ run: 1, address: BO, origin: "desktop", mix: answerMix }),
    actions.mixChanged({ run: 1, address: BO, origin: "phone", mix: phoneMix }),
    // ...and the answer to the desk's request, holding the mix as it was then, comes last.
    { ...actions.mixApplied(BO, { "channelGains.1": 40 }), mix: answerMix },
  ]);

  assert.equal(viewMix(state, BO).channelGains[1], 90, "the phone's later change stays");
  assert.equal(viewMix(state, BO).volumePercent, 30);
  assert.equal(BO in state.pending, false);
});

test("a refused change falls back to the server's mix and keeps the error for that musician", () => {
  const refused = reduce([
    twoMusicians,
    actions.mixEdit(BO, { volumePercent: 80 }),
    actions.mixFailed(BO, { volumePercent: 80 }, "the engine is not running"),
  ]);

  assert.equal(viewMix(refused, BO).volumePercent, 100);
  assert.deepEqual(refused.mixError, { address: BO, message: "the engine is not running" });

  const recovered = reduce(
    [actions.mixEdit(BO, { volumePercent: 70 }), actions.mixApplied(BO, { volumePercent: 70 })],
    refused,
  );
  assert.equal(recovered.mixError, null);
});

test("edits for a musician who is not listed are ignored, and a bad edit fails loudly", () => {
  const state = reduce([twoMusicians]);

  assert.equal(consoleReducer(state, actions.mixEdit(CY, { volumePercent: 80 })), state);
  assert.equal(consoleReducer(state, actions.mixEdit(BO, {})), state);
  assert.throws(() => consoleReducer(state, actions.mixEdit(BO, { volume: 80 })), /mix edit/);
});

test("a status keeps the edits of musicians still listed and drops those of musicians who left", () => {
  const state = reduce([
    twoMusicians,
    actions.mixEdit(ANA, { muted: true }),
    actions.mixEdit(BO, { muted: true }),
    actions.status(status([row(ANA)])),
  ]);

  assert.deepEqual(pendingEdits(state, ANA), { muted: true });
  assert.equal(BO in state.pending, false);
  assert.equal(viewMix(state, ANA).muted, true);
});

test("musicians whose link is warning or bad need attention", () => {
  const state = reduce([
    actions.status(
      status([
        row(ANA),
        row(BO, { linkHealth: "warning" }),
        row(CY, {
          linkHealth: "bad",
          packetsPerSecond: null,
          underrunsPerSecond: null,
          lossPercent: null,
          receiveTimeoutsPerSecond: null,
        }),
      ]),
    ),
  ]);

  assert.deepEqual(
    musiciansNeedingAttention(state).map((musician) => musician.address),
    [BO, CY],
  );
});

test("an action the console does not know leaves the state as it was", () => {
  const state = reduce([twoMusicians]);

  assert.equal(consoleReducer(state, { type: "something-else" }), state);
});
