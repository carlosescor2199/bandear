// The Record screen (board 2 of the design): the transport, one track per recorded input with its
// arm button, meter and waveform lane, the takes in the recordings folder, the virtual soundcheck
// card and the recording settings. The container renders what record-view.js works out, at most once per animation
// frame, and turns the controls into console commands (see console-app.js), each outcome said in
// words above the transport.
//
// What the screen decides (the transport's busy guard, the lanes' peak histories, arming, the
// folder field, the soundcheck's Play, Stop, seek and loop) lives in record-controller.js, tested
// without a DOM; this file only reads and writes elements. The waveform lanes are drawn from the
// `meters` event; no engine call is involved.
//
// Play is the virtual soundcheck (see soundcheck-view.js): it plays the take the soundcheck card
// picks, else the last take, through the engine in place of the live inputs, so the local output
// and every phone hear it; there is no separate "listen only" playback. The takes table's
// Soundcheck plays that take the same way, and the card shows where the take is, with its seek
// slider, pause, loop and "back to live inputs".

import "../components/meter.js";
import "../components/record-track.js";
import { createRecordController } from "../lib/record-controller.js";
import {
  diskView,
  localTrackRow,
  recordMessages,
  settingsView,
  takeRows,
  trackRows,
  transportView,
} from "../lib/record-view.js";
import { formatUnreadable } from "../lib/recording.js";
import { SOUNDCHECK_BLOCKS_RECORDING } from "../lib/soundcheck.js";
import { soundcheckView } from "../lib/soundcheck-view.js";
import { recordingStatus } from "../lib/console-state.js";

const PLAY_ICON = `<svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4v16l13-8z"/></svg>`;
const PAUSE_ICON = `<svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>`;

const LAYOUT = `
  <header class="screen-header">
    <h1 tabindex="-1">Record</h1>
    <p>Every input on its own track, written straight from the engine while the show runs.</p>
  </header>
  <p class="mix-message" role="status" data-outcome hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-outcome-error hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-failure hidden></p>
  <section class="record-card record-transport" aria-label="Transport">
    <div class="record-buttons">
      <button type="button" class="record-icon" data-rewind aria-label="Back to start" title="Back to start">
        <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M6 5h2v14H6zM20 5v14L9 12z"/></svg>
      </button>
      <button type="button" class="record-icon" data-play aria-label="Play" data-icon="play">${PLAY_ICON}</button>
      <button type="button" class="record-icon" data-stop aria-label="Stop recording" title="Stop recording">
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><rect x="5" y="5" width="14" height="14" rx="2"/></svg>
      </button>
      <button type="button" class="record-rec" data-record aria-pressed="false">
        <span class="record-dot" aria-hidden="true"></span><span data-record-word>Record</span>
      </button>
    </div>
    <div class="record-position">
      <div class="record-time" data-position aria-label="Position">0:00</div>
      <div class="record-state" role="status" data-state></div>
    </div>
    <div class="record-field">
      <label for="record-take-name">Take</label>
      <input id="record-take-name" data-take-name maxlength="60" autocomplete="off" placeholder="Take" />
    </div>
    <button type="button" class="record-button" data-mark>Add marker</button>
    <span class="record-spacer"></span>
    <dl class="record-summary">
      <dt>Armed</dt><dd data-armed></dd>
      <dt>Format</dt><dd data-format></dd>
      <dt>Disk</dt><dd><span class="ml-mono" data-free></span><span data-left></span></dd>
    </dl>
    <p class="record-disk-warning" role="alert" data-disk-warning hidden></p>
    <p class="record-hint record-full" data-disk-problem hidden></p>
    <div class="record-markers record-full" data-markers-box hidden>
      <h2 class="record-label" data-markers-title>Markers</h2>
      <ol class="record-marker-list" data-markers></ol>
    </div>
  </section>
  <p class="record-hint" data-locked hidden></p>
  <section class="record-card record-tracks" aria-labelledby="record-tracks-title">
    <h2 class="record-label record-tracks-title" id="record-tracks-title">Tracks</h2>
    <p class="record-empty" data-no-tracks hidden>
      The tracks are the running device's inputs: start the engine from the
      <a href="#/mix">Mix</a> screen to see and arm them.
    </p>
    <div data-tracks></div>
  </section>
  <div class="record-lower">
    <section class="record-card record-takes" aria-labelledby="record-takes-title">
      <div class="record-card-head">
        <h2 id="record-takes-title">Takes</h2>
        <span class="record-folder ml-mono" data-takes-folder></span>
        <button type="button" class="record-button" data-refresh>Refresh</button>
      </div>
      <p class="mix-message mix-message--error" role="alert" data-takes-error hidden></p>
      <p class="record-hint" data-takes-note hidden></p>
      <div class="record-table-box">
        <table class="record-table">
          <thead>
            <tr>
              <th scope="col">Take</th>
              <th scope="col">Started</th>
              <th scope="col">Length</th>
              <th scope="col">Ended</th>
              <th scope="col"><span class="ml-visually-hidden">Actions</span></th>
            </tr>
          </thead>
          <tbody data-takes></tbody>
        </table>
      </div>
      <p class="record-empty" data-no-takes hidden></p>
    </section>
    <aside class="record-side" aria-label="Soundcheck and recording settings">
      <section class="record-card record-soundcheck" aria-labelledby="record-soundcheck-title">
        <div class="record-soundcheck-head">
          <h2 id="record-soundcheck-title">Virtual soundcheck</h2>
          <span class="record-badge" data-sc-badge></span>
        </div>
        <div class="record-soundcheck-pick" data-sc-off>
          <label for="record-sc-take">Play back as the live inputs</label>
          <select id="record-sc-take" data-sc-take></select>
          <button type="button" class="record-button" data-sc-start>Start soundcheck</button>
          <p class="record-hint" data-sc-hint hidden></p>
        </div>
        <div class="record-soundcheck-now" data-sc-on hidden>
          <div class="record-soundcheck-take">
            <span class="record-soundcheck-name" data-sc-name></span>
            <span class="ml-mono" data-sc-time></span>
          </div>
          <input type="range" class="record-soundcheck-seek" min="0" step="0.1" data-sc-seek aria-label="Position in the take" />
          <div class="record-folder-actions">
            <button type="button" class="record-button" data-sc-pause>Pause</button>
            <button type="button" class="record-button" data-sc-stop>Back to live inputs</button>
          </div>
          <p class="record-hint" data-sc-clock></p>
          <p class="record-hint" data-sc-underruns></p>
          <p class="mix-message mix-message--error" role="alert" data-sc-failure hidden></p>
        </div>
        <label class="record-check">
          <input type="checkbox" data-sc-loop />
          Loop the take
        </label>
        <p class="record-hint">
          Each track plays into the input it was recorded from, in place of the interface, so every
          mix, the local output and the phones can be set with nobody playing. The interface stays
          open: going back to the live inputs is instant.
        </p>
      </section>
      <section class="record-card record-settings" aria-labelledby="record-settings-title">
        <h2 id="record-settings-title">Settings</h2>
        <form class="record-folder-form" data-folder-form>
          <label for="record-folder">Folder</label>
          <input id="record-folder" class="ml-mono" data-folder autocomplete="off" spellcheck="false" />
          <div class="record-folder-actions">
            <button type="submit" class="record-button" data-folder-use>Use this folder</button>
            <button type="button" class="record-button" data-folder-default>Default folder</button>
          </div>
        </form>
        <p class="record-hint" data-folder-note></p>
        <p class="record-caution" role="status" data-notice hidden></p>
        <div class="record-format">
          <label for="record-format">Files</label>
          <select id="record-format" data-format-select></select>
        </div>
        <label class="record-check">
          <input type="checkbox" data-include-local />
          Also record the local mix (stereo)
        </label>
        <p class="record-hint">
          Tracks are written before any processing, and a linked pair stays one stereo file. A file
          that passes 4 GB switches to RF64 on its own.
        </p>
      </section>
    </aside>
  </div>
`;

// Keeps one child of `container` per row, in the rows' order, each kept by its key.
function keyedChildren(container, rows, create) {
  const byKey = new Map([...container.children].map((child) => [child.dataset.key, child]));
  const children = rows.map((row) => {
    let child = byKey.get(row.key);
    if (child) {
      byKey.delete(row.key);
    } else {
      child = create();
      child.dataset.key = row.key;
    }
    return child;
  });
  for (const stale of byKey.values()) {
    stale.remove();
  }
  children.forEach((child, position) => {
    const here = container.children[position];
    if (here !== child) {
      container.insertBefore(child, here ?? null);
    }
  });
  return children;
}

function cell(text, className = null) {
  const td = document.createElement("td");
  td.textContent = text;
  if (className) {
    td.className = className;
  }
  return td;
}

// One row of the takes table.
function takeLine(row) {
  const tr = document.createElement("tr");
  const nameCell = document.createElement("td");
  const name = document.createElement("div");
  name.className = "record-take-name";
  if (row.recording) {
    const dot = document.createElement("span");
    dot.className = "record-dot";
    dot.setAttribute("aria-hidden", "true");
    name.append(dot);
  }
  name.append(row.name);
  const detail = document.createElement("div");
  detail.className = "record-take-detail";
  detail.textContent = [row.tracks, row.markers, row.dropped].filter(Boolean).join(" · ");
  nameCell.append(name, detail);
  const ended = document.createElement("td");
  const badge = document.createElement("span");
  badge.className = "record-badge";
  badge.dataset.kind = row.end.kind;
  badge.textContent = row.end.text;
  badge.title = row.end.detail;
  ended.append(badge);
  const actions = document.createElement("td");
  actions.className = "record-actions";
  const open = document.createElement("button");
  open.type = "button";
  open.className = "record-button";
  open.dataset.open = row.folder;
  open.textContent = "Open folder";
  open.setAttribute("aria-label", `Open the folder of ${row.name}`);
  // A take's Play is its soundcheck: it plays through the engine in place of the live inputs.
  const check = document.createElement("button");
  check.type = "button";
  check.className = "record-button";
  check.dataset.soundcheck = row.folder;
  check.textContent = row.soundcheck.playing ? "Playing" : "Soundcheck";
  check.disabled = row.soundcheck.disabled;
  check.title = row.soundcheck.title;
  check.setAttribute("aria-label", row.soundcheck.title);
  actions.append(check, open);
  tr.append(
    nameCell,
    cell(row.started),
    cell(row.length, "ml-mono"),
    ended,
    actions,
  );
  return tr;
}

/** Mounts the Record screen in `main` for the console `app`; returns the unmount function. */
export function mountRecordScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "record-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const $ = (selector) => screen.querySelector(selector);
  const parts = {
    outcome: $("[data-outcome]"),
    outcomeError: $("[data-outcome-error]"),
    failure: $("[data-failure]"),
    rewind: $("[data-rewind]"),
    play: $("[data-play]"),
    stop: $("[data-stop]"),
    record: $("[data-record]"),
    recordWord: $("[data-record-word]"),
    position: $("[data-position]"),
    state: $("[data-state]"),
    takeName: $("[data-take-name]"),
    mark: $("[data-mark]"),
    armed: $("[data-armed]"),
    format: $("[data-format]"),
    free: $("[data-free]"),
    left: $("[data-left]"),
    diskWarning: $("[data-disk-warning]"),
    diskProblem: $("[data-disk-problem]"),
    markersBox: $("[data-markers-box]"),
    markersTitle: $("[data-markers-title]"),
    markers: $("[data-markers]"),
    locked: $("[data-locked]"),
    noTracks: $("[data-no-tracks]"),
    tracks: $("[data-tracks]"),
    takesFolder: $("[data-takes-folder]"),
    takesError: $("[data-takes-error]"),
    takesNote: $("[data-takes-note]"),
    takes: $("[data-takes]"),
    noTakes: $("[data-no-takes]"),
    refresh: $("[data-refresh]"),
    folderForm: $("[data-folder-form]"),
    folder: $("[data-folder]"),
    folderUse: $("[data-folder-use]"),
    folderDefault: $("[data-folder-default]"),
    folderNote: $("[data-folder-note]"),
    notice: $("[data-notice]"),
    formatSelect: $("[data-format-select]"),
    includeLocal: $("[data-include-local]"),
    scBadge: $("[data-sc-badge]"),
    scOff: $("[data-sc-off]"),
    scTake: $("[data-sc-take]"),
    scStart: $("[data-sc-start]"),
    scHint: $("[data-sc-hint]"),
    scOn: $("[data-sc-on]"),
    scName: $("[data-sc-name]"),
    scTime: $("[data-sc-time]"),
    scSeek: $("[data-sc-seek]"),
    scPause: $("[data-sc-pause]"),
    scStop: $("[data-sc-stop]"),
    scClock: $("[data-sc-clock]"),
    scUnderruns: $("[data-sc-underruns]"),
    scFailure: $("[data-sc-failure]"),
    scLoop: $("[data-sc-loop]"),
  };

  const controller = createRecordController(app, { onChange: () => render() });
  const local = controller.local;
  // What was last written into the takes table, the markers list and the soundcheck's take list.
  const drawn = { takesKey: null, markersKey: null, choicesKey: null };

  function show(element, text) {
    element.hidden = !text;
    element.textContent = text ?? "";
  }

  function renderTransport(state) {
    const view = transportView(state, local);
    const busy = local.busy !== null;
    parts.record.disabled = busy || !view.canRecord;
    parts.record.setAttribute("aria-pressed", String(view.recording));
    parts.record.dataset.recording = String(view.recording);
    parts.recordWord.textContent = view.recording ? "Recording" : "Record";
    if (view.canRecord || view.recording) {
      parts.record.removeAttribute("title");
    } else {
      parts.record.title = view.soundcheck ? SOUNDCHECK_BLOCKS_RECORDING : view.stateWord;
    }
    parts.stop.disabled = busy || !view.canStop;
    parts.stop.setAttribute("aria-label", view.stopLabel);
    parts.stop.title = view.stopLabel;
    parts.play.disabled = busy || !view.play.can;
    parts.play.setAttribute("aria-label", view.play.label);
    parts.play.title = view.play.label;
    const icon = view.play.action === "pause" ? "pause" : "play";
    if (parts.play.dataset.icon !== icon) {
      parts.play.dataset.icon = icon;
      parts.play.innerHTML = icon === "pause" ? PAUSE_ICON : PLAY_ICON;
    }
    parts.mark.disabled = busy || !view.canMark;
    parts.rewind.disabled = !view.canRewind;
    parts.takeName.disabled = view.recording;
    parts.position.textContent = view.position;
    parts.state.textContent = view.stateWord;
    parts.state.dataset.kind = view.stateKind;
    parts.armed.textContent = view.armed;
    parts.format.textContent = view.format;
    const markersKey = JSON.stringify([view.markersOf, view.markers]);
    if (markersKey !== drawn.markersKey) {
      drawn.markersKey = markersKey;
      parts.markersBox.hidden = view.markers.length === 0;
      parts.markersTitle.textContent = view.markersOf ? `Markers · ${view.markersOf}` : "Markers";
      parts.markers.replaceChildren(
        ...view.markers.map((marker) => {
          const item = document.createElement("li");
          const time = document.createElement("span");
          time.className = "ml-mono";
          time.textContent = marker.time;
          item.append(time, ` ${marker.name}`);
          return item;
        }),
      );
    }

    const disk = diskView(state);
    parts.free.textContent = disk.free;
    parts.left.textContent = disk.left ? ` · ${disk.left}` : "";
    show(parts.diskProblem, disk.problem);
    show(parts.diskWarning, disk.warning?.text ?? null);
    parts.diskWarning.dataset.level = disk.warning?.level ?? "";
  }

  function renderTracks(state) {
    const recording = recordingStatus(state);
    const locked = recording.state === "recording" || recording.state === "stopping";
    const rows = trackRows(state);
    const localRow = localTrackRow(state);
    const all = localRow && rows.length > 0 ? [...rows, localRow] : rows;
    parts.noTracks.hidden = rows.length > 0;
    keyedChildren(parts.tracks, all, () => document.createElement("ml-record-track")).forEach(
      (element, position) => {
        const row = all[position];
        element.data = { ...row, locked, lane: controller.laneOf(row, state) };
      },
    );
  }

  function renderTakes(state) {
    const takes = state.takes;
    parts.takesFolder.textContent = takes.folder ?? "";
    show(parts.takesError, recordMessages(state).takes);
    show(parts.takesNote, formatUnreadable(takes));
    const rows = takeRows(state, -new Date().getTimezoneOffset());
    let empty = null;
    if (takes.list === null) {
      empty = takes.error ? null : "Listing the takes…";
    } else if (rows.length === 0) {
      empty = takes.missing
        ? "The recordings folder does not exist yet: the first take makes it."
        : "No takes in this folder yet.";
    }
    show(parts.noTakes, empty);
    const key = JSON.stringify(rows);
    if (key !== drawn.takesKey) {
      drawn.takesKey = key;
      parts.takes.replaceChildren(...rows.map(takeLine));
    }
  }

  function renderSettings(state) {
    const view = settingsView(state);
    const off = !view.known || view.locked;
    for (const control of [
      parts.folder,
      parts.folderUse,
      parts.folderDefault,
      parts.formatSelect,
      parts.includeLocal,
    ]) {
      control.disabled = off;
    }
    parts.folderDefault.disabled = off || view.isDefault;
    // The field shows the desk's folder unless the engineer is typing in it.
    const folder = controller.folderToShow(state, document.activeElement === parts.folder);
    if (folder !== null) {
      parts.folder.value = folder;
    }
    parts.folderNote.textContent = view.isDefault
      ? "This is the default folder."
      : `The default folder is ${view.defaultFolder}.`;
    show(parts.notice, view.notice);
    const optionsKey = view.formats.map((option) => option.value).join();
    if (parts.formatSelect.dataset.options !== optionsKey) {
      parts.formatSelect.dataset.options = optionsKey;
      parts.formatSelect.replaceChildren(
        ...view.formats.map((option) => new Option(option.label, option.value)),
      );
    }
    parts.formatSelect.value = view.format;
    parts.includeLocal.checked = view.includeLocal;
  }

  function renderSoundcheck(state) {
    const view = soundcheckView(state, {
      chosen: local.chosenTake,
      drag: local.drag,
      loopNext: local.loopNext,
    });
    const busy = local.busy !== null;
    parts.scBadge.textContent = view.badge;
    parts.scBadge.dataset.kind = { off: "off", paused: "warn", failed: "error" }[view.kind] ?? "ok";
    parts.scOff.hidden = view.on;
    parts.scOn.hidden = !view.on;
    const choicesKey = JSON.stringify(view.choices);
    if (choicesKey !== drawn.choicesKey) {
      drawn.choicesKey = choicesKey;
      parts.scTake.replaceChildren(
        ...view.choices.map((choice) => new Option(choice.label, choice.folder)),
      );
    }
    parts.scTake.value = view.chosen ?? "";
    parts.scTake.disabled = view.choices.length === 0;
    parts.scStart.disabled = busy || !view.canStart;
    show(parts.scHint, view.startHint);
    parts.scName.textContent = view.takeName ?? "";
    parts.scTime.textContent = `${view.position} / ${view.length}`;
    parts.scSeek.max = String(view.slider.max);
    if (local.drag === null || document.activeElement !== parts.scSeek) {
      parts.scSeek.value = String(view.slider.value);
    }
    parts.scSeek.disabled = Boolean(view.slider.disabled);
    parts.scSeek.setAttribute("aria-valuetext", `${view.position} of ${view.length}`);
    parts.scPause.disabled = busy || !(view.canPause || view.canResume);
    parts.scPause.textContent = view.canPause ? "Pause" : view.resumeWord;
    parts.scStop.disabled = busy || !view.canStop;
    parts.scClock.textContent = view.clock;
    parts.scUnderruns.textContent = view.underruns;
    show(parts.scFailure, view.failure);
    parts.scLoop.checked = view.looping;
    parts.scLoop.disabled = !view.canLoop;
  }

  function render() {
    const state = app.store.getState();
    controller.followTake(state);
    const messages = recordMessages(state);
    show(parts.outcome, messages.outcome?.ok ? messages.outcome.text : null);
    show(parts.outcomeError, messages.outcome && !messages.outcome.ok ? messages.outcome.text : null);
    show(parts.failure, messages.failure);
    show(parts.locked, messages.locked);
    renderTransport(state);
    renderTracks(state);
    renderTakes(state);
    renderSoundcheck(state);
    renderSettings(state);
  }

  let frame = 0;
  function scheduleRender() {
    if (frame === 0) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        render();
      });
    }
  }

  // The lanes are drawn at most once a frame, after the meters that changed them.
  let laneFrame = 0;
  function scheduleLanes() {
    if (laneFrame !== 0) {
      return;
    }
    laneFrame = requestAnimationFrame(() => {
      laneFrame = 0;
      for (const element of parts.tracks.children) {
        const history = controller.history(element.dataset.key);
        if (history && element.data?.lane === "recording") {
          element.drawHistory(history);
        }
      }
    });
  }

  function onMeters(meters) {
    const elements = [...parts.tracks.children].filter((element) => element.data);
    const levels = controller.takeMeters(elements.map((element) => element.data), meters);
    elements.forEach((element, position) => element.setLevel(levels[position]));
    if (controller.taking()) {
      scheduleLanes();
    }
  }

  // A transport command: its buttons wait until it is answered; the outcome is the store's.
  parts.record.addEventListener("click", () =>
    controller.command("start", () => app.startRecording(parts.takeName.value)),
  );
  // Stop goes back to the live inputs while a take plays in place of them, else stops the take.
  parts.stop.addEventListener("click", () => controller.stop());
  parts.play.addEventListener("click", () => controller.play());
  parts.mark.addEventListener("click", () => controller.command("mark", () => app.addMarker("")));
  parts.rewind.addEventListener("click", () => controller.rewind());
  parts.scTake.addEventListener("change", () => controller.chooseTake(parts.scTake.value));
  parts.scStart.addEventListener("click", () => {
    if (parts.scTake.value) {
      controller.soundcheckTake(parts.scTake.value);
    }
  });
  parts.scPause.addEventListener("click", () => controller.play());
  parts.scStop.addEventListener("click", () => controller.stop());
  parts.scSeek.addEventListener("input", () => controller.drag(parts.scSeek.value));
  parts.scSeek.addEventListener("change", () => controller.seekTo(parts.scSeek.value));
  parts.scLoop.addEventListener("change", () => controller.setLoop(parts.scLoop.checked));
  parts.refresh.addEventListener("click", async () => {
    await app.refreshTakes();
    render();
  });
  parts.takes.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-open]");
    if (button) {
      app.openTakeFolder(button.dataset.open).then(render);
    }
    const check = event.target.closest("button[data-soundcheck]");
    if (check) {
      controller.soundcheckTake(check.dataset.soundcheck);
    }
  });
  screen.addEventListener("track-arm", ({ detail: { key } }) => controller.arm(key));
  parts.folderForm.addEventListener("submit", (event) => {
    event.preventDefault();
    // A refused folder stays in the field, so it can be corrected (see folderToShow).
    controller.useFolder(parts.folder.value);
  });
  parts.folderDefault.addEventListener("click", () => controller.useDefaultFolder());
  parts.formatSelect.addEventListener("change", () =>
    controller.changeSettings({ format: parts.formatSelect.value }),
  );
  parts.includeLocal.addEventListener("change", () =>
    controller.changeSettings({ includeLocal: parts.includeLocal.checked }),
  );

  const unsubscribe = app.store.subscribe(scheduleRender);
  const stopMeters = app.onMeters(onMeters);
  render();
  if (app.store.getState().recordingSettings === null) {
    app.loadRecordingSettings();
  }
  app.refreshTakes();
  app.refreshEstimate();

  return () => {
    unsubscribe();
    stopMeters();
    cancelAnimationFrame(frame);
    cancelAnimationFrame(laneFrame);
    screen.remove();
  };
}
