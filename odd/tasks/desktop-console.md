# Desktop console (redesign phase 1)

## Objective

Turn the desktop app from a status window into the mixing console of the approved redesign: the
mix is the main screen, and the engineer can shape every musician's mix from the computer while
the musician keeps control on the phone.

## Problem and why

Today the desktop only shows status. Only the phone can change a mix: the server has no way to
set one from the desktop (`EngineHandle` has no `set_mix`; `MusicianDto` drops gains, pans,
mutes and solos, `apps/desktop/src-tauri/src/dto.rs:96-98`). The UI polls `engine_status` every
250 ms and never receives engine events. Capture prefers a stereo configuration even when the
interface has more inputs (`apps/server/src/capture.rs:252`), so a 4-input interface shows two
channels. The UI is one 641-line `main.js` of hand-built DOM that cannot grow into five sections.

## Design (approved)

Design canvas "MixLink desktop redesign" (https://claude.ai/artifact/GcjxqS8ME4eBYVSzxjbQtB,
version 9). Phase 1 builds board 1 (Mix) and the parts of board 5 (Network and system) that hold
what the current UI already does:

- **Mix** (main screen): top bar with the engine pill, input device and format, a link to
  musicians that need attention, and Start/Stop; musician tabs with their link state; channel
  strips (group chip, input meter, pan, fader, value, Mute, Solo); group strips; the musician's
  master strip (level, level cap, mute).
- **Network and system** (phase 1 subset): the musicians table (name with rename, address, link
  state, remove, add a phone by address) and the audio setup (driver, device, buffer, device
  switch).

Look: ground `#0d1117`, surfaces `#161b22` and `#1f2630`, borders `#2b3440`, text `#e6edf3`,
secondary text `#9ba8b5`, amber accent `#f5a524`, green `#3fb950`, red `#f85149`, blue `#4cc2ff`;
IBM Plex Sans for text and JetBrains Mono for numbers.

## Program (all phases)

1. Desktop console (this document).
2. Channels, groups and session: names, icons, hide, stereo link, runtime groups, session and mix
   presets (PRD RF7), engineer volume limit (PRD RF12), copy and mirror mixes.
3. Local mix and multitrack recording: real-time-safe float engine with several consumers, local
   output, per-track WAV/RF64 recording, takes, markers, virtual soundcheck.
4. Network and system: health history and chart, phone buffer from the server, packet redundancy,
   log file, demo source, mixes backup.
5. Processing: EQ, gate, compressor, reverb and delay sends, master limiter, presets.

## Design decisions

- **UI: native Web Components and ES modules, no bundler** (approved by the user). Tauri serves
  `apps/desktop/ui` as is. Atomic design: atoms (fader, meter, pan, toggle, chip, pill) are small
  custom elements; organisms (channel strip, musician tabs, master strip) compose them; screens
  are containers that own state and talk to Tauri, components are presentational (attributes in,
  events out).
- Pure JS logic (state reducers, value mapping, formatting) lives in plain ES modules tested with
  `node --test` (Node 24 via fnm).
- The server stays the single source of truth for a mix: the desktop and the phone both send
  partial updates, last writer wins, and the server pushes the resulting mix to the phone when the
  change did not come from that phone.
- The engine must not know about Tauri (`odd/tasks/server-ui-tauri.md:71-74`): the desktop
  subscribes to the engine's event bus and turns events into Tauri events.
- Sections that are not built yet are not shown in the sidebar; each phase adds its own.

## Scope

- Capture every input channel the interface offers (up to the engine's 32-channel limit).
- Engine and desktop: set a musician's mix from the desktop; full mix in the status DTO; push the
  mix to the phone; Android applies a pushed mix.
- Engine events and meters delivered to the UI as Tauri events.
- New UI shell, atoms, Mix screen and the phase-1 Network and system screen; remove the old UI.

## Out of scope

- Everything in phases 2-5 (channel names, presets, copy/mirror, local output, recording, health
  history, redundancy, DSP). The design's controls for them are not built in this phase.

## Constraints

- Clean-room design: own protocol names, look and copy.
- ASIO is never chosen by default.
- Touch targets of at least 32 px on desktop (40 px for primary actions); text contrast at least
  4.5:1; state never shown by colour alone; keyboard operable faders.
- Technical artifacts and UI copy in English.
- No regression of today's desktop features: start/stop, driver, device and buffer selection,
  device switch, add/remove target, rename, link health.

## Acceptance criteria

- Moving a fader, pan, mute or solo on the desktop changes what that phone plays, and the phone's
  screen follows without the phone sending the change back.
- A change made on the phone shows on the desktop.
- With an interface that offers 4 inputs, 4 channels appear.
- Meters move smoothly (event driven, not 250 ms polling).
- All existing desktop features work from the new screens.

## Checks

- Server: `cargo test -p personal-monitoring`
- Desktop: `cargo test -p mixlink-desktop`
- Android: `:app:testDebugUnitTest assembleDebug --rerun-tasks` (recount tests from the XML).
- UI: `fnm exec --using=default node --test apps/desktop/ui/tests`
- Live: the desktop app with the Volt and the Galaxy S23.

## Delivery

Strategy `stacked-to-main` (as in the previous features). Forecast about 2,700 authored changed
lines, so it ships as several stacked pull requests, one per reviewed slice. Push and PRs are the
user's decision.

## Tasks

- [x] T1 (capture): prefer the input configuration with the most channels (up to 32) at 48 kHz
  over a stereo one; tests for the ranking. Route: inline (one mechanical file). RED observed
  (`input_config_rank` missing), GREEN: `cargo test -p personal-monitoring` 174 passed.
  Commit: this commit.
- [x] T2 (engine mix control): `EngineHandle::set_mix(address, &MixUpdate)` merges a partial
  update with the same code as a phone's `mix` (`MixUpdate::merge_into`, under a per-mix writer
  lock so the phone and the desktop never drop each other's fields); `EngineEvent::MixChanged`
  carries `origin: MixOrigin::{Client { peer }, Engine}`; each control connection listens on a
  live (tokio broadcast) side of the event bus and pushes the whole mix as the new `mix_state`
  message (the `mix_ack` shape) unless the change came from that same connection; the Android
  app ignores unknown types, so older builds are unaffected; `MusicianDto` carries the full mix
  (flattened `MixDto`); Tauri command `set_mix(address, mix)`. Route: delegated writer. RED
  observed (server: 34 compile errors, `MixUpdate`, `MixState::apply`, `set_target_mix`,
  `MixOrigin`, `should_push_mix`, `mix_state_message`, `parse_mix_update` missing; desktop:
  `set_mix`, `MixUpdateRequest` missing). GREEN: `cargo test -p personal-monitoring` 184 passed
  (10 new), `cargo test -p mixlink-desktop` 25 passed (4 new), `cargo build -p mixlink-desktop`
  ok. About 940 changed lines, mostly tests: over the 400-line heuristic because the engine API,
  the push and the desktop command only prove the behaviour together.
  Commit: this commit.
- [x] T3 (Android): parse the pushed mix and apply it in the ViewModel without sending it back;
  JVM tests. Route: delegated writer. `mix_ack` parsed no values before, so nothing was reused:
  `ControlChannel` reads `mix_state` with `parseMixReport` (every field required; a malformed push
  is reported as an invalid control response, never applied) and calls the new
  `Listener.onMixPushed` (default no-op, so other listeners are unaffected). `MonitorViewModel`
  dispatches `MonitorEvent.MixPushed` and syncs the audio thread's master values, never calling
  `sendMixControl` (no echo). The reducer maps the server's 32/16 tables onto the announced
  channels and groups (the strips' count before the first `config`), clamps to 0-100, ignores a
  push outside a session or one equal to the mix the phone already plays (the server's lagged
  re-send), leaves saved mixes alone, and turns More of me off with a notice, since the pushed
  levels are already lifted. A gesture in flight is not held back: the push lands at once and the
  next gesture step sends its value with the pushed mix around it (last writer wins at the
  server). RED observed (13 compile errors: `MixPushed`, `onMixPushed`, `MIX_CHANGED_ELSEWHERE`
  missing). GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 357 tests (14 new),
  0 failures. Commit: this commit.
- [x] T3b (Android, review R3-001 of T3): converge the phone's mix on the server's answer.
  `ControlChannel` counts the phone's unanswered mixes (counted before the send, taken back when
  the socket refuses it, reset on every open, end and close). The server answers each `mix` in
  order with `mix_ack` or `error`; both count as an answer, so a refused mix cannot leave the count
  stuck. Only the ack that leaves nothing unanswered hands its mix on (`Listener.onServerMix`,
  renamed from `onMixPushed`, through the same `ServerMixReceived` path as a push: no echo, same
  mapping, clamping and More of me rule); older acks still acknowledge but their values are
  ignored, and an ack whose mix cannot be read still acknowledges. A `mix_state` that arrives
  while a mix is unanswered is dropped rather than held: the answer to that mix follows on the same
  connection and is the server's latest word. `mixAcknowledged` is unchanged. Simultaneous edits of
  the same mix on the phone and the desktop remain last-writer-wins on the server, because the
  phone sends whole mixes; per-control sparse updates would need a protocol extension and are a
  phase-2 candidate. Route: delegated writer. RED observed (after the rename-only step: 6 of 365
  tests failed, `aNewConnectionStartsWithNoMixUnanswered`,
  `theAnswerToTheNewestMixIsHandedOnAndAnAnswerToAnOlderOneIsNot`, `aRefusedMixCountsAsAnswered`,
  `aPushWhileAMixIsUnansweredIsDroppedForTheAnswerThatFollows`,
  `aPushOlderThanAMixStillInFlightGivesWayToTheAnswerToThatMix`,
  `withTwoMixesInFlightOnlyTheAnswerToTheNewestIsTaken`). GREEN: `:app:testDebugUnitTest
  assembleDebug --rerun-tasks` ok, 365 tests (8 new), 0 failures. Commit: this commit.
- [ ] T4 (desktop events): subscribe to the engine event bus and emit Tauri events; meters at
  about 30 Hz; the UI keeps one `engine_status` call for the first paint. Route: delegated writer.
- [ ] T5 (UI foundation): tokens, atoms as custom elements, the app shell with the sidebar, the
  state store and pure helpers with `node --test` tests; the old UI still runs. Route: delegated
  writer.
- [ ] T6 (Mix screen): top bar, musician tabs, channel strips, group strips and master strip
  wired to `set_mix` and the events. Route: delegated writer.
- [ ] T7 (Network and system, switch): musicians table and audio setup moved from the old UI;
  switch `index.html` to the new shell; delete the old `main.js` and styles; docs. Route:
  delegated writer.

## Progress

- Branch `feat/desktop-console` from `main` (963881f).
- T1 dd4a5bd: assessed medium, `under_budget` (pending in the slice).
- T2 0c4b94e: assessed medium, `slice_budget_reached`; review granted, lineage
  `review-5b4c5e4e2b2acb8f` over 963881f..0c4b94e (T1 + T2) approved with one reliability lens and
  acknowledged. Next review base: 0c4b94e.
- Review follow-ups (non-blocking, folded into T4 as a T2b commit):
  - R3-001: no test covers the control connection's live-event recovery paths (broadcast lagged
    pushes the whole mix, broadcast closed keeps the connection, failed push closes it).
  - R3-002: `MixState::update` is still a lock-free `pub(crate)` store beside the locking
    `apply`; today only tests call it, but route remaining writers through `apply` (or lock in
    `update`) and add a two-thread test with disjoint fields.

## Next step

T4.
