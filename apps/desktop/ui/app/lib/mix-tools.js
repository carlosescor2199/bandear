// The Mix tools' behaviour apart from the DOM, so what <ml-mix-tools> does can be tested without a
// browser: which panel is open and where the focus goes when it closes, which musicians are ticked,
// what the Mirror control shows, when a request may go out, and the guarded delete with its timers.
// The element only renders what this answers and forwards the engineer's actions to it. No DOM.
// The Mix screen runs the tools' commands and hands every answer back through mixToolCommands.
//
// It also words a strip's mirror (followingState), which <ml-channel-strip> and <ml-group-strip>
// show.

import { CONFIRM_DELAY_MS, confirmReady } from "./network-view.js";

/** How long an armed delete stays armed before it goes back to a plain Delete. */
export const ARMED_FOR_MS = 10000;

/**
 * How a strip of a mix that follows `following`'s (a name, or null) shows it: `{ disabled, text,
 * label }`, the strip's level, pan, mute and solo (or group fader) taking no move while it follows,
 * the words under its name, and its accessible name from `name`.
 */
export function followingState(following, name) {
  return {
    disabled: Boolean(following),
    text: following ? `Follows ${following}` : "",
    label: following ? `${name}, follows ${following}'s mix` : name,
  };
}

/**
 * The tools of one Mix screen. `emit(type, detail)` sends the element's events; `changed()` asks
 * the element to render again (a delete that became confirmable, or disarmed itself); `now`,
 * `setTimer` and `clearTimer` are the clock and timers, stand-ins in the tests.
 */
export function createMixTools({
  emit,
  changed = () => {},
  now = () => performance.now(),
  setTimer = setTimeout,
  clearTimer = clearTimeout,
}) {
  let data = null;
  let open = null;
  const copyTicks = new Set();
  let applyTicks = null;
  // The Mirror choice sent and not answered yet; the desk's own value shows once it is answered or
  // the desk's mirror changes.
  let pendingMirror = null;
  let armed = null;
  let timers = [];

  function disarm() {
    armed = null;
    for (const timer of timers.splice(0)) {
      clearTimer(timer);
    }
  }

  function targets(list) {
    return (list === "copy" ? data?.copyTargets : data?.presetTargets) ?? [];
  }

  function ticksOf(list) {
    return list === "copy" ? copyTicks : applyTicks;
  }

  // Only musicians that can be reached stay ticked, in the order the targets list them.
  function ticked(list) {
    const ticks = ticksOf(list);
    return targets(list)
      .filter((target) => !target.disabled && ticks?.has(target.address))
      .map((target) => target.address);
  }

  return {
    /** Takes the tools' data (mixToolsView and the open musician's `address`). */
    setData(next) {
      if (data?.address !== next.address) {
        // Another musician's mix: nothing ticked, chosen or armed for the last one stays.
        copyTicks.clear();
        applyTicks = null;
        pendingMirror = null;
        disarm();
      } else if (data.mirror?.value !== next.mirror?.value) {
        // The desk's mirror changed (the answer's status, or any other route): it shows now.
        pendingMirror = null;
      }
      data = next;
      for (const target of [...data.copyTargets, ...data.presetTargets]) {
        if (target.disabled) {
          copyTicks.delete(target.address);
          applyTicks?.delete(target.address);
        }
      }
      if (applyTicks === null && data.presets?.length) {
        // The open musician is ticked to begin with, unless their mix follows another's.
        applyTicks = new Set(
          data.presetTargets
            .filter((target) => target.address === data.address && !target.disabled)
            .map((target) => target.address),
        );
      }
      if (armed && !data.presets?.some((preset) => preset.id === armed.id)) {
        disarm();
      }
    },

    /**
     * The value the Mirror control shows: a choice on its way while it is still offered and the
     * desk's mirror has not changed since, else the desk's own.
     */
    mirrorValue() {
      const offered = data?.mirror.options.some((option) => option.value === pendingMirror);
      return pendingMirror !== null && offered ? pendingMirror : (data?.mirror.value ?? "");
    },

    /** The engineer chose `value` in the Mirror control. */
    chooseMirror(value) {
      // "away" is the leader who is not here: what the mix follows, not a new choice.
      if (!data || value === "away" || value === this.mirrorValue()) {
        return;
      }
      pendingMirror = value;
      emit("mirror-change", { leader: value || null });
    },

    /**
     * The screen's command for `kind` was answered, `ok` or not: `{ clearSave, clearRename }` say
     * which fields to empty. Whatever the answer, the Mirror control shows the desk's mirror again.
     */
    answered(kind, ok) {
      if (kind === "mirror") {
        pendingMirror = null;
      }
      if (ok && kind === "copy") {
        copyTicks.clear();
      }
      return { clearSave: ok && kind === "save", clearRename: ok && kind === "rename" };
    },

    /** The panel open ("copy", "presets") or null. */
    open: () => open,

    /**
     * Opens `panel`, or closes it when it is open: `{ focus }` names where the focus goes, "panel"
     * (into the panel opened) or the panel whose button takes it back. Opening Presets before they
     * could be read asks for them again.
     */
    toggle(panel) {
      const previous = open;
      open = panel === open ? null : panel;
      if (open === "presets" && data?.presets === null) {
        emit("presets-load", {});
      }
      if (open) {
        return { focus: "panel" };
      }
      disarm();
      return { focus: previous };
    },

    /** Escape in a panel: closes it, and the focus goes back to its button. */
    escape() {
      if (open === null) {
        return { focus: null };
      }
      return this.toggle(open);
    },

    /** The engineer ticked or unticked `address` in `list` ("copy" or "apply"). */
    tick(list, address, checked) {
      const target = targets(list).find((candidate) => candidate.address === address);
      const ticks = ticksOf(list);
      if (!target || target.disabled || !ticks) {
        return;
      }
      if (checked) {
        ticks.add(address);
      } else {
        ticks.delete(address);
      }
    },

    /** Whether `address` shows ticked in `list`. */
    isTicked: (list, address) => ticked(list).includes(address),

    copyReady: () => Boolean(data?.canUse) && ticked("copy").length > 0,
    applyReady: (id) => Boolean(data?.canUse && id) && ticked("apply").length > 0,
    saveReady: (name) => Boolean(data?.canUse) && String(name ?? "").trim() !== "",

    copy() {
      if (this.copyReady()) {
        emit("mix-copy", { to: ticked("copy") });
      }
    },

    save(name) {
      if (this.saveReady(name)) {
        emit("preset-save", { name: name.trim() });
      }
    },

    apply(id) {
      if (this.applyReady(id)) {
        emit("preset-apply", { id, to: ticked("apply") });
      }
    },

    /** Renaming needs no engine: the presets live in the desk's session. */
    rename(id, name) {
      const trimmed = String(name ?? "").trim();
      if (id && trimmed) {
        emit("preset-rename", { id, name: trimmed });
      }
    },

    /** Delete pressed for the preset `id`: it asks first, and disarms itself after a while. */
    arm(id) {
      disarm();
      armed = { id, at: now() };
      timers.push(
        setTimer(changed, CONFIRM_DELAY_MS),
        setTimer(() => {
          disarm();
          changed();
        }, ARMED_FOR_MS),
      );
    },

    /** Keep pressed (or Escape): the preset stays. */
    keep: disarm,

    /** The confirming Delete for `id`: works only once armed long enough. */
    confirmDelete(id) {
      if (armed?.id !== id || !confirmReady(armed.at, now())) {
        return false;
      }
      disarm();
      emit("preset-delete", { id });
      return true;
    },

    /** How the delete for `id` shows: `{ armed, ready }`. */
    deleteState(id) {
      const here = armed?.id === id;
      return { armed: here, ready: here && confirmReady(armed.at, now()) };
    },

    /** Stops the timers, when the element leaves the page. */
    dispose: disarm,
  };
}

/**
 * The Mix screen's side of the tools: `handle(type, detail)` runs the console `app`'s command for
 * one of <ml-mix-tools>' events (`types`) on the open musician's mix, then hands its answer `{ ok,
 * message }` to `said(outcome)`, which says it above the strips and gives the tools the store's
 * data at once, and then to `tools.answered(kind, ok)`, so the Mirror control goes from the choice
 * on its way straight to the desk's mirror the answer brought. The tools are answered every time:
 * a command that fails (its error goes on to the caller) or finds no musician open is answered as
 * not done.
 */
export function mixToolCommands({ app, tools, said }) {
  const commands = {
    "mirror-change": ["mirror", (address, detail) => app.setMirror(address, detail.leader)],
    "mix-copy": ["copy", (address, detail) => app.copyMix(address, detail.to)],
    "preset-save": ["save", (address, detail) => app.savePreset(address, detail.name)],
    "preset-apply": ["apply", (_, detail) => app.applyPreset(detail.id, detail.to)],
    "preset-rename": ["rename", (_, detail) => app.renamePreset(detail.id, detail.name)],
    "preset-delete": ["delete", (_, detail) => app.deletePreset(detail.id)],
  };
  return {
    types: Object.keys(commands),
    async handle(type, detail) {
      const [kind, run] = commands[type];
      const address = app.store.getState().selected;
      let outcome = null;
      try {
        // The tools act on the open musician's mix: with none open nothing is run.
        outcome = address ? await run(address, detail) : null;
      } finally {
        // The screen first, so the tools already hold the status the answer brought, then the
        // tools whatever happened, so a Mirror choice never stays on show unanswered.
        if (outcome) {
          said(outcome);
        }
        tools.answered(kind, outcome?.ok === true);
      }
      return outcome;
    },
  };
}
