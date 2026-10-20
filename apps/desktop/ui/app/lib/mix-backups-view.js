// Mix backups: what the Session panel (Network and system) shows of the backups of the musicians'
// mixes, how it words a restore, and its restore-with-a-confirm behaviour apart from the DOM. No
// DOM.
//
// The desk backs every musician's live mix up a few seconds after the mixes go still (at the
// latest BACKUP_MAX_WAIT_MS after the first change while they keep changing), and when the engine
// stops or the app quits; it keeps the newest KEPT_BACKUPS. A backup is listed as `{ stamp,
// savedAt, musicians, names, problem }`: its stamp (what a restore names it by), when it was taken
// (unix milliseconds, or null for one that cannot be read), how many musicians' mixes it keeps and
// what the desk called them, and why it cannot be restored (null when it can). Restoring sends each
// backed-up mix to the musician it was taken from when their phone is connected now; one that is
// not is skipped and said, never queued: when a phone connects, its own mix wins.

import { sentenceWithReason } from "./format.js";
import { joinNames } from "./mixes.js";

/** How many backups the desk keeps. */
export const KEPT_BACKUPS = 10;
/** How long the mixes stay still before a backup is taken. */
export const BACKUP_QUIET_MS = 3000;
/** The longest a change waits for its backup while the mixes keep changing. */
export const BACKUP_MAX_WAIT_MS = 20000;

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

/** "18 Oct 2026, 19:04:05" in local time for unix milliseconds `at`, or null when there are none. */
export function formatBackupTime(at) {
  if (!Number.isFinite(at)) {
    return null;
  }
  const date = new Date(at);
  const two = (value) => String(value).padStart(2, "0");
  return (
    `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()}, ` +
    `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`
  );
}

/** "no musicians", "1 musician: Ana", "3 musicians: Ana, Bass and Rack". */
function musiciansText(count, names) {
  if (!count) {
    return "no musicians";
  }
  const lead = count === 1 ? "1 musician" : `${count} musicians`;
  return names?.length ? `${lead}: ${joinNames(names)}` : lead;
}

/**
 * The backups as the panel lists them: `{ stamp, restorable, label, detail }`, `label` being when
 * it was taken and `detail` whose mixes it keeps, or why it cannot be restored.
 */
export function mixBackupRows(list) {
  return (list ?? []).map((kept) => {
    const restorable = !kept.problem;
    return {
      stamp: kept.stamp,
      restorable,
      label: (restorable && formatBackupTime(kept.savedAt)) || `Backup ${kept.stamp}`,
      detail: restorable
        ? musiciansText(kept.musicians, kept.names)
        : sentenceWithReason("It cannot be restored", kept.problem),
    };
  });
}

/**
 * What the panel's Mix backups part shows for the console's `state`: `{ rows, unread,
 * engineRunning }`; `unread` is why the backups could not be listed ("" while they are being
 * listed), or null once they were.
 */
export function mixBackupsView(state) {
  const { list, error } = state.mixBackups;
  return {
    rows: mixBackupRows(list),
    unread: list === null ? (error ?? "") : null,
    engineRunning: state.engine.status !== null,
  };
}

/**
 * What a restore did, from the `restore_mix_backup` answer `{ backup, restored: [{ address,
 * follows, mix }], skipped: [{ key, label, reason, error }] }`, in sentences. `labelAt(address)`
 * names a musician a mix went to, `nameOf(skipped)` the one a skipped mix was for (see
 * skippedName in sessions.js).
 */
export function formatMixRestored(outcome, { labelAt, nameOf }) {
  const when = formatBackupTime(outcome.backup?.savedAt);
  const sentences = [];
  const full = outcome.restored.filter((mix) => !mix.follows);
  if (full.length > 0) {
    const names = joinNames(full.map((mix) => labelAt(mix.address)));
    sentences.push(`Restored the mixes backed up${when ? ` at ${when}` : ""} to ${names}.`);
  } else if (outcome.restored.length === 0) {
    sentences.push("No musician's mix was restored from the backup.");
  }
  for (const mix of outcome.restored.filter((restored) => restored.follows)) {
    sentences.push(
      `${labelAt(mix.address)} follows ${mix.follows}'s mix, so only their master volume and ` +
        "mute were set.",
    );
  }
  const skipped = (reason) => outcome.skipped.filter((mix) => mix.reason === reason).map(nameOf);
  const away = skipped("notConnected");
  if (away.length > 0) {
    sentences.push(
      `Not restored to ${joinNames(away)}: not connected, and a phone's own mix wins when it connects.`,
    );
  }
  const gone = skipped("notHere");
  if (gone.length > 0) {
    sentences.push(`Not restored to ${joinNames(gone)}: not here.`);
  }
  for (const mix of outcome.skipped.filter((skippedMix) => skippedMix.reason === "refused")) {
    sentences.push(sentenceWithReason(`Not restored to ${nameOf(mix)}`, mix.error));
  }
  return sentences.join(" ");
}

/**
 * The Mix backups part's behaviour: Restore asks first, and only the confirming Restore emits
 * `mix-backup-restore` { stamp } through `emit(type, detail)`.
 */
export function createMixBackupsPanel({ emit }) {
  let rows = [];
  let asking = null;

  const row = (stamp) => rows.find((candidate) => candidate.stamp === stamp) ?? null;

  return {
    /** Takes the rows (mixBackupRows); a backup no longer listed is no longer asked about. */
    setRows(next) {
      rows = next;
      if (asking && !row(asking)) {
        asking = null;
      }
    },

    /** Restore pressed for `stamp`: asks first, for a backup that can be restored. */
    ask(stamp) {
      asking = row(stamp)?.restorable ? stamp : null;
    },

    /** The backup asked about, as listed (`{ stamp, label, detail }`), or null. */
    asking: () => (asking ? row(asking) : null),

    /** "Cancel": nothing is restored. */
    cancel() {
      asking = null;
    },

    /** The confirming "Restore": the backup asked about is restored. */
    confirm() {
      if (asking) {
        const stamp = asking;
        asking = null;
        emit("mix-backup-restore", { stamp });
      }
    },
  };
}
