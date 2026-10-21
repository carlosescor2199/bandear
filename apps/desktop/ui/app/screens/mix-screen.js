// The Mix screen (board 1 of the design): the container that joins the console app (store and
// engine) to the presentational organisms. It renders what mix-view.js works out, at most once
// per animation frame, routes the meters straight to the strips, and turns the organisms' events
// into engine calls and mix edits. The tools beside the musician tabs (Mirror, Copy mix to…,
// Presets) act on the open musician's mix, and their last outcome is said in words above the
// strips. The last tab is the engineer's Local mix (local-mix.js): the same strips without a lock,
// a cap or a limit, and no tools, metered by the `meters` event's `local` peak.

import "../components/index.js";
import { editKey } from "../lib/mix.js";
import { mixToolsView } from "../lib/mixes.js";
import { mixToolCommands } from "../lib/mix-tools.js";
import { musicianLabel } from "../lib/format.js";
import { LOCAL_MIX, processingActive, selectedMusician } from "../lib/console-state.js";
import {
  channelEdits,
  channelStrips,
  groupStrips,
  masterLevels,
  masterStrip,
  mixMessages,
  mixNotice,
  mixTabs,
  stripLevel,
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
  <div class="mix-tabs">
    <ml-musician-tabs></ml-musician-tabs>
    <ml-mix-tools hidden></ml-mix-tools>
  </div>
  <p class="mix-message" role="status" data-tools hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-tools-error hidden></p>
  <p class="mix-message" role="status" data-departed hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-error hidden></p>
  <p class="mix-message" role="status" data-controls hidden></p>
  <p class="mix-message mix-message--error" role="alert" data-command hidden></p>
  <p class="mix-message" role="status" data-audio hidden></p>
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

// Keeps one child of `container` per item, in the items' order, each kept by its key (made by
// `create` for a new key) and returned. A child stays with its key while others come and go, so a
// group strip stays with its group (by id) and a channel strip with its input when groups are
// reordered or an input is hidden or paired, even in the middle of a drag.
function keyedChildren(container, items, keyOf, create) {
  const byKey = new Map([...container.children].map((child) => [child.dataset.key, child]));
  const children = items.map((item) => {
    const key = String(keyOf(item));
    let child = byKey.get(key);
    if (child) {
      byKey.delete(key);
    } else {
      child = create();
      child.dataset.key = key;
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

/** Mounts the Mix screen in `main` for the console `app`; returns the unmount function. */
export function mountMixScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "mix-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const parts = {
    topBar: screen.querySelector("ml-top-bar"),
    tabs: screen.querySelector("ml-musician-tabs"),
    tools: screen.querySelector("ml-mix-tools"),
    toolsDone: screen.querySelector("[data-tools]"),
    toolsError: screen.querySelector("[data-tools-error]"),
    departed: screen.querySelector("[data-departed]"),
    error: screen.querySelector("[data-error]"),
    controls: screen.querySelector("[data-controls]"),
    command: screen.querySelector("[data-command]"),
    audio: screen.querySelector("[data-audio]"),
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

  // The last outcome of the tools, `{ ok, message }`, said until the next one.
  let toolsOutcome = null;

  function renderTools(state) {
    parts.tools.hidden = selectedMusician(state) === null;
    if (!parts.tools.hidden) {
      parts.tools.data = { ...mixToolsView(state, state.selected), address: state.selected };
    }
    parts.toolsDone.hidden = !toolsOutcome?.ok;
    parts.toolsError.hidden = !toolsOutcome || toolsOutcome.ok;
    parts.toolsDone.textContent = toolsOutcome?.ok ? toolsOutcome.message : "";
    parts.toolsError.textContent = toolsOutcome && !toolsOutcome.ok ? toolsOutcome.message : "";
  }

  function render() {
    const state = app.store.getState();
    const address = state.selected;
    parts.topBar.data = topBarView(state);
    parts.tabs.data = mixTabs(state);
    renderTools(state);

    const messages = mixMessages(state);
    for (const name of ["departed", "error", "controls", "command", "audio"]) {
      parts[name].hidden = messages[name] === null;
      parts[name].textContent = messages[name] ?? "";
    }

    const notice = mixNotice(state);
    renderNotice(notice);
    const local = address === LOCAL_MIX && state.localMix !== null;
    const musician = selectedMusician(state);
    parts.body.hidden = notice !== null || (musician === null && !local);
    if (parts.body.hidden) {
      return;
    }
    parts.body.setAttribute("aria-label", local ? "Local mix" : `${musicianLabel(musician)}'s mix`);
    // Each strip links to its input's processing editor and says whether it is on.
    const strips = channelStrips(state, address).map((strip) => ({
      ...strip,
      processing: processingActive(state, strip.sides[0] + 1),
    }));
    keyedChildren(
      parts.channels,
      strips,
      (strip) => strip.index,
      () => document.createElement("ml-channel-strip"),
    ).forEach((element, position) => {
      element.data = strips[position];
    });
    const groups = groupStrips(state, address);
    keyedChildren(
      parts.groups,
      groups,
      (group) => group.id,
      () => document.createElement("ml-group-strip"),
    ).forEach((element, position) => {
      element.data = groups[position];
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

  function onMeters({ channels, musicians, local }) {
    if (parts.body.hidden) {
      return;
    }
    for (const strip of parts.channels.children) {
      if (strip.data) {
        strip.setLevel(stripLevel(strip.data.sides, channels));
      }
    }
    const { left, right } = masterLevels({ musicians, local }, app.store.getState().selected);
    parts.master.setLevels(left, right);
  }

  // A channel strip's move goes to every input it stands for (both sides of a pair, its balance
  // as two pans, its lock); a group or master move is one edit. The console app sends the
  // engineer's limit and locks with their own command (see console-app.js).
  function onMixEdit(event) {
    const address = app.store.getState().selected;
    const { field, index, sides, value } = event.detail;
    if (address) {
      app.edit(
        address,
        sides ? channelEdits(event.detail, field, value) : { [editKey(field, index)]: value },
      );
    }
  }

  // The tools' commands for the open musician, each answer said above the strips and handed back
  // to the tools (mixToolCommands, tested without a browser). The tools take the store's data at
  // once, before they hear the answer, so the Mirror control never shows the old mirror between
  // the answer and the next frame.
  const toolCommands = mixToolCommands({
    app,
    tools: parts.tools,
    said(outcome) {
      toolsOutcome = outcome;
      renderTools(app.store.getState());
      scheduleRender();
    },
  });
  for (const type of toolCommands.types) {
    screen.addEventListener(type, (event) => toolCommands.handle(type, event.detail));
  }
  screen.addEventListener("presets-load", () => app.loadPresets());
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
