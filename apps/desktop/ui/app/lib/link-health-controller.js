// The link health panel's wiring on the Network and system screen, without the DOM: which
// musician's panel is open, its range and its table, and the follower that reads their history
// while it is open (link-health-view.js). The screen hands it the listed musicians on each render
// and draws what `view` answers.
//
// - Picking the open musician again closes the panel; picking another one follows them instead.
// - The panel goes, and the follower stops, when the musician leaves the list (removed, or the
//   engine stopped: nobody is listed then), and when the screen unmounts (`dispose`).
// - A phone listed by its address that then registers changes key (`address:` to `client:`): the
//   panel stays with the row at that address and follows it by its new key (followedMusician).

import {
  DEFAULT_RANGE_MS,
  createHistoryFollower,
  followedMusician,
  linkHealthView,
  musicianKey,
} from "./link-health-view.js";

/**
 * The panel's controller. `read(key, sinceSeq)` answers as the console app's `linkHistory`;
 * `onChange()` asks the screen to draw again (a new sample, or a read that failed); `schedule`,
 * `cancel` and `intervalMs` are the follower's clock (see createHistoryFollower).
 */
export function createLinkHealthController({ read, onChange, schedule, cancel, intervalMs, timeZone }) {
  // The musician whose panel is open (their key and where they were last listed), its range and
  // whether its table shows; the history held for them and why the last read failed.
  const open = {
    key: null,
    address: null,
    rangeMs: DEFAULT_RANGE_MS,
    showTable: false,
    buffer: null,
    error: null,
  };
  const follower = createHistoryFollower({
    read,
    onChange: (buffer, error) => {
      open.buffer = buffer;
      open.error = error;
      onChange();
    },
    schedule,
    cancel,
    intervalMs,
  });

  function close() {
    follower.stop();
    Object.assign(open, { key: null, address: null, buffer: null, error: null, showTable: false });
  }

  function follow(musician) {
    open.key = musicianKey(musician);
    open.address = musician.address;
    void follower.follow(open.key);
  }

  // The open musician as listed now, following a registration to its new key; closes the panel
  // when they are no longer listed.
  function current(musicians) {
    if (open.key === null) {
      return null;
    }
    const musician = followedMusician(musicians, open.key, open.address);
    if (musician === null) {
      close();
      return null;
    }
    if (musicianKey(musician) !== open.key) {
      // Same phone, now registered: the history held stays shown until the new key's read.
      follow(musician);
    }
    open.address = musician.address;
    return musician;
  }

  return {
    /** Opens the panel of the musician listed at `address`, or closes it if they are the open one. */
    select(musicians, address) {
      const shown = current(musicians);
      const picked = musicians.find((musician) => musician.address === address);
      close();
      if (picked && (shown === null || musicianKey(shown) !== musicianKey(picked))) {
        follow(picked);
      }
    },
    close,
    setRange(rangeMs) {
      open.rangeMs = rangeMs;
    },
    showTable(shown) {
      open.showTable = shown;
    },
    /** Stops following for good (the screen unmounts). */
    dispose() {
      close();
    },
    /**
     * What to draw for `musicians` (the listed rows): `{ selected, data }`, the open musician's
     * address for the table and the panel's view (linkHealthView), both null when it is closed.
     */
    view(musicians) {
      const musician = current(musicians);
      if (musician === null) {
        return { selected: null, data: null };
      }
      return {
        selected: musician.address,
        data: linkHealthView({
          musician,
          buffer: open.buffer,
          rangeMs: open.rangeMs,
          error: open.error,
          showTable: open.showTable,
          timeZone,
        }),
      };
    },
  };
}
