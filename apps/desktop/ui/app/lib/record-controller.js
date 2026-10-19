// The Record screen's controller: what the screen (screens/record-screen.js) and the shell's REC
// chip do between the console app (console-app.js) and the DOM, kept free of it so it can be tested
// in node. The screen only reads elements, writes what this works out and forwards its events.
//
// - The transport: one command at a time; the buttons wait until it is answered (`local.busy`).
// - The waveform lanes: while a take records, each armed track keeps a bounded history of its
//   peaks (createPeakHistory), cleared when the next take folder appears.
// - Arming: a press is a delta (armChange), so quick presses on several tracks all land.
// - The folder field: shows the desk's folder unless the engineer is typing; a refused folder stays
//   in it to be corrected, a kept one shows as the desk has it and its takes are listed.

import { recordingStatus } from "./console-state.js";
import { stripLevel } from "./mix-view.js";
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

/**
 * The controller for the console `app` (its `store` and commands). `onChange` is called whenever
 * the controller's own state changed and the screen should render again.
 */
export function createRecordController(app, { onChange = () => {} } = {}) {
  // A command on its way (its buttons wait), and Back to start pressed after a take.
  const local = { busy: null, atStart: false };
  const histories = new Map();
  let take = null;
  let shownFolder = null;

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
    async command(name, run) {
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
    },

    /** Arms or disarms the row `key` (a track or the local mix); null for a row not shown. */
    async arm(key) {
      const state = app.store.getState();
      const row = [...trackRows(state), localTrackRow(state)].find((item) => item?.key === key);
      if (!row) {
        return null;
      }
      const result = await app.setRecordingSettings(armChange(state, row));
      onChange();
      return result;
    },

    /** Asks for the recordings folder `text`; a kept one is listed and shown in the field. */
    async useFolder(text) {
      const result = await app.setRecordingSettings({ folder: text });
      if (result.ok) {
        shownFolder = null;
        await app.refreshTakes();
      }
      onChange();
      return result;
    },

    /** Goes back to the default recordings folder and lists its takes. */
    async useDefaultFolder() {
      shownFolder = null;
      const result = await app.setRecordingSettings({ folder: null });
      await app.refreshTakes();
      onChange();
      return result;
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
 * Shows the REC chip in `strip` (holding an <ml-rec-chip>) while the console `app` reports a take
 * recording, being finished or stopped on an error; hidden otherwise. Returns the unsubscribe.
 */
export function mountRecChip(strip, app) {
  const chip = strip.querySelector("ml-rec-chip");
  function render() {
    const view = recChip(app.store.getState());
    strip.hidden = view === null;
    chip.data = view;
  }
  render();
  return app.store.subscribe(render);
}
