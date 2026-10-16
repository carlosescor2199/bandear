// The Channels and groups screen (board 3 of the design): the container that joins the console app
// to the channel table and the group cards. It renders what channels-view.js works out, at most
// once per animation frame, routes the input meters straight to the table, and turns the
// organisms' events into channel and group edits, each shown with its outcome in words.

import "../components/channel-table.js";
import "../components/group-cards.js";
import {
  KIND_OPTIONS,
  addGroup,
  canAddGroup,
  channelRows,
  deleteGroup,
  groupCards,
  memberKey,
  pendingToggles,
  renameChannel,
  renameGroup,
  setChannelHidden,
  setChannelKind,
  setChannelLink,
  setGroupMembers,
  shownKey,
  withPendingMembers,
  withPendingShown,
} from "../lib/channels-view.js";

const LAYOUT = `
  <header class="screen-header">
    <h1 tabindex="-1">Channels and groups</h1>
    <p>Names, instruments and groups reach every phone at once. A hidden input is shown nowhere and no one hears it.</p>
  </header>
  <p class="channels-note" data-note hidden>
    The engine is not running, so this desk does not know its inputs and groups yet. Start it from
    the <a href="#/mix">Mix</a> screen.
  </p>
  <div class="channels-messages">
    <p class="channels-message" role="status" data-outcome></p>
    <p class="channels-message channels-message--error" role="alert" data-failure></p>
  </div>
  <div class="channels-body" data-body>
    <section class="channels-table" aria-label="Channels">
      <ml-channel-table></ml-channel-table>
    </section>
    <aside class="channels-groups" aria-label="Groups">
      <ml-group-cards></ml-group-cards>
    </aside>
  </div>
`;

/** Mounts the Channels and groups screen in `main` for the console `app`; returns the unmount. */
export function mountChannelsScreen(main, app) {
  const screen = document.createElement("div");
  screen.className = "channels-screen";
  screen.innerHTML = LAYOUT;
  main.append(screen);
  const parts = {
    note: screen.querySelector("[data-note]"),
    outcome: screen.querySelector("[data-outcome]"),
    failure: screen.querySelector("[data-failure]"),
    body: screen.querySelector("[data-body]"),
    table: screen.querySelector("ml-channel-table"),
    groups: screen.querySelector("ml-group-cards"),
  };
  // The answer to the engineer's last edit: `{ ok, message }`, or null before the first.
  let lastResult = null;
  // The Shown and group boxes clicked whose edits are not answered yet: they show as clicked.
  const pending = pendingToggles();

  function render() {
    const state = app.store.getState();
    const known = state.engine.status !== null;
    parts.note.hidden = known;
    parts.body.hidden = !known;
    parts.outcome.textContent = lastResult?.ok ? lastResult.message : "";
    parts.failure.textContent = lastResult && !lastResult.ok ? lastResult.message : "";
    if (!known) {
      return;
    }
    parts.table.data = {
      rows: withPendingShown(channelRows(state), pending),
      kinds: KIND_OPTIONS,
    };
    parts.groups.data = {
      cards: withPendingMembers(groupCards(state), pending),
      canAdd: canAddGroup(state),
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

  // The device's input count, which every channel edit fills the list up to.
  function inputs() {
    return app.store.getState().engine.status?.sourceChannels ?? 0;
  }

  // Sends one edit and shows its outcome at once; the status it brings is in the store by then.
  // A toggle's edit holds the box as clicked (`toggle`: `{ key, value }`) until it is answered.
  async function edit(send, toggle = null) {
    const token = toggle ? pending.hold(toggle.key, toggle.value) : null;
    if (toggle) {
      render();
    }
    lastResult = await send();
    if (toggle) {
      pending.release(toggle.key, token);
    }
    render();
  }

  function editChannels(change, toggle = null) {
    const count = inputs();
    return edit(() => app.editChannels((table) => change(table, count)), toggle);
  }

  screen.addEventListener("channel-rename", ({ detail: { index, name } }) =>
    editChannels((table, count) => renameChannel(table, index, name, count)),
  );
  screen.addEventListener("channel-kind", ({ detail: { index, kind } }) =>
    editChannels((table, count) => setChannelKind(table, index, kind, count)),
  );
  screen.addEventListener("channel-link", ({ detail: { index, linked } }) =>
    editChannels((table, count) => setChannelLink(table, index, linked, count)),
  );
  screen.addEventListener("channel-shown", ({ detail: { index, shown } }) =>
    editChannels((table, count) => setChannelHidden(table, index, !shown, count), {
      key: shownKey(index),
      value: shown,
    }),
  );
  screen.addEventListener("group-add", () => edit(() => app.editGroups(addGroup)));
  screen.addEventListener("group-rename", ({ detail: { id, name } }) =>
    edit(() => app.editGroups((groups) => renameGroup(groups, id, name))),
  );
  screen.addEventListener("group-member", ({ detail: { id, key, numbers, member } }) =>
    edit(() => app.editGroups((groups) => setGroupMembers(groups, id, numbers, member)), {
      key: memberKey(id, key),
      value: member,
    }),
  );
  screen.addEventListener("group-delete", ({ detail: { id } }) =>
    edit(() => app.editGroups((groups) => deleteGroup(groups, id))),
  );

  const unsubscribe = app.store.subscribe(scheduleRender);
  const stopMeters = app.onMeters(({ channels }) => {
    if (!parts.body.hidden) {
      parts.table.setLevels(channels);
    }
  });
  render();

  return () => {
    unsubscribe();
    stopMeters();
    cancelAnimationFrame(frame);
    screen.remove();
  };
}
