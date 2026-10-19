// The Record screen's controller: what the screen (screens/record-screen.js) and the shell's REC
// chip do between the console app (console-app.js) and the DOM, kept free of it so it can be tested
// in node. The screen only reads elements, writes what this works out and forwards its events.
//
// - The transport: one command at a time; the buttons wait until it is answered (`local.busy`).
//   Moving the take (Back to start, the seek slider) and its loop go through the same guard, so
//   none of them goes out while Stop is on its way; moves do not wait for each other.
// - The waveform lanes: while a take records, each armed track keeps a bounded history of its
//   peaks (createPeakHistory), cleared when the next take folder appears.
// - Arming: a press is a delta (armChange), so quick presses on several tracks all land, and a
//   second press on a track before the first is answered toggles what the first asked.
// - Settings requests never leave a promise rejected: the screen renders again whatever happens and
//   a failure is said as the recording outcome.
// - The virtual soundcheck: Play starts the take the card picks (else the last take) in place of
//   the live inputs, pauses it or plays it on; a take row's Soundcheck starts that take; Stop goes
//   back to the live inputs while a take plays; Back to start and the seek slider move it; the loop
//   applies to the take playing, or is kept for the next start.
// - The folder field: shows the desk's folder unless the engineer is typing; a refused folder stays
//   in it to be corrected, a kept one shows as the desk has it and its takes are listed.

import { actions, isSoundcheckNow, recordingStatus, soundcheckStatus } from "./console-state.js";
import { demoChip } from "./demo-source.js";
import { sentenceWithReason } from "./format.js";
import { stripLevel } from "./mix-view.js";
import { frameAt } from "./soundcheck.js";
import { soundcheckChip, soundcheckPlay } from "./soundcheck-view.js";
import {
  armChange,
  createPeakHistory,
  localTrackRow,
  recChip,
  settingsView,
  trackRows,
} from "./record-view.js";

/** The peaks a lane keeps: about 20 seconds at the 30 meters a second the engine sends. */
export const LANE_CAPACITY = 600;

function reason(error) {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The controller for the console `app` (its `store` and commands). `onChange` is called whenever
 * the controller's own state changed and the screen should render again.
 */
export function createRecordController(app, { onChange = () => {} } = {}) {
  // A command on its way (its buttons wait), Back to start pressed after a take, the take the
  // soundcheck card picks for Play, where the seek slider is being dragged to (seconds), and the
  // loop asked for the next soundcheck.
  const local = { busy: null, atStart: false, chosenTake: null, drag: null, loopNext: false };

  async function command(name, run) {
    if (local.busy !== null) {
      return false;
    }
    local.busy = name;
    onChange();
    try {
      await run();
    } finally {
      local.busy = null;
      onChange();
    }
    return true;
  }

  // A transport command whose answer the caller gets: null when another was on its way (the
  // screen renders again, so a control the engineer changed shows what holds).
  async function answered(name, run) {
    let result = null;
    const ran = await command(name, async () => {
      result = await run();
    });
    if (!ran) {
      onChange();
    }
    return result;
  }

  // Moves of the take on their way: they share the transport's guard ("move"), so none goes out
  // while another command is on its way (a move during Stop would be refused and its words would
  // replace the stop's), but moves do not wait for each other: the console sends the latest.
  let moving = 0;
  async function move(run) {
    if (local.busy !== null && local.busy !== "move") {
      onChange();
      return null;
    }
    moving += 1;
    local.busy = "move";
    onChange();
    try {
      return await run();
    } finally {
      moving -= 1;
      if (moving === 0) {
        local.busy = null;
      }
      onChange();
    }
  }

  // Whether a take plays in place of the inputs now.
  function soundchecking() {
    return isSoundcheckNow(app.store.getState());
  }
  const histories = new Map();
  let take = null;
  let shownFolder = null;
  // Per row key, what the arm press on its way asked for (`{ on }`).
  const intents = new Map();

  // Runs the settings request `run`; whatever happens the screen renders again, and a request
  // that fails is said as the store's recording outcome and answered `{ ok: false, message }`.
  async function settings(run) {
    try {
      return await run();
    } catch (error) {
      const words = sentenceWithReason("The recording settings were not changed", reason(error));
      app.store.dispatch(actions.recordingOutcome("settings", false, words));
      return { ok: false, message: words };
    } finally {
      onChange();
    }
  }

  // Lists the takes; a listing that fails is kept to be said as the takes' error.
  async function listTakes() {
    try {
      await app.refreshTakes();
    } catch (error) {
      app.store.dispatch(actions.takesFailed(reason(error)));
    }
  }

  function historyOf(key) {
    let history = histories.get(key);
    if (!history) {
      history = createPeakHistory(LANE_CAPACITY);
      histories.set(key, history);
    }
    return history;
  }

  /** Whether a take records now: only then do the lanes keep peaks. */
  function taking() {
    return recordingStatus(app.store.getState()).state === "recording";
  }

  return {
    local,
    taking,

    /**
     * Follows the take of `state`: a new take folder starts the lanes empty and the position counts
     * from it. Returns whether a new take was seen.
     */
    followTake(state) {
      const folder = recordingStatus(state).takeFolder;
      if (!folder || folder === take) {
        return false;
      }
      take = folder;
      local.atStart = false;
      for (const history of histories.values()) {
        history.clear();
      }
      return true;
    },

    /**
     * The level of each of `rows` (track rows) from a `meters` event, in order; while a take
     * records, the armed rows keep it in their history.
     */
    takeMeters(rows, { channels, local: localLevels }) {
      const keep = taking();
      return rows.map((row) => {
        const level = row.local
          ? Math.max(localLevels?.left ?? 0, localLevels?.right ?? 0)
          : stripLevel(row.sides, channels ?? []);
        if (keep && row.armed) {
          historyOf(row.key).push(level);
        }
        return level;
      });
    },

    /** The peak history of the row `key`, or undefined when it has kept none. */
    history(key) {
      return histories.get(key);
    },

    /**
     * What the lane of `row` shows: "off" unarmed, "recording" while a take records or once it
     * kept peaks, "armed" otherwise.
     */
    laneOf(row, state) {
      if (!row.armed) {
        return "off";
      }
      const live = ["recording", "stopping"].includes(recordingStatus(state).state);
      return live || (histories.get(row.key)?.length ?? 0) > 0 ? "recording" : "armed";
    },

    /**
     * Runs the transport command `name`: until `run` is answered another is not run (returns
     * false); otherwise returns true once it is. A failure frees the buttons and is thrown.
     */
    command,

    /**
     * Play: starts the take chosen in the soundcheck card (else the last take) as a soundcheck,
     * or pauses the take playing, or plays a paused one on (see soundcheckPlay). Answers the
     * console's `{ ok, message }`, or null when Play cannot be pressed or a command is on its way.
     */
    play() {
      const view = soundcheckPlay(app.store.getState(), local.chosenTake);
      if (!view.can) {
        return Promise.resolve(null);
      }
      const run = {
        start: () => app.startSoundcheck(view.folder, { looping: local.loopNext }),
        pause: () => app.pauseSoundcheck(),
        resume: () => app.resumeSoundcheck(),
      }[view.action];
      return answered("play", run);
    },

    /** A take row's Soundcheck: plays the take in `folder` in place of the live inputs. */
    soundcheckTake(folder) {
      return answered("soundcheck", () =>
        app.startSoundcheck(folder, { looping: local.loopNext }),
      );
    },

    /**
     * Stop: back to the live inputs while a take plays in place of them, else stops the take
     * recording (and reads the disk estimate again).
     */
    stop() {
      if (soundchecking()) {
        return answered("stop", () => app.stopSoundcheck());
      }
      return answered("stop", async () => {
        const result = await app.stopRecording();
        await app.refreshEstimate();
        return result;
      });
    },

    /**
     * Back to start: the take playing in place of the inputs moves to its start; otherwise the
     * position shown goes back from the last take's length to 0:00.
     */
    async rewind() {
      if (soundchecking()) {
        return move(() => app.seekSoundcheck(0));
      }
      local.atStart = true;
      onChange();
      return null;
    },

    /** The take the soundcheck card picks for Play (null: the last take). */
    chooseTake(folder) {
      local.chosenTake = folder || null;
      onChange();
    },

    /** The seek slider is being dragged to `seconds`: the position shows it until it is let go. */
    drag(seconds) {
      local.drag = Number(seconds);
      onChange();
    },

    /**
     * The seek slider let go at `seconds`: the take moves there. Null, and nothing sent, while
     * another transport command is on its way.
     */
    async seekTo(seconds) {
      local.drag = null;
      const { lengthFrames } = soundcheckStatus(app.store.getState());
      return move(() => app.seekSoundcheck(frameAt(seconds, lengthFrames)));
    },

    /**
     * The loop: the take playing starts again at its end (or not), through the transport's guard
     * (null, and nothing sent, while another command is on its way); with the inputs live, the
     * choice is kept for the next start.
     */
    async setLoop(on) {
      if (soundchecking()) {
        return answered("loop", () => app.setSoundcheckLoop(on));
      }
      local.loopNext = Boolean(on);
      onChange();
      return null;
    },

    /**
     * Arms or disarms the row `key` (a track or the local mix); null for a row not shown. A press
     * made while an earlier one on the same row is on its way toggles what that one asked, so a
     * double press ends where it began.
     */
    async arm(key) {
      const state = app.store.getState();
      const row = [...trackRows(state), localTrackRow(state)].find((item) => item?.key === key);
      if (!row) {
        return null;
      }
      const armedNow = intents.has(key) ? intents.get(key).on : row.armed;
      const intent = { on: !armedNow };
      intents.set(key, intent);
      try {
        return await settings(() => app.setRecordingSettings(armChange(state, row, armedNow)));
      } finally {
        if (intents.get(key) === intent) {
          intents.delete(key);
        }
      }
    },

    /** Asks for the settings `change` (the format, the local mix) and renders again. */
    changeSettings(change) {
      return settings(() => app.setRecordingSettings(change));
    },

    /** Asks for the recordings folder `text`; a kept one is listed and shown in the field. */
    useFolder(text) {
      return settings(async () => {
        const result = await app.setRecordingSettings({ folder: text });
        if (result.ok) {
          shownFolder = null;
          await listTakes();
        }
        return result;
      });
    },

    /** Goes back to the default recordings folder and lists its takes. */
    useDefaultFolder() {
      shownFolder = null;
      return settings(async () => {
        const result = await app.setRecordingSettings({ folder: null });
        await listTakes();
        return result;
      });
    },

    /**
     * The folder to write into the folder field for `state`, or null to leave the field as it is:
     * already shown, or the engineer is `typing` in it.
     */
    folderToShow(state, typing) {
      const { folder } = settingsView(state);
      if (typing || folder === shownFolder) {
        return null;
      }
      shownFolder = folder;
      return folder;
    },
  };
}

/**
 * Shows the shell's status strip while it has a chip to show: the REC chip (`[data-chip="rec"]`,
 * or the strip's first <ml-rec-chip>) while the console `app` reports a take recording, being
 * finished or stopped on an error, and the SOUNDCHECK chip (`[data-chip="soundcheck"]`, when the
 * strip has one) while a take plays in place of the inputs, and the demo badge
 * (`[data-chip="demo"]`, when the strip has one) while the engine plays the demo source; hidden
 * otherwise. Returns the unsubscribe.
 */
export function mountRecChip(strip, app) {
  const rec = strip.querySelector('[data-chip="rec"]') ?? strip.querySelector("ml-rec-chip");
  const check = strip.querySelector('[data-chip="soundcheck"]');
  const demo = strip.querySelector('[data-chip="demo"]');
  function render() {
    const state = app.store.getState();
    const recView = recChip(state);
    const checkView = check ? soundcheckChip(state) : null;
    const demoView = demo ? demoChip(state) : null;
    strip.hidden = recView === null && checkView === null && demoView === null;
    rec.data = recView;
    if (check) {
      check.data = checkView;
    }
    if (demo) {
      demo.data = demoView;
    }
  }
  render();
  return app.store.subscribe(render);
}
