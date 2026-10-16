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
//   departed   { address, label } of the open musician when a status no longer listed them and
//              the first musician opened instead; cleared by a pick, their return or a run's end.
//   pending    { address: { editKey: value } }: local edits still waiting for the engine, the
//              engineer's limit and locks among them (keys of ENGINEER_FIELDS, see mix.js).
//   mixError   { address, message } of the last refused change, cleared by the next answer.
//   controlsNotice { address, outcome }: the last `set_engineer_controls` answer for `address`
//              when it did not both apply and save; cleared by its next full success.
//   audio      { host, device, bufferFrames, asioDriver }: the driver, input, buffer and picked ASIO
//              driver the next start asks for (see engine-config.js); the desk's session keeps them.
//   audioNotice { kind, message }: a word about the audio input for the current run: "fallback"
//              when the input the choices name was missing and the engine started on the default
//              input, "unsaved" when the choices run but the session could not keep them. A run's
//              end clears it.
//   connection { status, error }: "connecting", "ready" (the engine answered, or a status came)
//              or "failed" with the reason the console cannot hear the engine.
//   commandError { action, message }: a stop or a device switch the engine refused while its
//              run went on; cleared by the next start or stop, or when that command works.
//   devices    { host, list, error }: the input devices `list_input_devices` gave for `host`.
//
// The mix a screen shows is the engine's mix with the pending edits on top (`viewMix`), so a fader
// never jumps back to an older value while its change is on the way. Each copy of a musician's mix
// carries the engine's revision, and a status row, a `mix-changed` event and a `set_mix` answer
// can arrive in any order: the store keeps the copy with the highest revision from any of them and
// never goes back to a lower one. An answer also settles the edits it carried. A status that moves
// the layout (the groups' order, an input's pairing or hiding) drops the edits whose position no
// longer means the same thing (see settleLayout).
//
// Every engine payload names its run. Starting, a failed start and a confirmed stop end the
// current run, and a payload of an ended or older run is ignored, so a late event can never bring
// back an engine that has stopped. A stop the engine refused ends nothing: the engine may still
// run, so its run stays live and the refusal is shown (`commandFailed`).

import { pairUnits } from "./channels.js";
import { DEFAULT_AUDIO, audioChoice } from "./engine-config.js";
import { musicianLabel, needsAttention } from "./format.js";
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
  departed: null,
  pending: NO_EDITS,
  mixError: null,
  controlsNotice: null,
  audio: DEFAULT_AUDIO,
  audioNotice: null,
  connection: Object.freeze({ status: "connecting", error: null }),
  commandError: null,
  devices: Object.freeze({ host: null, list: Object.freeze([]), error: null }),
});

const READY = Object.freeze({ status: "ready", error: null });

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
  /** The engine answered a `set_mix` that carried the edits `sent` with the mix it stored. */
  mixApplied: (address, sent, mix = null) => ({ type: "mix-applied", address, sent, mix }),
  /** A `set_mix` that carried the edits `sent` was refused. */
  mixFailed: (address, sent, error) => ({ type: "mix-failed", address, sent, error }),
  /**
   * The engine answered a `set_engineer_controls` that carried the engineer's edits `sent` with
   * `outcome` (`{ applied, saved, saveError, mix }`): settled like a `set_mix` answer, and kept as
   * the musician's controls notice when it did not both apply and save. A refusal is `mixFailed`.
   */
  engineerApplied: (address, sent, outcome) => ({
    type: "engineer-applied",
    address,
    sent,
    outcome,
  }),
  /**
   * A change of the audio choices for the next start: the driver (`host`), the input (`device`),
   * the buffer (`bufferFrames`) or the picked ASIO driver (`asioDriver`), see audioChoice.
   */
  audio: (change) => ({ type: "audio", change }),
  /** `{ kind, message }` about the audio input of the current run, or null to clear it. */
  audioNotice: (notice) => ({ type: "audio-notice", notice }),
  /** The input devices listed for `host`, or the reason they could not be listed. */
  devices: (host, list, error = null) => ({ type: "devices", devices: { host, list, error } }),
  /**
   * The engine refused `action` ("stop" or "switch") but its run goes on: the phase goes back to
   * what the last status said, and the refusal is kept to be shown.
   */
  commandFailed: (action, message) => ({ type: "command-failed", action, message }),
  /** `action` worked: an earlier refusal of the same command no longer holds. */
  commandDone: (action) => ({ type: "command-done", action }),
  /** The console is (re)connecting to the engine's events and status. */
  connecting: () => ({ type: "connection", connection: { status: "connecting", error: null } }),
  connected: () => ({ type: "connection", connection: READY }),
  /** The console cannot hear the engine, for `message`. */
  connectionFailed: (message) => ({
    type: "connection",
    connection: { status: "failed", error: message },
  }),
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

const CHANNEL_TABLES = new Set([
  "channelGains",
  "pans",
  "channelMuted",
  "channelSolo",
  "lockedChannels",
]);

// What each input of a status is on the Mix: "hidden", "single", or the "first" or "second" side
// of a stereo pair.
function inputRoles(status) {
  const channels = (status.channels ?? []).slice(0, status.sourceChannels);
  const roles = [];
  for (const unit of pairUnits(channels)) {
    unit.forEach((index, side) => {
      if (channels[index].hidden) {
        roles[index] = "hidden";
      } else if (unit.length === 1) {
        roles[index] = "single";
      } else {
        roles[index] = side === 0 ? "first" : "second";
      }
    });
  }
  return roles;
}

function sameGroups(before, after) {
  const ids = (status) => (status.groups ?? []).map((group) => group.id);
  const [was, is] = [ids(before), ids(after)];
  return was.length === is.length && was.every((id, position) => id === is[position]);
}

// The pending edits that still mean what they meant once the console's layout went from status
// `before` to `after`. Edits are keyed by position: a group level by the group's place, which
// moves when a group is added, deleted or reordered, and a channel's values by input, whose strip
// changes when it joins or leaves a pair or is hidden or shown. Such edits are dropped, so the
// next send never writes a level meant for one group onto another, nor a mono strip's pan onto
// a new pair; the faders show what the engine has. A rename moves nothing and keeps every edit.
function settleLayout(pending, before, after) {
  if (!before || Object.keys(pending).length === 0) {
    return pending;
  }
  const groupsMoved = !sameGroups(before, after);
  const [rolesBefore, rolesAfter] = [inputRoles(before), inputRoles(after)];
  const stale = (key) => {
    const { field, index } = parseEditKey(key);
    if (field === "groupLevels") {
      return groupsMoved;
    }
    return CHANNEL_TABLES.has(field) && rolesBefore[index] !== rolesAfter[index];
  };
  let dropped = false;
  const settled = {};
  for (const [address, edits] of Object.entries(pending)) {
    const kept = Object.entries(edits).filter(([key]) => !stale(key));
    dropped ||= kept.length !== Object.keys(edits).length;
    if (kept.length > 0) {
      settled[address] = Object.fromEntries(kept);
    }
  }
  return dropped ? settled : pending;
}

// True when `incoming` is a newer copy of a musician's mix than `current`.
function isNewer(incoming, current) {
  return incoming.revision > current.revision;
}

function receiveStatus(state, status) {
  if (!isLiveRun(state, status.run)) {
    return state;
  }
  // A status may have been read before a change the console already has: keep the newer mix.
  const musicians = status.musicians.map((row) => {
    const musician = musicianFromDto(row);
    const known = musicianByAddress(state, musician.address);
    return known && !isNewer(musician.mix, known.mix) ? { ...musician, mix: known.mix } : musician;
  });
  const addresses = new Set(musicians.map((musician) => musician.address));
  let phase = status.stopped ? "stopped" : "running";
  if (state.engine.phase === "stopping") {
    phase = "stopping";
  }
  const stays = addresses.has(state.selected);
  // The open musician was on screen and is gone now: the first one opens, and the console says so.
  const leaving = stays ? null : musicianByAddress(state, state.selected);
  let departed = leaving ? { address: leaving.address, label: musicianLabel(leaving) } : state.departed;
  if (departed && addresses.has(departed.address)) {
    departed = null;
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
    selected: stays ? state.selected : (musicians[0]?.address ?? null),
    departed,
    pending: settleLayout(onlyListed(state.pending, addresses), state.engine.status, status),
    mixError: state.mixError && addresses.has(state.mixError.address) ? state.mixError : null,
    controlsNotice:
      state.controlsNotice && addresses.has(state.controlsNotice.address)
        ? state.controlsNotice
        : null,
    // A status proves the console hears the engine.
    connection: READY,
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
      departed: null,
      pending: NO_EDITS,
      mixError: null,
      controlsNotice: null,
      commandError: null,
      audioNotice: null,
    };
  }
  return { ...state, engine: { ...state.engine, phase, error }, commandError: null };
}

// The engine refused a command but its run goes on: back to the phase the last status showed.
function commandFailed(state, { action, message }) {
  const { status } = state.engine;
  let phase = "idle";
  if (status) {
    phase = status.stopped ? "stopped" : "running";
  }
  return {
    ...state,
    engine: { ...state.engine, phase },
    commandError: { action, message },
  };
}

function isListed(state, address) {
  return state.musicians.some((musician) => musician.address === address);
}

// Takes `mix` for `address` when it is newer than the copy the console has; never goes back.
function offerMix(state, address, mix) {
  const known = musicianByAddress(state, address);
  const incoming = mixFromDto(mix);
  if (!known || !isNewer(incoming, known.mix)) {
    return state;
  }
  return {
    ...state,
    musicians: state.musicians.map((musician) =>
      musician.address === address ? { ...musician, mix: incoming } : musician,
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
// anyone else has nothing to settle. The answer's mix is offered like any other copy: taken when
// it is the newest yet (its event has not arrived), ignored when an event already brought a newer
// one. Either way the edits it carried never leave the fader on an older value.
function answered(state, { address, sent, mix }) {
  if (!isListed(state, address)) {
    return state;
  }
  const next = mix ? offerMix(state, address, mix) : state;
  return {
    ...next,
    pending: withPending(next, address, settleEdits(next.pending[address], sent)),
    mixError: next.mixError?.address === address ? null : next.mixError,
  };
}

// An answer to the engineer's controls settles like a `set_mix` answer; one that did not both apply
// and save stays to be said, and a full success clears what was said for that musician.
function engineerAnswered(state, { address, sent, outcome }) {
  if (!isListed(state, address)) {
    return state;
  }
  const next = answered(state, { address, sent, mix: outcome?.mix ?? null });
  const complete = Boolean(outcome?.applied && outcome?.saved);
  let controlsNotice = next.controlsNotice;
  if (!complete) {
    controlsNotice = { address, outcome };
  } else if (controlsNotice?.address === address) {
    controlsNotice = null;
  }
  return { ...next, controlsNotice };
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
      if (!isListed(state, action.address)) {
        return state;
      }
      return state.selected === action.address && state.departed === null
        ? state
        : { ...state, selected: action.address, departed: null };
    case "mix-changed":
      return isLiveRun(state, action.run) ? offerMix(state, action.address, action.mix) : state;
    case "mix-edit":
      return edit(state, action.address, action.edits);
    case "mix-applied":
      return answered(state, action);
    case "mix-failed":
      return refused(state, action);
    case "engineer-applied":
      return engineerAnswered(state, action);
    case "audio":
      return { ...state, audio: audioChoice(state.audio, action.change) };
    case "audio-notice":
      return { ...state, audioNotice: action.notice };
    case "command-failed":
      return commandFailed(state, action);
    case "command-done":
      return state.commandError?.action === action.action ? { ...state, commandError: null } : state;
    case "connection":
      return { ...state, connection: action.connection };
    case "devices":
      return { ...state, devices: action.devices };
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
