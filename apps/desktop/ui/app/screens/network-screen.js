// The Network and system screen (the phase-1 part of board 5): the musicians table, the audio
// input and the engineer's named sessions. The container joins the console app to the three
// organisms: it renders what network-view.js and sessions.js work out, keeps the screen's own
// choices (an ASIO driver not started yet, a device switch waiting for confirmation, the form's
// error) and turns events into engine calls (the Session panel's through sessionPanelCommands).
// The audio setup also chooses where the engineer's local mix plays ("Local mix out", see
// local-mix.js): the outputs are listed again for every engine run, since they depend on its
// driver. Picking a musician in the table opens their link health (link-health-controller.js): the
// panel follows their history once a second while it is open, with the range and the table its own.
// Each row's Buffer sets that phone's playback buffer from the desk, Auto or Fixed
// (playback-buffer.js), and says on the row what came of it.

import "../components/audio-setup.js";
import "../components/link-health.js";
import "../components/musicians-table.js";
import "../components/session-panel.js";
import { createLinkHealthController } from "../lib/link-health-controller.js";
import { followOutputs, localOutputChoice, localOutputView } from "../lib/local-mix.js";
import {
  BUFFER_OPTIONS,
  HOST_OPTIONS,
  audioSetupView,
  deviceSwitchPlan,
  musicianRows,
  validateAddress,
} from "../lib/network-view.js";
import { mixBackupsView } from "../lib/mix-backups-view.js";
import { mixMessages } from "../lib/mix-view.js";
import { bufferRequest } from "../lib/playback-buffer.js";
import { sessionPanelCommands, sessionPanelView } from "../lib/sessions.js";

const LAYOUT = `
  <header class="screen-header">
    <h1 tabindex="-1">Network and system</h1>
    <p>Each phone's link and name, the audio input the mixes come from, and the saved sessions.</p>
  </header>
  <p class="network-note" data-note hidden>
    The engine is not running, so no phone is listed. Start it from the <a href="#/mix">Mix</a>
    screen, or choose a driver below.
  </p>
  <p class="mix-message mix-message--error" role="alert" data-command hidden></p>
  <p class="mix-message" role="status" data-audio hidden></p>
  <div class="network-body">
    <section class="network-musicians" aria-label="Musicians">
      <ml-musicians-table></ml-musicians-table>
      <ml-link-health hidden></ml-link-health>
    </section>
    <aside class="network-side" aria-label="Audio input and session">
      <ml-audio-setup></ml-audio-setup>
      <ml-session-panel></ml-session-panel>
    </aside>
  </div>
`;

/** Mounts the Network and system screen in `main` for the console `app`; returns the unmount. */
export function mountNetworkScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "network-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const parts = {
    note: screen.querySelector("[data-note]"),
    command: screen.querySelector("[data-command]"),
    audioMessage: screen.querySelector("[data-audio]"),
    table: screen.querySelector("ml-musicians-table"),
    health: screen.querySelector("ml-link-health"),
    audio: screen.querySelector("ml-audio-setup"),
    session: screen.querySelector("ml-session-panel"),
  };
  // The screen's own choices, not the engine's: an ASIO driver being picked, a device switch
  // waiting for confirmation, and the errors of the last musician command.
  const local = {
    draftHost: null,
    draftDevice: null,
    pendingSwitch: null,
    addError: null,
    tableError: null,
    // The last change of the local mix's output, `{ ok, message }`, said until the next one.
    outputSaid: null,
    // The last change of a musician's playback buffer, `{ address, ok, message }`, said on that
    // row until the next one.
    bufferSaid: null,
  };
  // The open musician's link health: which one, its range and table, and the follower.
  const health = createLinkHealthController({
    read: (key, sinceSeq) => app.linkHistory(key, sinceSeq),
    onChange: () => scheduleRender(),
  });

  function render() {
    const state = app.store.getState();
    parts.note.hidden = state.engine.status !== null;
    const { command, audio } = mixMessages(state);
    parts.command.hidden = command === null;
    parts.command.textContent = command ?? "";
    parts.audioMessage.hidden = audio === null;
    parts.audioMessage.textContent = audio ?? "";
    // Closed when its musician is gone from the list (removed, or the engine stopped).
    const { selected, data } = health.view(state.musicians);
    parts.table.data = {
      rows: musicianRows(state),
      addError: local.addError,
      tableError: local.tableError,
      selected,
      bufferSaid: local.bufferSaid,
    };
    parts.health.data = data;
    parts.audio.data = {
      ...audioSetupView(state, local),
      hosts: HOST_OPTIONS,
      buffers: BUFFER_OPTIONS,
      devicesError: state.devices.error ? `Devices could not be listed: ${state.devices.error}` : null,
      localOutput: { ...localOutputView(state), said: local.outputSaid },
    };
    parts.session.data = {
      ...sessionPanelView(state),
      backups: mixBackupsView(state),
      unsaved: app.unsavedEdits,
    };
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

  // Applies the screen's own changes and draws at once; also used after an awaited command, so
  // the engineer's own action never waits for the next frame.
  function update(changes = {}) {
    Object.assign(local, changes);
    render();
  }

  async function onHost({ detail: { host } }) {
    const applied = app.store.getState().audio.host;
    if (host === "asio" && applied !== "asio") {
      // Never started on its own: list the drivers and wait for one to be picked.
      update({ draftHost: "asio", draftDevice: null, pendingSwitch: null });
      await app.loadDevices("asio");
      render();
      return;
    }
    update({ draftHost: null, draftDevice: null, pendingSwitch: null });
    if (host !== applied) {
      await app.applyAudio({ host });
    }
    await app.loadDevices(host);
    render();
  }

  async function onDevice({ detail: { name } }) {
    if (local.draftHost === "asio") {
      update({ draftDevice: name || null });
      return;
    }
    const state = app.store.getState();
    const { listing } = audioSetupView(state, local);
    const plan = deviceSwitchPlan(state, listing ? [] : state.devices.list, name);
    if (plan.kind === "confirm") {
      update({ pendingSwitch: plan });
    } else if (plan.kind === "switch") {
      update({ pendingSwitch: null });
      await app.switchDevice(plan.name);
      render();
    } else {
      render();
    }
  }

  async function onConfirmSwitch() {
    const plan = local.pendingSwitch;
    update({ pendingSwitch: null });
    if (plan) {
      await app.switchDevice(plan.name);
      render();
    }
  }

  async function onApplyAsio() {
    const result = await app.applyAudio({ host: "asio" }, { deviceFilter: local.draftDevice });
    if (result.ok) {
      update({ draftHost: null, draftDevice: null });
      await app.loadDevices("asio");
    }
    render();
  }

  async function onBuffer({ detail: { frames } }) {
    const state = app.store.getState();
    if (local.draftHost === "asio" || state.engine.status === null) {
      // Kept for the next start; nothing runs that it could change now.
      app.setAudio({ bufferFrames: frames });
      render();
      return;
    }
    await app.applyAudio({ bufferFrames: frames }, { keepDevice: true });
    render();
  }

  async function onAdd({ detail }) {
    const checked = validateAddress(detail.address);
    if (!checked.ok) {
      update({ addError: checked.message });
      return;
    }
    const result = await app.addTarget(checked.address);
    if (result.ok) {
      parts.table.clearAddress();
      update({ addError: null });
    } else {
      update({ addError: `Could not add ${checked.address}: ${result.message}` });
    }
  }

  function onSelect({ detail: { address } }) {
    health.select(app.store.getState().musicians, address);
    render();
  }

  async function onRemove({ detail: { address } }) {
    const result = await app.removeTarget(address);
    update({ tableError: result.ok ? null : `Could not remove ${address}: ${result.message}` });
  }

  // A Buffer chosen on a row: checked here (a fixed length in whole ms within range), then sent;
  // what came of it is said on the row.
  async function onMusicianBuffer({ detail: { address, key, mode, ms } }) {
    const request = bufferRequest(mode, ms);
    if (!request.ok) {
      update({ bufferSaid: { address, ok: false, message: request.message } });
      return;
    }
    const result = await app.setMusicianBuffer(key, request.mode, request.ms);
    update({ bufferSaid: { address, ok: result.ok, message: result.message } });
  }

  async function onRename({ detail: { address, name } }) {
    const result = await app.rename(address, name);
    update({ tableError: result.ok ? null : `Could not save the name: ${result.message}` });
  }

  // A choice of the local mix's output goes to the console, which sends one at a time; only the
  // last choice's outcome is said (localOutputChoice).
  const chooseOutput = localOutputChoice({
    app,
    said: (outcome) => update({ outputSaid: outcome }),
  });

  // The outputs depend on the driver an engine runs, so each run lists them again.
  const followRun = followOutputs(() => app.loadOutputs());

  screen.addEventListener("audio-host", onHost);
  screen.addEventListener("audio-device", onDevice);
  screen.addEventListener("audio-switch-confirm", onConfirmSwitch);
  screen.addEventListener("audio-switch-cancel", () => update({ pendingSwitch: null }));
  screen.addEventListener("audio-asio-apply", onApplyAsio);
  screen.addEventListener("audio-buffer", onBuffer);
  screen.addEventListener("musician-add", onAdd);
  screen.addEventListener("musician-remove", onRemove);
  screen.addEventListener("musician-rename", onRename);
  screen.addEventListener("musician-select", onSelect);
  screen.addEventListener("musician-buffer", onMusicianBuffer);
  screen.addEventListener("link-range", ({ detail: { rangeMs } }) => {
    health.setRange(rangeMs);
    render();
  });
  screen.addEventListener("link-table", ({ detail: { shown } }) => {
    health.showTable(shown);
    render();
  });
  screen.addEventListener("link-close", () => {
    health.close();
    render();
  });
  screen.addEventListener("local-output", ({ detail: { value } }) => chooseOutput(value));
  // The Session panel's commands: each outcome is said in the panel at once, with the store's data.
  const sessions = sessionPanelCommands({
    app,
    panel: parts.session,
    said: (outcome) => {
      parts.session.say(outcome);
      render();
    },
  });
  for (const type of sessions.types) {
    screen.addEventListener(type, ({ detail }) => {
      sessions.handle(type, detail).catch((error) =>
        parts.session.say({ ok: false, message: String(error?.message ?? error) }),
      );
    });
  }
  const unsubscribe = app.store.subscribe((state) => {
    followRun(state);
    scheduleRender();
  });
  render();
  app.loadDevices(app.store.getState().audio.host);
  // The backups are taken as the musicians' mixes change: the screen lists them afresh each time
  // it opens (and on Refresh).
  app.loadMixBackups();
  followRun(app.store.getState());

  return () => {
    unsubscribe();
    health.dispose();
    cancelAnimationFrame(frame);
    screen.remove();
  };
}
