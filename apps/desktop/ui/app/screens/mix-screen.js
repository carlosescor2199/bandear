// The Mix screen (board 1 of the design): the container that joins the console app (store and
// engine) to the presentational organisms. It renders what mix-view.js works out, at most once
// per animation frame, routes the meters straight to the strips, and turns the organisms' events
// into engine calls and mix edits.

import "../components/index.js";
import { editKey } from "../lib/mix.js";
import { musicianLabel } from "../lib/format.js";
import { selectedMusician } from "../lib/console-state.js";
import {
  channelStrips,
  groupStrips,
  masterStrip,
  mixMessages,
  mixNotice,
  musicianTabs,
  topBarView,
} from "../lib/mix-view.js";

const NOTICES = Object.freeze({
  connecting: { title: "Connecting to the engine…", text: "" },
  disconnected: { title: "The console cannot reach the engine", action: "Try again" },
  stopped: {
    title: "The engine is stopped",
    text: "Start it to send each musician their mix.",
    action: "Start engine",
  },
  starting: { title: "Starting the engine…", text: "Opening the audio input." },
  stopping: { title: "Stopping the engine…", text: "Closing the audio input." },
  error: { title: "The engine is not running", action: "Try again" },
  "no-musicians": {
    title: "No phones yet",
    text:
      "Musicians appear here when they open MixLink on their phone and join this computer. " +
      "You can also add a phone by address in ",
    link: { href: "#/network", label: "Network and system" },
  },
});

const LAYOUT = `
  <h1 class="ml-visually-hidden" tabindex="-1">Mix</h1>
  <ml-top-bar></ml-top-bar>
  <div class="mix-tabs"><ml-musician-tabs></ml-musician-tabs></div>
  <p class="mix-message" role="status" data-departed hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-error hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-command hidden></p>
  <div class="mix-notice" data-notice hidden>
    <h2 data-notice-title></h2>
    <p data-notice-text></p>
    <button type="button" class="mix-notice-action" data-notice-action hidden></button>
  </div>
  <div class="mix-body" role="tabpanel" data-body hidden>
    <section class="mix-channels" aria-label="Channels" data-channels></section>
    <section class="mix-side" aria-label="Groups and master">
      <div class="mix-groups" data-groups></div>
      <ml-master-strip></ml-master-strip>
    </section>
  </div>
`;

// Keeps exactly `count` children of `container`, made by `create`, and returns them.
function ensureChildren(container, count, create) {
  while (container.children.length > count) {
    container.lastElementChild.remove();
  }
  while (container.children.length < count) {
    container.append(create());
  }
  return [...container.children];
}

/** Mounts the Mix screen in `main` for the console `app`; returns the unmount function. */
export function mountMixScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "mix-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const parts = {
    topBar: screen.querySelector("ml-top-bar"),
    tabs: screen.querySelector("ml-musician-tabs"),
    departed: screen.querySelector("[data-departed]"),
    error: screen.querySelector("[data-error]"),
    command: screen.querySelector("[data-command]"),
    notice: screen.querySelector("[data-notice]"),
    noticeTitle: screen.querySelector("[data-notice-title]"),
    noticeText: screen.querySelector("[data-notice-text]"),
    noticeAction: screen.querySelector("[data-notice-action]"),
    body: screen.querySelector("[data-body]"),
    channels: screen.querySelector("[data-channels]"),
    groups: screen.querySelector("[data-groups]"),
    master: screen.querySelector("ml-master-strip"),
  };

  function renderNotice(notice) {
    parts.notice.hidden = notice === null;
    if (!notice) {
      return;
    }
    const copy = NOTICES[notice.kind];
    parts.noticeTitle.textContent = copy.title;
    parts.noticeText.textContent = notice.message ?? copy.text;
    parts.noticeText.hidden = parts.noticeText.textContent === "";
    parts.noticeAction.dataset.kind = notice.kind;
    if (copy.link) {
      const link = document.createElement("a");
      link.href = copy.link.href;
      link.textContent = copy.link.label;
      parts.noticeText.append(link, ".");
    }
    parts.noticeAction.hidden = !copy.action;
    parts.noticeAction.textContent = copy.action ?? "";
  }

  function render() {
    const state = app.store.getState();
    const address = state.selected;
    parts.topBar.data = topBarView(state);
    parts.tabs.data = musicianTabs(state);

    const messages = mixMessages(state);
    for (const name of ["departed", "error", "command"]) {
      parts[name].hidden = messages[name] === null;
      parts[name].textContent = messages[name] ?? "";
    }

    const notice = mixNotice(state);
    renderNotice(notice);
    const musician = selectedMusician(state);
    parts.body.hidden = notice !== null || musician === null;
    if (parts.body.hidden) {
      return;
    }
    parts.body.setAttribute("aria-label", `${musicianLabel(musician)}'s mix`);
    const strips = channelStrips(state, address);
    ensureChildren(parts.channels, strips.length, () =>
      document.createElement("ml-channel-strip"),
    ).forEach((element, index) => {
      element.data = strips[index];
    });
    const groups = groupStrips(state, address);
    ensureChildren(parts.groups, groups.length, () =>
      document.createElement("ml-group-strip"),
    ).forEach((element, index) => {
      element.data = groups[index];
    });
    parts.master.data = masterStrip(state, address);
  }

  // Store changes can come many times a frame (a fader drag, a burst of events): draw once.
  let frame = 0;
  function scheduleRender() {
    if (frame === 0) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        render();
      });
    }
  }

  function onMeters({ channels, musicians }) {
    if (parts.body.hidden) {
      return;
    }
    [...parts.channels.children].forEach((strip, index) => {
      strip.setLevel(channels[index] ?? 0);
    });
    const output = musicians[app.store.getState().selected];
    parts.master.setLevels(output?.left ?? 0, output?.right ?? 0);
  }

  function onMixEdit(event) {
    const address = app.store.getState().selected;
    const { field, index, value } = event.detail;
    if (address) {
      app.edit(address, { [editKey(field, index)]: value });
    }
  }

  screen.addEventListener("engine-start", () => app.start());
  screen.addEventListener("engine-stop", () => app.stop());
  screen.addEventListener("musician-select", (event) => app.select(event.detail.address));
  screen.addEventListener("mix-edit", onMixEdit);
  // "Try again" after a lost connection connects again; every other notice's action starts.
  parts.noticeAction.addEventListener("click", () => {
    if (parts.noticeAction.dataset.kind === "disconnected") {
      app.reconnect();
    } else {
      app.start();
    }
  });
  const unsubscribe = app.store.subscribe(scheduleRender);
  const stopMeters = app.onMeters(onMeters);
  render();

  return () => {
    unsubscribe();
    stopMeters();
    cancelAnimationFrame(frame);
    screen.remove();
  };
}
