// The Channels and groups screen (board 3 of the design): the container that joins the console app
// to the channel table and the group cards. It renders what channelsScreenCommands
// (channels-view.js) works out, at most once per animation frame, routes the input meters straight
// to the table, and hands the organisms' events to channelsScreenCommands, which turns them into
// channel and group edits, holds a clicked box as clicked until its edit is answered, and keeps the
// outcome to be shown in words.

import "../components/channel-table.js";
import "../components/group-cards.js";
import { processingActive } from "../lib/console-state.js";
import { KIND_OPTIONS, channelsScreenCommands } from "../lib/channels-view.js";

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
  // The edits, the boxes held as clicked and the last outcome; the screen draws at once whenever
  // they change, so the engineer's own action never waits for the next frame.
  const commands = channelsScreenCommands({ app, changed: () => render() });

  function render() {
    const view = commands.view();
    parts.note.hidden = view.known;
    parts.body.hidden = !view.known;
    parts.outcome.textContent = view.outcome?.ok ? view.outcome.message : "";
    parts.failure.textContent = view.outcome && !view.outcome.ok ? view.outcome.message : "";
    if (!view.known) {
      return;
    }
    const state = app.store.getState();
    parts.table.data = {
      // A pair's strip is kept on its first side.
      rows: view.rows.map((row) => ({
        ...row,
        processing: processingActive(state, row.stereo.first + 1),
      })),
      kinds: KIND_OPTIONS,
    };
    parts.groups.data = { cards: view.cards, canAdd: view.canAdd };
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

  for (const type of commands.types) {
    screen.addEventListener(type, ({ detail }) => commands.handle(type, detail));
  }

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
