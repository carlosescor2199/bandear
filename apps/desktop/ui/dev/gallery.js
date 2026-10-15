// The component gallery: renders every atom in its states and drives the live ones from the fake
// engine (no Tauri). Serve apps/desktop/ui with dev/serve.mjs and open /dev/gallery.html.

import "../app/components/index.js";
import { createEngineApi } from "../app/lib/bridge.js";
import { createFakeBridge } from "../app/lib/fake-bridge.js";
import { formatDb, formatPercent, linkState, musicianLabel } from "../app/lib/format.js";
import { levelToDb } from "../app/lib/values.js";

const LOG_LENGTH = 12;
const NO_REPORT = {
  packetsPerSecond: null,
  underrunsPerSecond: null,
  lossPercent: null,
  receiveTimeoutsPerSecond: null,
};

const fake = createFakeBridge({ running: true });
const api = createEngineApi(fake);

// Live meters: the fake engine's `meters` event, about 30 a second, straight into the elements.
const inputMeters = [...document.querySelectorAll("ml-meter[data-channel]")];
const outputMeters = [...document.querySelectorAll("ml-meter[data-output]")];
let paused = false;
api.onMeters(({ channels, musicians }) => {
  if (paused) {
    return;
  }
  for (const meter of inputMeters) {
    meter.setLevel(channels[Number(meter.dataset.channel)] ?? 0);
  }
  for (const meter of outputMeters) {
    meter.setLevel(musicians[meter.dataset.output]?.[meter.dataset.side] ?? 0);
  }
});
fake.startMeters();

const pause = document.querySelector("[data-pause-meters]");
pause.addEventListener("click", () => {
  paused = !paused;
  pause.setAttribute("aria-pressed", String(paused));
  pause.textContent = paused ? "Resume meters" : "Pause meters";
});

for (const figure of document.querySelectorAll("[data-zone-specimen]")) {
  const level = Number(figure.dataset.zoneSpecimen);
  figure.querySelector("figcaption").textContent = formatDb(levelToDb(level));
}

// Link states: the fake engine's musicians, then the cases they do not cover.
const examples = [
  ...(await api.status()).musicians.map((musician) => ({
    caption: musicianLabel(musician),
    musician,
  })),
  {
    caption: "Heavy loss",
    musician: {
      linkHealth: "bad",
      controlConnected: true,
      packetsPerSecond: 150,
      underrunsPerSecond: 4,
      lossPercent: 6.5,
      receiveTimeoutsPerSecond: 1,
    },
  },
  {
    caption: "Hears nothing",
    musician: {
      linkHealth: "bad",
      controlConnected: true,
      packetsPerSecond: 0,
      underrunsPerSecond: 0,
      lossPercent: 0,
      receiveTimeoutsPerSecond: 5,
    },
  },
  {
    caption: "Just added",
    musician: { linkHealth: "unknown", controlConnected: true, ...NO_REPORT },
  },
  {
    caption: "App closed",
    musician: { linkHealth: "unknown", controlConnected: false, ...NO_REPORT },
  },
];
const linkRow = document.querySelector("[data-link-states]");
for (const { caption, musician } of examples) {
  const { level, label, detail } = linkState(musician);
  const figure = document.createElement("figure");
  const state = document.createElement("ml-link-state");
  state.setAttribute("level", level);
  state.setAttribute("label", label);
  state.setAttribute("detail", detail);
  const text = document.createElement("figcaption");
  text.textContent = caption;
  figure.append(state, text);
  linkRow.append(figure);
}

// The event log, and the fader readouts the strips will show.
const log = document.querySelector("[data-event-log]");
function record(event) {
  const element = event.target;
  if (!element.localName?.startsWith("ml-")) {
    return;
  }
  if (element.localName === "ml-fader") {
    const readout = element.closest("figure")?.querySelector(".readout");
    if (readout && !readout.hasAttribute("data-muted")) {
      readout.textContent = formatPercent(event.detail.value);
    }
  }
  const item = document.createElement("li");
  const name = element.getAttribute("label") ?? element.localName;
  item.textContent = `${name}: ${event.type} ${JSON.stringify(event.detail)}`;
  log.prepend(item);
  while (log.children.length > LOG_LENGTH) {
    log.lastElementChild.remove();
  }
}
document.addEventListener("input", record);
document.addEventListener("change", record);
