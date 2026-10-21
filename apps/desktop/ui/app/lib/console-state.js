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
//   presets    the engineer's mix presets as the desk keeps them (see mixes.js), or null until
//              they were read. They live in the desk's session, so a run's end keeps them.
//   sessions   { list, error }: the engineer's saved sessions (see sessions.js), null until they
//              were listed, and why the last listing failed. A run's end keeps them.
//   sessionNotice what the desk said when it opened its session (a damaged part set aside, a newer
//              session left alone, labels imported), or null.
//   mixBackups { list, error }: the backups of the musicians' mixes (see mix-backups-view.js),
//              the newest first, null until they were listed, and why the last listing failed. A
//              run's end keeps them.
//   localMix   the engineer's local mix (the status's `localMix`, see local-mix.js), or null while
//              no engine runs. It is opened, edited and answered like a musician's mix under the
//              id LOCAL_MIX, which no musician's address can be: `selected`, `pending` and
//              `mixError` take it as they take an address. It has no level cap, limit or locks, so
//              edits of those are never kept for it.
//   outputs    { list, error }: the output devices the running engine offers for the local mix
//              (`output_devices`), null until listed; a run's end forgets them, and a listing
//              answered for a run other than the one shown is not taken.
//   recordingSettings how the desk records takes (the `recording_settings` answer, see
//              recording.js), or null until read. They live in the desk's session, so a run's end
//              keeps them. Whether a take records is the status's `recording` (recordingStatus).
//   takes      { list, folder, unreadable, missing, error }: the takes in the recordings folder,
//              the newest first (`list_takes`), null until listed, and why the last listing
//              failed. A run's end keeps them.
//   estimate   what a take with the kept settings would take on the disk (`record_estimate`), or
//              null; a run's end forgets it, since its rate depends on the running device.
//   recordingOutcome { action, ok, message }: the last recording or soundcheck command's outcome
//              in words ("settings", "start", "stop", "marker", "open", "soundcheck"), or null.
//              Whether a take plays in place of the inputs is the status's `soundcheck`
//              (soundcheckStatus), which a soundcheck command's answer updates at once.
//   processing every channel's strip, `[{ channel, settings, active }]` CH 1 first (see
//              processing.js), as the desk answered it, or null until read. It lives in the desk's
//              session, so a run's end keeps it; a loaded session replaces it.
//   processingPending { channel: settings }: a strip edit still waiting for the desk, which the
//              screen shows over the kept one (channelStrip) until its answer settles it.
//   processingError { channel, message } of the last refused strip edit, cleared by the next answer.
//   stripPresets every strip preset, the built-in ones first (`strip_presets`), or null until read.
//   eqCurves  { channel: [{ freqHz, db }] }: the equaliser response last worked out for a channel.
//
// Loading a saved session replaces the mixes, the groups and the channel details at once, so the
// edits still waiting are dropped once it loaded (`sessionLoaded`): none of them was meant for the
// loaded session. A refused load keeps them.
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
import { DEFAULT_AUDIO, audioChoice, restoredAudio } from "./engine-config.js";
import { musicianLabel, needsAttention } from "./format.js";
import { STRIP_CHANNELS, defaultStrip, isNeutralStrip } from "./processing.js";
import { isRecording, recordingOf } from "./recording.js";
import { isSoundchecking, soundcheckOf } from "./soundcheck.js";
import {
  ENGINEER_FIELDS,
  MIX_FIELDS,
  applyEdits,
  mixFromDto,
  parseEditKey,
  settleEdits,
} from "./mix.js";

export const PHASES = Object.freeze(["idle", "starting", "running", "stopping", "stopped", "error"]);

/**
 * The id of the engineer's local mix (the desktop's `LOCAL_MIX_ID`): its tab, its pending edits
 * and its `mix-changed` event. A musician's address always has a port, so it is never this.
 */
export const LOCAL_MIX = "local";

/** The fields the local mix has no use for: a phone's level cap and the engineer's controls. */
const NOT_LOCAL = new Set(["maxLevelPercent", ...ENGINEER_FIELDS]);

const NO_OUTPUTS = Object.freeze({ list: null, error: null });

const NO_TAKES = Object.freeze({
  list: null,
  folder: null,
  unreadable: 0,
  missing: false,
  error: null,
});

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
  presets: null,
  sessions: Object.freeze({ list: null, error: null }),
  sessionNotice: null,
  mixBackups: Object.freeze({ list: null, error: null }),
  localMix: null,
  outputs: NO_OUTPUTS,
  recordingSettings: null,
  takes: NO_TAKES,
  estimate: null,
  recordingOutcome: null,
  processing: null,
  processingPending: NO_EDITS,
  processingError: null,
  stripPresets: null,
  eqCurves: NO_EDITS,
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
  /**
   * A `mix-changed` payload: `{ run, address, origin, mix }` for a musician's mix, or `{ run, id,
   * origin, mix }` with the id LOCAL_MIX for the engineer's local mix.
   */
  mixChanged: ({ run, address, id, origin, mix }) => ({
    type: "mix-changed",
    run,
    address: id === LOCAL_MIX ? LOCAL_MIX : address,
    origin,
    mix,
  }),
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
   * The output devices the engine of run `run` offers for the local mix (`output_devices`): taken
   * only while that run is the one shown, since another run's driver may offer others.
   */
  outputs: (list, run) => ({ type: "outputs", run, outputs: { list, error: null } }),
  /**
   * The output devices of run `run` could not be listed, for `message`; the last list known stays.
   * Taken only while that run is the one shown, as `outputs`.
   */
  outputsFailed: (message, run) => ({ type: "outputs-failed", run, message }),
  /** The engineer's mix presets as the desk answered them. */
  presets: (presets) => ({ type: "presets", presets }),
  /** The engineer's saved sessions as the desk listed them. */
  sessions: (list) => ({ type: "sessions", sessions: { list, error: null } }),
  /** The saved sessions could not be listed, for `message`; the last list known stays. */
  sessionsFailed: (message) => ({ type: "sessions-failed", message }),
  /** The backups of the musicians' mixes as the desk listed them (see mix-backups-view.js). */
  mixBackups: (list) => ({ type: "mix-backups", mixBackups: { list, error: null } }),
  /** The mix backups could not be listed, for `message`; the last list known stays. */
  mixBackupsFailed: (message) => ({ type: "mix-backups-failed", message }),
  /** What the desk said when it opened its session, or null. */
  sessionNotice: (notice) => ({ type: "session-notice", notice }),
  /**
   * A saved session was loaded (the `load_session` answer): the edits still waiting are dropped,
   * the presets are the loaded ones, and its audio input, when it was taken, is what the next start
   * opens.
   */
  sessionLoaded: (outcome) => ({ type: "session-loaded", outcome }),
  /**
   * The engine refused `action` ("stop" or "switch") but its run goes on: the phase goes back to
   * what the last status said, and the refusal is kept to be shown.
   */
  commandFailed: (action, message) => ({ type: "command-failed", action, message }),
  /** `action` worked: an earlier refusal of the same command no longer holds. */
  commandDone: (action) => ({ type: "command-done", action }),
  /** How the desk records takes, as `recording_settings` or `set_recording_settings` answered. */
  recordingSettings: (settings) => ({ type: "recording-settings", settings }),
  /** The `list_takes` answer: `{ folder, takes, unreadable, missing }`. */
  takes: ({ folder, takes, unreadable, missing }) => ({
    type: "takes",
    takes: {
      list: takes,
      folder,
      unreadable: unreadable ?? 0,
      missing: Boolean(missing),
      error: null,
    },
  }),
  /** The takes could not be listed, for `message`; the last list known stays. */
  takesFailed: (message) => ({ type: "takes-failed", message }),
  /** The `record_estimate` answer, or null to forget it. */
  estimate: (estimate) => ({ type: "estimate", estimate }),
  /**
   * The outcome of a recording command in words: `action` is "settings", "start", "stop",
   * "marker" or "open". Null clears it.
   */
  recordingOutcome: (action, ok, message) => ({
    type: "recording-outcome",
    outcome: action === null ? null : { action, ok, message },
  }),
  /**
   * A soundcheck command's answer (the soundcheck now, see soundcheck.js) for engine run `run`:
   * shown at once on that run's status, ahead of the next status.
   */
  soundcheck: (soundcheck, run) => ({ type: "soundcheck", soundcheck, run }),
  /** Every channel's strip as `processing` answered it, `[{ channel, settings, active }]`. */
  processing: (list) => ({ type: "processing", list }),
  /** A strip edit of `channel` (1-based) on its way to the desk: the whole strip, `settings`. */
  processingEdit: (channel, settings) => ({ type: "processing-edit", channel, settings }),
  /**
   * The desk answered a processing change (an edit of `channel` that carried `sent`, or a reset,
   * copy or preset with `sent` null) with `answer`, `{ outcome, channels, message }`: every channel
   * it names takes its strip, and the edit it carried is settled.
   */
  processingApplied: (channel, sent, answer) => ({
    type: "processing-applied",
    channel,
    sent,
    answer,
  }),
  /** A processing change of `channel` that carried `sent` was refused, for `message`. */
  processingFailed: (channel, sent, message) => ({
    type: "processing-failed",
    channel,
    sent,
    message,
  }),
  /** Every strip preset as the desk listed it, the built-in ones first. */
  stripPresets: (list) => ({ type: "strip-presets", list }),
  /** The equaliser response `points` (`[{ freqHz, db }]`) worked out for `channel`. */
  eqCurve: (channel, points) => ({ type: "eq-curve", channel, points }),
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

/**
 * True when `run` is the engine run whose status the console shows now, and live: an answer about
 * that engine (its outputs) applies; one about an ended or a newer, not yet shown, run does not.
 */
function isShownRun(state, run) {
  return Number.isInteger(run) && run === state.engine.run && isLiveRun(state, run);
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

// The newer of the local mix a status carries and the copy the console has; null for a status
// without one (a desk from before the local mix).
function receivedLocalMix(state, status) {
  if (!status.localMix) {
    return null;
  }
  const incoming = mixFromDto(status.localMix);
  return state.localMix && !isNewer(incoming, state.localMix) ? state.localMix : incoming;
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
  const localMix = receivedLocalMix(state, status);
  // Every mix the console can open: the musicians' addresses, and the local mix's id.
  const addresses = new Set(musicians.map((musician) => musician.address));
  if (localMix) {
    addresses.add(LOCAL_MIX);
  }
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
    localMix,
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
      localMix: null,
      outputs: NO_OUTPUTS,
      estimate: null,
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

// Whether the console has the mix `address` names: a listed musician's, or the local mix.
function isListed(state, address) {
  return mixOf(state, address) !== null;
}

// Takes `mix` for `address` (a musician's address, or LOCAL_MIX) when it is newer than the copy
// the console has; never goes back.
function offerMix(state, address, mix) {
  const known = mixOf(state, address);
  const incoming = mixFromDto(mix);
  if (!known || !isNewer(incoming, known)) {
    return state;
  }
  if (address === LOCAL_MIX) {
    return { ...state, localMix: incoming };
  }
  return {
    ...state,
    musicians: state.musicians.map((musician) =>
      musician.address === address ? { ...musician, mix: incoming } : musician,
    ),
  };
}

// The edits a mix keeps: all of them for a musician; for the local mix none of a level cap, a
// limit or a lock, which it has no use for.
function keptEdits(address, edits) {
  if (address !== LOCAL_MIX) {
    return edits;
  }
  return Object.fromEntries(
    Object.entries(edits).filter(([key]) => !NOT_LOCAL.has(parseEditKey(key).field)),
  );
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
  Object.keys(edits).forEach(parseEditKey);
  const kept = keptEdits(address, edits);
  if (Object.keys(kept).length === 0 || !isListed(state, address)) {
    return state;
  }
  return {
    ...state,
    pending: withPending(state, address, { ...(state.pending[address] ?? NO_EDITS), ...kept }),
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

// Every edit still waiting goes, and so does what was said about the last refused or half-done one.
function dropEdits(state) {
  if (state.pending === NO_EDITS && !state.mixError && !state.controlsNotice) {
    return state;
  }
  return { ...state, pending: NO_EDITS, mixError: null, controlsNotice: null };
}

function sessionLoaded(state, outcome) {
  const next = dropEdits(state);
  return {
    ...next,
    processingPending: NO_EDITS,
    processingError: null,
    presets: outcome.presets ?? next.presets,
    audio: outcome.audioLoaded ? restoredAudio(outcome.audio) : next.audio,
  };
}

// The strips as the console knows them, every channel listed: the ones read, or neutral ones until
// the first read, so an answer that came first still lands.
function knownStrips(state) {
  if (state.processing) {
    return state.processing;
  }
  return Array.from({ length: STRIP_CHANNELS }, (_, index) => ({
    channel: index + 1,
    settings: defaultStrip(),
    active: false,
  }));
}

// `pending` without the edit of `channel`, when the edit still waiting is the one `sent` (a later
// one stays: it goes out next).
function settleStrip(pending, channel, sent) {
  if (sent === null || pending[channel] !== sent) {
    return pending;
  }
  const { [channel]: _settled, ...rest } = pending;
  return Object.keys(rest).length === 0 ? NO_EDITS : rest;
}

function processingApplied(state, { channel, sent, answer }) {
  const changed = new Map((answer?.channels ?? []).map((strip) => [strip.channel, strip]));
  const processing =
    changed.size === 0
      ? state.processing
      : knownStrips(state).map((strip) => changed.get(strip.channel) ?? strip);
  return {
    ...state,
    processing,
    processingPending: settleStrip(state.processingPending, channel, sent),
    processingError: null,
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
    case "outputs":
      return isShownRun(state, action.run) ? { ...state, outputs: action.outputs } : state;
    case "outputs-failed":
      return isShownRun(state, action.run)
        ? { ...state, outputs: { ...state.outputs, error: action.message } }
        : state;
    case "presets":
      return { ...state, presets: action.presets };
    case "processing":
      return { ...state, processing: action.list ?? null };
    case "processing-edit":
      return {
        ...state,
        processingPending: { ...state.processingPending, [action.channel]: action.settings },
      };
    case "processing-applied":
      return processingApplied(state, action);
    case "processing-failed":
      return {
        ...state,
        processingPending: settleStrip(state.processingPending, action.channel, action.sent),
        processingError: { channel: action.channel, message: action.message },
      };
    case "strip-presets":
      return { ...state, stripPresets: action.list ?? null };
    case "eq-curve":
      return { ...state, eqCurves: { ...state.eqCurves, [action.channel]: action.points } };
    case "sessions":
      return { ...state, sessions: action.sessions };
    case "sessions-failed":
      return { ...state, sessions: { ...state.sessions, error: action.message } };
    case "mix-backups":
      return { ...state, mixBackups: action.mixBackups };
    case "mix-backups-failed":
      return { ...state, mixBackups: { ...state.mixBackups, error: action.message } };
    case "session-notice":
      return { ...state, sessionNotice: action.notice ?? null };
    case "session-loaded":
      return sessionLoaded(state, action.outcome);
    case "recording-settings":
      return { ...state, recordingSettings: action.settings ?? null };
    case "takes":
      return { ...state, takes: action.takes };
    case "takes-failed":
      return { ...state, takes: { ...state.takes, error: action.message } };
    case "estimate":
      return { ...state, estimate: action.estimate ?? null };
    case "recording-outcome":
      return { ...state, recordingOutcome: action.outcome };
    case "soundcheck":
      return state.engine.status && action.soundcheck && isShownRun(state, action.run)
        ? {
            ...state,
            engine: { ...state.engine, status: { ...state.engine.status, soundcheck: action.soundcheck } },
          }
        : state;
    default:
      return state;
  }
}

/**
 * The strip a screen shows for `channel` (1-based): the edit still waiting, else the desk's, else a
 * neutral one while the strips are not read yet. `{ channel, settings, active, pending }`.
 */
export function channelStrip(state, channel) {
  const waiting = state.processingPending[channel];
  if (waiting) {
    return { channel, settings: waiting, active: !isNeutralStrip(waiting), pending: true };
  }
  const kept = state.processing?.find((strip) => strip.channel === channel);
  return kept
    ? { ...kept, pending: false }
    : { channel, settings: defaultStrip(), active: false, pending: false };
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

/**
 * The engine's mix `address` names: the musician's at that address, or the local mix for
 * LOCAL_MIX; null when the console has none.
 */
export function mixOf(state, address) {
  if (address === LOCAL_MIX) {
    return state.localMix ?? null;
  }
  return musicianByAddress(state, address)?.mix ?? null;
}

/**
 * The mix a screen shows for `address` (a musician's address, or LOCAL_MIX): the engine's mix
 * with the pending edits on top.
 */
export function viewMix(state, address) {
  const mix = mixOf(state, address);
  if (!mix) {
    return null;
  }
  const edits = state.pending[address];
  return edits ? applyEdits(mix, edits) : mix;
}

/**
 * The take being recorded as the shown status reports it (see recording.js), or the idle recorder
 * while no engine runs.
 */
export function recordingStatus(state) {
  return recordingOf(state.engine.status);
}

/** True while a take records or its files are being finished: the input device cannot change. */
export function isRecordingNow(state) {
  return isRecording(recordingStatus(state));
}

/**
 * The virtual soundcheck as the shown status reports it (see soundcheck.js), or the idle one, the
 * inputs live, while no engine runs.
 */
export function soundcheckStatus(state) {
  return soundcheckOf(state.engine.status);
}

/**
 * True while a take plays in place of the inputs (playing, paused or failed): nothing records and
 * the input device cannot change.
 */
export function isSoundcheckNow(state) {
  return isSoundchecking(soundcheckStatus(state));
}

/** The last take that ended while the shown engine ran, or null. */
export function lastTake(state) {
  return state.engine.status?.lastTake ?? null;
}

/** The musicians whose link is a warning or bad, in the status's order. */
export function musiciansNeedingAttention(state) {
  return state.musicians.filter(needsAttention);
}
