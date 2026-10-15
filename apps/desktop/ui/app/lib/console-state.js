// The console's state and its pure reducer, fed by the engine's `status` and `mix-changed` events
// and by the engineer's edits. Meters are not kept here: at 30 updates a second they go straight
// to the meter elements.
//
// State:
//   engine     { phase, error, status, run, endedRun }: phase is "idle", "starting", "running",
//              "stopping", "stopped" or "error"; status is the last `EngineStatusDto` without its
//              musicians; run is the engine run that status came from (null when none runs), and
//              endedRun the newest run known to have ended.
//   musicians  [musician] in the status's order: the status row with its mix nested as `mix`.
//   selected   the address whose mix is open. It survives a stop, so a restart reopens it.
//   pending    { address: { editKey: value } }: local edits still waiting for the engine.
//   mixError   { address, message } of the last refused change, cleared by the next answer.
//
// The mix a screen shows is the engine's mix with the pending edits on top (`viewMix`), so a fader
// never jumps back to an older value while its change is on the way. The engine's mix follows only
// the `status` and `mix-changed` events, which arrive in the engine's order; a `set_mix` answer
// only settles the edits it carried, since a phone may have changed the mix again by the time it
// arrives.
//
// Every engine payload names its run. Starting, a failed start and a stop end the current run, and
// a payload of an ended or older run is ignored, so a late event can never bring back an engine
// that has stopped.

import { needsAttention } from "./format.js";
import { MIX_FIELDS, applyEdits, mixFromDto, parseEditKey, settleEdits } from "./mix.js";

export const PHASES = Object.freeze(["idle", "starting", "running", "stopping", "stopped", "error"]);

/** The phases in which the engine of the current run is gone or being replaced. */
const RUN_ENDING_PHASES = new Set(["idle", "starting", "error"]);

const NO_EDITS = Object.freeze({});
const MIX_FIELD_SET = new Set(MIX_FIELDS);

export const initialState = Object.freeze({
  engine: Object.freeze({ phase: "idle", error: null, status: null, run: null, endedRun: 0 }),
  musicians: Object.freeze([]),
  selected: null,
  pending: NO_EDITS,
  mixError: null,
});

/** Action creators, so a screen never spells an action by hand. */
export const actions = Object.freeze({
  /** An `EngineStatusDto` from `engine_status` or the `status` event. */
  status: (status) => ({ type: "status", status }),
  /**
   * The engine's phase as a screen learns it from its own start and stop calls. "starting",
   * "idle" and "error" end the current run.
   */
  engine: (phase, error = null) => ({ type: "engine", phase, error }),
  select: (address) => ({ type: "select", address }),
  /** A `mix-changed` payload: `{ run, address, origin, mix }`. */
  mixChanged: ({ run, address, origin, mix }) => ({ type: "mix-changed", run, address, origin, mix }),
  /** Local edits to one musician's mix, keyed as `editKey` makes them. */
  mixEdit: (address, edits) => ({ type: "mix-edit", address, edits }),
  /** The engine answered a `set_mix` that carried the edits `sent`. */
  mixApplied: (address, sent) => ({ type: "mix-applied", address, sent }),
  /** A `set_mix` that carried the edits `sent` was refused. */
  mixFailed: (address, sent, error) => ({ type: "mix-failed", address, sent, error }),
});

/**
 * True when a payload of engine run `run` belongs to the engine the console shows, or to a newer
 * one whose first status has not arrived yet; false for an ended or older run, or no run at all.
 */
export function isLiveRun(state, run) {
  const { run: current, endedRun } = state.engine;
  return Number.isInteger(run) && run > endedRun && (current === null || run >= current);
}

/** A status row as the console keeps it: every field but the mix, and the mix nested. */
export function musicianFromDto(row) {
  const musician = { mix: mixFromDto(row) };
  for (const [key, value] of Object.entries(row)) {
    if (!MIX_FIELD_SET.has(key)) {
      musician[key] = value;
    }
  }
  return musician;
}

function withoutMusicians(status) {
  const { musicians: _musicians, ...engine } = status;
  return engine;
}

function onlyListed(pending, addresses) {
  const kept = Object.entries(pending).filter(([address]) => addresses.has(address));
  return kept.length === Object.keys(pending).length ? pending : Object.fromEntries(kept);
}

function receiveStatus(state, status) {
  if (!isLiveRun(state, status.run)) {
    return state;
  }
  const musicians = status.musicians.map(musicianFromDto);
  const addresses = new Set(musicians.map((musician) => musician.address));
  let phase = status.stopped ? "stopped" : "running";
  if (state.engine.phase === "stopping") {
    phase = "stopping";
  }
  return {
    ...state,
    engine: {
      ...state.engine,
      phase,
      error: phase === "running" ? null : state.engine.error,
      status: withoutMusicians(status),
      run: status.run,
    },
    musicians,
    selected: addresses.has(state.selected) ? state.selected : (musicians[0]?.address ?? null),
    pending: onlyListed(state.pending, addresses),
    mixError: state.mixError && addresses.has(state.mixError.address) ? state.mixError : null,
  };
}

function setPhase(state, { phase, error }) {
  if (!PHASES.includes(phase)) {
    throw new Error(`unknown engine phase: ${phase}`);
  }
  if (RUN_ENDING_PHASES.has(phase)) {
    // The engine of the current run is gone, or a start is replacing it: forget its snapshot and
    // never take its late payloads again.
    const { run, endedRun } = state.engine;
    return {
      ...state,
      engine: { phase, error, status: null, run: null, endedRun: Math.max(endedRun, run ?? 0) },
      musicians: [],
      pending: NO_EDITS,
      mixError: null,
    };
  }
  return { ...state, engine: { ...state.engine, phase, error } };
}

function isListed(state, address) {
  return state.musicians.some((musician) => musician.address === address);
}

function replaceMix(state, address, mix) {
  if (!isListed(state, address)) {
    return state;
  }
  return {
    ...state,
    musicians: state.musicians.map((musician) =>
      musician.address === address ? { ...musician, mix: mixFromDto(mix) } : musician,
    ),
  };
}

function withPending(state, address, edits) {
  const pending = { ...state.pending };
  if (Object.keys(edits).length === 0) {
    delete pending[address];
  } else {
    pending[address] = edits;
  }
  return pending;
}

function edit(state, address, edits) {
  const keys = Object.keys(edits);
  keys.forEach(parseEditKey);
  if (keys.length === 0 || !isListed(state, address)) {
    return state;
  }
  return {
    ...state,
    pending: withPending(state, address, { ...(state.pending[address] ?? NO_EDITS), ...edits }),
  };
}

// Only a listed musician can have pending edits (a status drops the others), so an answer for
// anyone else has nothing to settle. The answer's own copy of the mix is never adopted: it is the
// mix as it was when the request landed, and the events that followed it may already be newer.
function answered(state, { address, sent }) {
  if (!isListed(state, address)) {
    return state;
  }
  return {
    ...state,
    pending: withPending(state, address, settleEdits(state.pending[address], sent)),
    mixError: state.mixError?.address === address ? null : state.mixError,
  };
}

function refused(state, { address, sent, error }) {
  if (!isListed(state, address)) {
    return state;
  }
  return {
    ...state,
    pending: withPending(state, address, settleEdits(state.pending[address], sent)),
    // A Tauri command rejects with the command's error text; anything else carries a message.
    mixError: { address, message: error instanceof Error ? error.message : String(error) },
  };
}

export function consoleReducer(state, action) {
  switch (action.type) {
    case "status":
      return receiveStatus(state, action.status);
    case "engine":
      return setPhase(state, action);
    case "select":
      return state.selected === action.address || !isListed(state, action.address)
        ? state
        : { ...state, selected: action.address };
    case "mix-changed":
      return isLiveRun(state, action.run) ? replaceMix(state, action.address, action.mix) : state;
    case "mix-edit":
      return edit(state, action.address, action.edits);
    case "mix-applied":
      return answered(state, action);
    case "mix-failed":
      return refused(state, action);
    default:
      return state;
  }
}

export function musicianByAddress(state, address) {
  return state.musicians.find((musician) => musician.address === address) ?? null;
}

export function selectedMusician(state) {
  return musicianByAddress(state, state.selected);
}

/** The edits to `address`'s mix still waiting for the engine. */
export function pendingEdits(state, address) {
  return state.pending[address] ?? NO_EDITS;
}

/** The mix a screen shows for `address`: the engine's mix with the pending edits on top. */
export function viewMix(state, address) {
  const musician = musicianByAddress(state, address);
  if (!musician) {
    return null;
  }
  const edits = state.pending[address];
  return edits ? applyEdits(musician.mix, edits) : musician.mix;
}

/** The musicians whose link is a warning or bad, in the status's order. */
export function musiciansNeedingAttention(state) {
  return state.musicians.filter(needsAttention);
}
