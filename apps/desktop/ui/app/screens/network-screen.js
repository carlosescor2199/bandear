// The Network and system screen (the phase-1 part of board 5): the musicians table and the audio
// input. The container joins the console app to the two organisms: it renders what
// network-view.js works out, keeps the screen's own choices (an ASIO driver not started yet, a
// device switch waiting for confirmation, the form's error) and turns events into engine calls.

import "../components/audio-setup.js";
import "../components/musicians-table.js";
import {
  BUFFER_OPTIONS,
  HOST_OPTIONS,
  audioSetupView,
  deviceSwitchPlan,
  musicianRows,
  validateAddress,
} from "../lib/network-view.js";
import { mixMessages } from "../lib/mix-view.js";

const LAYOUT = `
  <header class="screen-header">
    <h1 tabindex="-1">Network and system</h1>
    <p>Each phone's link and name, and the audio input the mixes come from.</p>
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
    </section>
    <aside class="network-side" aria-label="Audio input">
      <ml-audio-setup></ml-audio-setup>
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
    audio: screen.querySelector("ml-audio-setup"),
  };
  // The screen's own choices, not the engine's: an ASIO driver being picked, a device switch
  // waiting for confirmation, and the errors of the last musician command.
  const local = {
    draftHost: null,
    draftDevice: null,
    pendingSwitch: null,
    addError: null,
    tableError: null,
  };

  function render() {
    const state = app.store.getState();
    parts.note.hidden = state.engine.status !== null;
    const { command, audio } = mixMessages(state);
    parts.command.hidden = command === null;
    parts.command.textContent = command ?? "";
    parts.audioMessage.hidden = audio === null;
    parts.audioMessage.textContent = audio ?? "";
    parts.table.data = {
      rows: musicianRows(state),
      addError: local.addError,
      tableError: local.tableError,
    };
    parts.audio.data = {
      ...audioSetupView(state, local),
      hosts: HOST_OPTIONS,
      buffers: BUFFER_OPTIONS,
      devicesError: state.devices.error ? `Devices could not be listed: ${state.devices.error}` : null,
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

  async function onRemove({ detail: { address } }) {
    const result = await app.removeTarget(address);
    update({ tableError: result.ok ? null : `Could not remove ${address}: ${result.message}` });
  }

  async function onRename({ detail: { address, name } }) {
    const result = await app.rename(address, name);
    update({ tableError: result.ok ? null : `Could not save the name: ${result.message}` });
  }

  screen.addEventListener("audio-host", onHost);
  screen.addEventListener("audio-device", onDevice);
  screen.addEventListener("audio-switch-confirm", onConfirmSwitch);
  screen.addEventListener("audio-switch-cancel", () => update({ pendingSwitch: null }));
  screen.addEventListener("audio-asio-apply", onApplyAsio);
  screen.addEventListener("audio-buffer", onBuffer);
  screen.addEventListener("musician-add", onAdd);
  screen.addEventListener("musician-remove", onRemove);
  screen.addEventListener("musician-rename", onRename);
  const unsubscribe = app.store.subscribe(scheduleRender);
  render();
  app.loadDevices(app.store.getState().audio.host);

  return () => {
    unsubscribe();
    cancelAnimationFrame(frame);
    screen.remove();
  };
}
