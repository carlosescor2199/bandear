import test from "node:test";
import assert from "node:assert/strict";

import {
  actions,
  consoleReducer,
  initialState,
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

  const back = reduce([twoMusicians], busy);
  assert.equal(back.selected, BO, "the same musician opens again after a restart");
});

test("a mix change replaces that musician's mix, from the phone or from this desk", () => {
  const state = reduce([
    twoMusicians,
    actions.mixChanged({ address: BO, origin: "phone", mix: mixWith({ volumePercent: 30 }) }),
  ]);

  assert.equal(viewMix(state, BO).volumePercent, 30);
  assert.equal(viewMix(state, ANA).volumePercent, 100);

  const fromDesk = reduce(
    [actions.mixChanged({ address: BO, origin: "desktop", mix: mixWith({ volumePercent: 55 }) })],
    state,
  );
  assert.equal(viewMix(fromDesk, BO).volumePercent, 55);
});

test("a mix change for a musician not listed changes nothing", () => {
  const state = reduce([twoMusicians]);

  assert.equal(
    consoleReducer(state, actions.mixChanged({ address: CY, origin: "phone", mix: defaultMix() })),
    state,
  );
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
    actions.mixChanged({ address: BO, origin: "phone", mix: phoneMix }),
  ]);

  assert.equal(viewMix(state, BO).channelGains[1], 40);
  assert.equal(viewMix(state, BO).volumePercent, 30, "the phone's other changes show");
});

test("an answer settles the edits it carried, and an edit made since keeps showing", () => {
  const answered = mixWith({});
  answered.channelGains[1] = 40;

  const state = reduce([
    twoMusicians,
    actions.mixEdit(BO, { "channelGains.1": 40 }),
    actions.mixEdit(BO, { "channelGains.1": 45 }),
    actions.mixApplied(BO, { "channelGains.1": 40 }, answered),
  ]);

  assert.equal(musicianByAddress(state, BO).mix.channelGains[1], 40);
  assert.equal(viewMix(state, BO).channelGains[1], 45);
  assert.deepEqual(pendingEdits(state, BO), { "channelGains.1": 45 });

  const final = mixWith({});
  final.channelGains[1] = 45;
  const settled = reduce([actions.mixApplied(BO, { "channelGains.1": 45 }, final)], state);
  assert.equal("192.168.1.12:50000" in settled.pending, false);
  assert.equal(viewMix(settled, BO).channelGains[1], 45);
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
    [
      actions.mixEdit(BO, { volumePercent: 70 }),
      actions.mixApplied(BO, { volumePercent: 70 }, mixWith({ volumePercent: 70 })),
    ],
    refused,
  );
  assert.equal(recovered.mixError, null);
  assert.equal(viewMix(recovered, BO).volumePercent, 70);
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
