// Named sessions: what the Session panel (Network and system) offers, how it words the desk's
// answers, the panel's behaviour apart from the DOM, and the name rule the desk keeps (the fake
// bridge keeps it too). No DOM.
//
// A saved session is listed as `{ id, name, savedAt, mixes, problem }`: its name, when it was saved
// (unix milliseconds, or null), how many musicians' mixes it keeps, and why it cannot be loaded
// (null when it can). Saving keeps the whole desk session (labels, the engineer's limits, locks
// and mirrors, channel details, groups, presets and the audio input) and, while an engine runs,
// every musician's mix. Loading replaces the desk session with it and pushes each saved mix to the
// musician it was saved for when they are here; a phone that is not connected keeps its own mix,
// which wins when it connects.

import { musicianLabel, sentenceWithReason } from "./format.js";
import { joinNames } from "./mixes.js";
import { ARMED_FOR_MS } from "./mix-tools.js";
import { confirmReady, CONFIRM_DELAY_MS } from "./network-view.js";

/** The longest name of a saved session, in characters. */
export const MAX_SESSION_NAME_LENGTH = 40;

/**
 * `name` trimmed, when it can name a saved session among `sessions` (the one with the id `renaming`
 * aside), else an Error, as the desk refuses it: no name, more than MAX_SESSION_NAME_LENGTH
 * characters, or another saved session's name in any case.
 */
export function checkSessionName(name, sessions, renaming = null) {
  const trimmed = String(name ?? "").trim();
  if (!trimmed) {
    throw new Error("a saved session needs a name");
  }
  if ([...trimmed].length > MAX_SESSION_NAME_LENGTH) {
    throw new Error(`a session name is at most ${MAX_SESSION_NAME_LENGTH} characters`);
  }
  const lower = trimmed.toLowerCase();
  if (sessions.some((saved) => saved.id !== renaming && saved.name.toLowerCase() === lower)) {
    throw new Error(`a session named "${trimmed}" is already saved`);
  }
  return trimmed;
}

const MONTHS = Object.freeze([
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
]);

/** "16 Oct 2026, 19:05" in local time for unix milliseconds `at`, or null when there are none. */
export function formatSavedAt(at) {
  if (!Number.isFinite(at)) {
    return null;
  }
  const date = new Date(at);
  const two = (value) => String(value).padStart(2, "0");
  return (
    `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()}, ` +
    `${two(date.getHours())}:${two(date.getMinutes())}`
  );
}

/** "no mixes", "1 musician's mix", "3 musicians' mixes". */
export function mixesText(count) {
  if (!count) {
    return "no mixes";
  }
  return count === 1 ? "1 musician's mix" : `${count} musicians' mixes`;
}

/**
 * The saved sessions as the panel lists them: `{ id, name, label, detail, loadable }`, `label`
 * being the option's text and `detail` when it was saved and how many mixes it keeps, or why it
 * cannot be loaded.
 */
export function sessionRows(sessions) {
  return (sessions ?? []).map((saved) => {
    const loadable = !saved.problem;
    const when = formatSavedAt(saved.savedAt);
    return {
      id: saved.id,
      name: saved.name,
      loadable,
      label: loadable ? saved.name : `${saved.name} (cannot be loaded)`,
      detail: loadable
        ? [when ? `Saved ${when}` : null, mixesText(saved.mixes)].filter(Boolean).join(" · ")
        : sentenceWithReason("It cannot be loaded", saved.problem),
    };
  });
}

/**
 * What the Session panel shows for the console's `state`: `{ notice, rows, unread, engineRunning }`.
 * `notice` is what the desk said when it opened its session (a damaged part set aside, a newer
 * session left alone, labels imported), or null; `unread` why the saved sessions could not be
 * listed, or null once they were.
 */
export function sessionPanelView(state) {
  const { list, error } = state.sessions;
  return {
    notice: state.sessionNotice
      ? sentenceWithReason("When the console opened its saved session", state.sessionNotice)
      : null,
    rows: sessionRows(list),
    unread: list === null ? (error ?? "") : null,
    engineRunning: state.engine.status !== null,
  };
}

/** What a save did, from the `save_session` answer `{ session, engineRunning }`. */
export function formatSessionSaved({ session, engineRunning }) {
  const saved = `Saved the session as "${session.name}"`;
  if (!engineRunning) {
    return `${saved} without mixes: the engine is not running.`;
  }
  if (!session.mixes) {
    return `${saved}. No musician was listed, so no mix was kept.`;
  }
  return `${saved}, with ${mixesText(session.mixes)}.`;
}

/**
 * What a load did, from the `load_session` answer, in sentences: what applies (now, or at the next
 * start), whether it was saved, the mixes pushed and to whom, the saved mixes that went to nobody
 * and why, what the engine refused, what of the saved session could not be read, and the audio
 * input. `labelAt(address)` names a musician a mix was pushed to, `nameOf(skipped)` the one a
 * skipped mix was for (see skippedName).
 */
export function formatSessionLoaded(outcome, { labelAt, nameOf }) {
  const name = outcome.session?.name ?? "the session";
  const sentences = [
    outcome.applied
      ? `Loaded "${name}": its channels, groups, presets, labels, limits, locks and mirrors apply now.`
      : `Loaded "${name}": it applies at the next start.`,
  ];
  if (!outcome.saved) {
    sentences.push(sentenceWithReason("It runs, but was not saved", outcome.saveError));
  }
  const full = outcome.mixes.filter((mix) => !mix.follows);
  if (full.length > 0) {
    const names = joinNames(full.map((mix) => labelAt(mix.address)));
    sentences.push(`Pushed the saved ${full.length === 1 ? "mix" : "mixes"} to ${names}.`);
  }
  for (const mix of outcome.mixes.filter((pushed) => pushed.follows)) {
    sentences.push(
      `${labelAt(mix.address)} follows ${mix.follows}'s mix, so only their master volume and ` +
        "mute were set.",
    );
  }
  const skipped = (reason) => outcome.skipped.filter((mix) => mix.reason === reason);
  const notRunning = skipped("notRunning");
  if (notRunning.length > 0) {
    sentences.push(
      notRunning.length === 1
        ? "The saved mix was not pushed: the engine is not running."
        : `The ${notRunning.length} saved mixes were not pushed: the engine is not running.`,
    );
  }
  const away = skipped("notConnected").map(nameOf);
  if (away.length > 0) {
    sentences.push(
      `Not pushed to ${joinNames(away)}: not connected, and a phone's own mix wins when it connects.`,
    );
  }
  const gone = skipped("notHere").map(nameOf);
  if (gone.length > 0) {
    sentences.push(`Not pushed to ${joinNames(gone)}: not here.`);
  }
  for (const mix of skipped("refused")) {
    sentences.push(sentenceWithReason(`Not pushed to ${nameOf(mix)}`, mix.error));
  }
  if (outcome.refused.length > 0) {
    sentences.push(sentenceWithReason("Not applied", outcome.refused.join("; ")));
  }
  if (outcome.damaged.length > 0) {
    sentences.push(
      sentenceWithReason(
        "Part of the saved session could not be read and was left out",
        outcome.damaged.join("; "),
      ),
    );
  }
  if (outcome.audioLoaded) {
    sentences.push("Its audio input is used from the next start.");
  } else if (outcome.audioSkipped) {
    sentences.push(
      `${sentenceWithReason("Its audio input was not taken", outcome.audioSkipped)} ` +
        "The desk keeps its own.",
    );
  }
  return sentences.join(" ");
}

/**
 * The name of the musician a skipped saved mix was for (`{ key, label }`): the label the loaded
 * session keeps, else the musician `musicians` (status rows) lists under that key, else their
 * address, else "a phone".
 */
export function skippedName({ key, label }, musicians) {
  if (label) {
    return label;
  }
  const [kind, value] = [key.slice(0, key.indexOf(":")), key.slice(key.indexOf(":") + 1)];
  const row = musicians.find((musician) =>
    kind === "client" ? musician.clientId === value : musician.address === value,
  );
  if (row) {
    return musicianLabel(row);
  }
  return kind === "address" ? value : "a phone";
}

/**
 * The Session panel's behaviour: which saved session is chosen, the load that waits for the
 * engineer's word while edits are still on their way, and the guarded delete with its timers.
 * `emit(type, detail)` sends the element's events; `changed()` asks it to render again (a delete
 * that became confirmable or disarmed itself); `now`, `setTimer` and `clearTimer` are the clock and
 * timers, stand-ins in the tests.
 */
export function createSessionPanel({
  emit,
  changed = () => {},
  now = () => performance.now(),
  setTimer = setTimeout,
  clearTimer = clearTimeout,
}) {
  let rows = [];
  let chosen = null;
  let armed = null;
  let timers = [];
  let asking = null;

  function disarm() {
    armed = null;
    for (const timer of timers.splice(0)) {
      clearTimer(timer);
    }
  }

  function row(id = chosen) {
    return rows.find((candidate) => candidate.id === id) ?? null;
  }

  return {
    /** Takes the panel's rows (sessionRows). The choice stays on its session while it is listed. */
    setRows(next) {
      rows = next;
      if (!row()) {
        chosen = rows[0]?.id ?? null;
      }
      if (armed && !row(armed.id)) {
        disarm();
      }
      if (asking && !row(asking.id)) {
        asking = null;
      }
    },

    /** The saved session chosen, or null when none is listed. */
    chosen: () => chosen,

    /** The engineer chose the saved session `id`: anything asked about another one is dropped. */
    choose(id) {
      if (id === chosen || !row(id)) {
        return;
      }
      chosen = id;
      asking = null;
      disarm();
    },

    saveReady: (name) => String(name ?? "").trim() !== "",

    save(name) {
      if (this.saveReady(name)) {
        emit("session-save", { name: String(name).trim() });
      }
    },

    loadReady: () => Boolean(row()?.loadable),

    /**
     * Load pressed with `unsaved` edits still on their way (mix changes or channel and group edits
     * not answered yet): with none the chosen session is loaded at once; otherwise the panel asks
     * first, since loading drops them.
     */
    load(unsaved = 0) {
      if (!this.loadReady()) {
        return;
      }
      if (unsaved > 0) {
        asking = { id: chosen, unsaved };
        return;
      }
      asking = null;
      emit("session-load", { id: chosen });
    },

    /** What the panel asks before a load, `{ id, name, unsaved }`, or null. */
    asking: () => (asking ? { ...asking, name: row(asking.id)?.name ?? "" } : null),

    /** "Load anyway": the session asked about is loaded and the edits on their way dropped. */
    confirmLoad() {
      if (asking) {
        const { id } = asking;
        asking = null;
        emit("session-load", { id });
      }
    },

    /** "Cancel": nothing is loaded. */
    cancelLoad() {
      asking = null;
    },

    renameReady: (name) => Boolean(row()?.loadable) && String(name ?? "").trim() !== "",

    rename(name) {
      if (this.renameReady(name)) {
        emit("session-rename", { id: chosen, name: String(name).trim() });
      }
    },

    /** Delete pressed for the chosen session: it asks first, and disarms itself after a while. */
    arm() {
      if (!chosen) {
        return;
      }
      disarm();
      armed = { id: chosen, at: now() };
      timers.push(
        setTimer(changed, CONFIRM_DELAY_MS),
        setTimer(() => {
          disarm();
          changed();
        }, ARMED_FOR_MS),
      );
    },

    /** Keep pressed (or Escape): the session stays. */
    keep: disarm,

    /** The confirming Delete: works only once armed long enough. */
    confirmDelete() {
      if (!armed || armed.id !== chosen || !confirmReady(armed.at, now())) {
        return false;
      }
      const { id } = armed;
      disarm();
      emit("session-delete", { id });
      return true;
    },

    /** How the delete shows: `{ armed, ready }`. */
    deleteState() {
      const here = Boolean(armed) && armed.id === chosen;
      return { armed: here, ready: here && confirmReady(armed.at, now()) };
    },

    /**
     * The screen's command for `kind` ("save", "load", "rename", "delete") was answered, `ok` or
     * not: `{ clearSave, clearRename }` say which fields to empty.
     */
    answered(kind, ok) {
      if (kind === "load") {
        asking = null;
      }
      return { clearSave: ok && kind === "save", clearRename: ok && kind === "rename" };
    },

    /** Stops the timers, when the element leaves the page. */
    dispose: disarm,
  };
}

/**
 * The Network and system screen's side of the Session panel: `handle(type, detail)` runs the
 * console `app`'s command for one of <ml-session-panel>'s events (`types`), then hands its answer
 * `{ ok, message }` to `said(outcome)` and to `panel.answered(kind, ok)`, whatever happened, so a
 * load asked about never stays asked.
 */
export function sessionPanelCommands({ app, panel, said }) {
  const commands = {
    "session-save": ["save", (detail) => app.saveSession(detail.name)],
    "session-load": ["load", (detail) => app.loadSession(detail.id)],
    "session-rename": ["rename", (detail) => app.renameSession(detail.id, detail.name)],
    "session-delete": ["delete", (detail) => app.deleteSession(detail.id)],
    "sessions-load": ["list", () => app.loadSessions().then(() => null)],
  };
  return {
    types: Object.keys(commands),
    async handle(type, detail) {
      const [kind, run] = commands[type];
      let outcome = null;
      try {
        outcome = await run(detail ?? {});
      } finally {
        if (outcome) {
          said(outcome);
        }
        panel.answered(kind, outcome?.ok === true);
      }
      return outcome;
    },
  };
}
