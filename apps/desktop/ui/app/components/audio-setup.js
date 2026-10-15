// <ml-audio-setup>: the audio input: driver, input device, buffer and what the engine reports.
//
// Property: `data`, from audioSetupView (network-view.js) plus `{ hosts, buffers, devicesError }`.
// Events (bubbling, composed): `audio-host` { host }, `audio-device` { name }, `audio-buffer`
//   { frames }, `audio-switch-confirm` and `audio-switch-cancel` (the warning before a switch
//   that leaves groups without inputs), `audio-asio-apply` (start on the ASIO driver picked).
//
// While an ASIO choice waits for its driver the warning about kernel-mode drivers shows, the
// device list offers the ASIO drivers with nothing picked, and "Start on this driver" stays off
// until one is.

import { attachShadowRoot, define, emit, reflect, setText, sheet } from "./support.js";

const styles = sheet(`
  :host {
    display: block;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-xl);
    background: var(--ml-surface);
  }
  h2 {
    margin: 0;
    font-size: var(--ml-text-lg);
  }
  .grid {
    display: grid;
    grid-template-columns: 110px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
  }
  .full {
    grid-column: 1 / -1;
  }
  label,
  dt {
    color: var(--ml-text-secondary);
    font-size: var(--ml-text-md);
  }
  select,
  button {
    min-height: var(--ml-target);
    font: inherit;
    font-size: var(--ml-text-md);
    color: inherit;
  }
  select {
    min-width: 0;
    padding: 0 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-sm);
    background: var(--ml-ground);
  }
  button {
    padding: 0 12px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-surface-raised);
    font-weight: var(--ml-weight-bold);
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .primary {
    min-height: var(--ml-target-primary);
    border-color: var(--ml-accent);
    background: var(--ml-accent);
    color: var(--ml-text-on-bright);
  }
  :focus-visible {
    outline: 2px solid var(--ml-focus);
    outline-offset: 2px;
  }
  .caution,
  .warning {
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--ml-warn-border);
    border-radius: var(--ml-radius-md);
    background: var(--ml-warn-bg);
    font-size: var(--ml-text-md);
  }
  .warning {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .error {
    margin: 0;
    color: var(--ml-danger-text);
    font-size: var(--ml-text-md);
  }
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 12px;
    margin: 0;
  }
  dd {
    margin: 0;
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-md);
  }
  .hint {
    margin: 0;
    color: var(--ml-text-muted);
    font-size: var(--ml-text-sm);
  }
  [hidden] {
    display: none !important;
  }
`);

class MlAudioSetup extends HTMLElement {
  #data = null;
  #parts;

  constructor() {
    super();
    const root = attachShadowRoot(
      this,
      styles,
      `<section aria-labelledby="audio-title">
        <h2 id="audio-title">Audio input</h2>
        <div class="grid">
          <label for="host">Driver</label>
          <select id="host"></select>
          <p class="caution full" data-asio hidden>
            <strong>ASIO loads a kernel-mode driver</strong>: an unstable one can restart the whole
            machine, so it is never chosen for you. Pick the driver deliberately, preferring your
            interface's own over a built-in one; nothing starts until you press Start on this
            driver.
          </p>
          <label for="device">Input device</label>
          <select id="device"></select>
          <p class="error full" role="alert" data-devices-error hidden></p>
          <div class="warning full" role="alert" data-warning hidden>
            <span data-warning-text></span>
            <button type="button" data-confirm>Switch anyway</button>
            <button type="button" data-cancel>Cancel</button>
          </div>
          <span class="full" data-apply-row hidden>
            <button type="button" class="primary" data-apply>Start on this driver</button>
          </span>
          <label for="buffer">Buffer</label>
          <select id="buffer"></select>
        </div>
        <dl>
          <dt>Capture format</dt><dd data-format></dd>
          <dt>Sample rate</dt><dd data-rate></dd>
          <dt>Control port</dt><dd data-port></dd>
          <dt>Samples received</dt><dd data-samples></dd>
          <dt>Packets sent / discarded</dt><dd data-packets></dd>
        </dl>
        <p class="hint">
          The driver, the buffer and, outside WASAPI, the device restart the capture engine, the
          way a DAW does when the driver changes: the phones reconnect on their own. The buffer
          only takes effect on ASIO, where the driver honours it; WASAPI shared mode pins the
          period to the device's own (10 ms). The sample rate is 48 kHz by design: it is the
          format on the wire the phones receive, not a free choice.
        </p>
      </section>`,
    );
    const $ = (selector) => root.querySelector(selector);
    this.#parts = {
      host: $("#host"),
      device: $("#device"),
      buffer: $("#buffer"),
      asio: $("[data-asio]"),
      devicesError: $("[data-devices-error]"),
      warning: $("[data-warning]"),
      warningText: $("[data-warning-text]"),
      applyRow: $("[data-apply-row]"),
      apply: $("[data-apply]"),
      format: $("[data-format]"),
      rate: $("[data-rate]"),
      port: $("[data-port]"),
      samples: $("[data-samples]"),
      packets: $("[data-packets]"),
    };
    const parts = this.#parts;
    parts.host.addEventListener("change", () => emit(this, "audio-host", { host: parts.host.value }));
    parts.device.addEventListener("change", () =>
      emit(this, "audio-device", { name: parts.device.value }),
    );
    parts.buffer.addEventListener("change", () =>
      emit(this, "audio-buffer", { frames: Number(parts.buffer.value) }),
    );
    $("[data-confirm]").addEventListener("click", () => emit(this, "audio-switch-confirm", {}));
    $("[data-cancel]").addEventListener("click", () => emit(this, "audio-switch-cancel", {}));
    parts.apply.addEventListener("click", () => emit(this, "audio-asio-apply", {}));
  }

  get data() {
    return this.#data;
  }

  set data(data) {
    this.#data = data;
    const parts = this.#parts;
    fill(parts.host, data.hosts, data.host);
    parts.asio.hidden = !data.asioPending;
    const choices = data.asioPending
      ? [{ value: "", label: "Choose an ASIO driver…" }, ...data.devices]
      : data.devices;
    let empty = "no input devices";
    if (data.listing) {
      empty = "Listing the devices…";
    }
    fill(parts.device, data.devices.length > 0 ? choices : [{ value: "", label: empty }], data.device);
    parts.device.disabled = data.deviceDisabled;
    parts.devicesError.hidden = !data.devicesError;
    setText(parts.devicesError, data.devicesError ?? "");
    parts.warning.hidden = data.warning === null;
    setText(parts.warningText, data.warning ?? "");
    parts.applyRow.hidden = !data.asioPending;
    parts.apply.disabled = !data.canApplyAsio;
    fill(parts.buffer, data.buffers, data.buffer);
    setText(parts.format, data.readouts.format);
    setText(parts.rate, data.readouts.sampleRate);
    setText(parts.port, data.readouts.controlPort);
    setText(parts.samples, data.readouts.samplesReceived);
    setText(parts.packets, data.readouts.packets);
  }
}

// Fills a select with `options` (rebuilt only when they change) and selects `value`, so a list
// the user is reading is not torn down four times a second.
function fill(select, options, value) {
  const key = options.map((option) => `${option.value}\u0000${option.label}`).join("\u0001");
  if (select.dataset.options !== key) {
    select.dataset.options = key;
    select.replaceChildren(
      ...options.map((option) => {
        const element = document.createElement("option");
        element.value = String(option.value);
        element.textContent = option.label;
        return element;
      }),
    );
  }
  const wanted = String(value ?? "");
  if (select.value !== wanted) {
    select.value = wanted;
  }
  reflect(select, "data-value", wanted);
}

define("ml-audio-setup", MlAudioSetup);
