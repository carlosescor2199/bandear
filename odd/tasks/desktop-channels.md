# Desktop channels, groups and session (redesign phase 2)

## Objective

Give the engineer the console's structure: named channels with icons, hidden spare inputs and
stereo pairs; groups edited live; per-musician limits and locks; mix presets, copy and mirror;
and a session that is saved and loaded. Musicians are known by their phone, not by their IP.

## Problem and why

- Channels are only numbers (`CH n`) on the desktop and the phone (`dto.rs:302-307`,
  `MonitorUiState.kt:287-288`); spare inputs cannot be hidden and stereo sources take two strips.
- Groups exist only as start-up flags (`cli.rs:46-49`); the desktop always sends `groups: []`
  (`engine-config.js:64`) and nothing edits them while the engine runs.
- Latent bug: the control thread keeps the start-up `GroupLayout` (`engine.rs:996-1010`,
  `control.rs:120`), so after a device switch phones still get the old groups and a newly
  registered phone's mix gets the old membership (`control.rs:519`), while `add_target` uses
  `current_layout` (`engine.rs:873`). `config` is re-sent only when the channel count changes
  (`control.rs:73-93`).
- Group levels are positional on the server and the phone (`group_levels[i]`,
  `MonitorReducer.kt:213`) and the desktop joins groups by name (`mix-view.js:73`).
- Musician labels are keyed by `ip:port` (`names.rs`), which breaks when DHCP moves a phone; the
  phone's stable `client_id` is stored by the engine but never exposed (`targets.rs:25-50`).
- PRD RF7 (save and load session and mix presets) and RF12 (the engineer locks channels and sets
  volume limits per musician) are not implemented (`PRD.md:40-41`).

## Design (approved)

Design canvas "MixLink desktop redesign" (https://claude.ai/artifact/GcjxqS8ME4eBYVSzxjbQtB):
board 3 "Channels and groups" (channel table with name, instrument, level, stereo link, group,
shown; group cards) and the Mix board's "Copy mix to…", "Mirror" and "Presets" controls.

## Design decisions (approved by the user)

- **Identity**: a musician is keyed by the phone's `client_id`; the `ip:port` key remains only for
  configured targets and old phones. `musicians.json` migrates into the session file.
- **Session file**: one versioned JSON file in the app config directory holding channel
  metadata, groups, presets and musician labels; written atomically (temp file and rename).
- **Groups** get stable ids. Deleting or reordering a group remaps every musician's group levels
  on the server (through `MixState::apply`, which bumps the revision and pushes `mix_state`).
  Group edits apply live and phones receive them at once; `config` is re-sent on a layout
  generation change, not only on a channel-count change.
- **Channel metadata** (name, instrument kind, hidden, stereo link) travels to phones in
  `config` as an optional field that old phones ignore.
- **Hidden** means not shown anywhere and not heard in any mix (spare inputs must not leak
  noise).
- **Stereo link** is a control concept: a linked pair of adjacent inputs is one strip whose
  gain, mute and solo write both channels and whose pan becomes a balance; the mixer itself is
  unchanged. Linking sets explicit pans; the server keeps both sides of a link equal.
- **Engineer limit** is a separate per-musician cap; the effective ceiling is the lower of the
  phone's "Level cap" and the engineer's limit.
- **Locks**: the engineer can lock channels per musician; the phone shows them disabled and the
  server ignores phone writes to them.
- **Presets** are engineer mixes saved on the desktop, distinct from the phone's "Saved mixes".
  "Copy mix to…" copies channels, pans, mutes, solos and group levels, not the master volume or
  limits. "Mirror" is one-way: the follower's controls are locked until the engineer stops it.
- **Session load** pushes the saved mixes to the phones; on reconnect the phone's own mix still
  wins, as today.

## Scope

- Identity and the session file; runtime groups with stable ids and the config fix; channel
  metadata end to end (engine, protocol, desktop, Android); the Channels and groups screen and
  the Mix screen using names, icons, hidden channels and linked pairs; engineer limits and
  locks end to end; presets, copy and mirror; session save and load.

## Out of scope

- Phases 3-5 (local output, recording, network history, redundancy, DSP).
- Sparse per-control updates from the phone (simultaneous edits stay last-writer-wins outside
  locked controls).

## Constraints

- Clean-room: own protocol names, look and copy.
- ASIO is never chosen by default; the T7b start-request builder stays the only start path.
- Old phones keep working: every protocol addition is optional and ignored by older clients.
- Accessibility as in phase 1 (contrast 4.5:1, keyboard operable, state never by colour alone).
- Technical artifacts and UI copy in English.

## Acceptance criteria

- Renaming a channel, changing its icon, hiding it or linking a pair shows on the desktop and on
  every phone at once; a hidden channel is silent in every mix.
- Creating, renaming, reassigning and deleting groups while running updates every phone, and no
  musician's group levels move to another group.
- After a device switch, phones and newly registered phones get the live groups.
- A musician keeps their label after their phone's IP changes.
- An engineer limit caps what the phone can reach; a locked channel cannot be changed from the
  phone and shows as locked there.
- A preset can be saved from one musician and applied to another; copy and mirror behave as
  decided above.
- Saving and loading a session restores channels, groups, presets and labels, and pushes the
  saved mixes to connected phones.

## Checks

- Server: `cargo test -p personal-monitoring`
- Desktop: `cargo test -p mixlink-desktop`
- UI: `fnm exec --using=default node --test apps/desktop/ui/tests`
- Android: `:app:testDebugUnitTest assembleDebug --rerun-tasks` (recount from the JUnit XML).
- Live: the desktop app with the Volt and the Galaxy S23.

## Delivery

Strategy `stacked-to-main`, on branch `feat/desktop-channels` stacked on `feat/desktop-console`.
Forecast about 6,000-8,000 authored changed lines, shipped as stacked pull requests per reviewed
slice. Push and PRs are the user's decision.

## Tasks

- [x] T1 (identity and session file): expose `client_id` in the engine status and the desktop
  DTO; a versioned session store in the app config dir with atomic writes, a recovery path for
  a corrupt file, and migration of `musicians.json`; labels keyed by `client_id` with the
  address fallback. Route: delegated writer (engine and desktop Rust, desktop UI; trigger: 2+
  non-trivial files across two crates and the UI).
  - Engine: `MusicianStatus.client_id` (None for configured targets and phones that sent none),
    filled by `announced_identities` from the target registry; the desktop row carries it as
    `clientId` (null when absent).
  - Session (`apps/desktop/src-tauri/src/session.rs`, replaces `names.rs`): `session.json` =
    `{"version": 1, "musicians": {"client:<id>" | "address:<ip:port>": {"label": ...}},
    "channels": [], "groups": [], "presets": []}`; the reserved sections are written back as read.
    Labels resolve `client:` first, then `address:`. A rename stores under the phone's key when
    the running engine lists a client id at that address (and drops the address's label so the
    row shows what was set), else under the address. Saves write `session.json.tmp`, sync and
    rename; a failed save changes neither the file nor what is shown. A corrupt, unreadable or
    misshapen file is moved to `session.json.corrupt-<unix ms>` and the desk starts empty (a file
    that cannot be moved is not saved over); a newer `version` is left untouched and every save is
    refused; both are logged and kept in `SessionStore::notice`. Migration: with no session and a
    `musicians.json`, its labels become `address:` entries and the session is written;
    `musicians.json` stays untouched and is no longer read.
  - UI: rows carry `clientId` and `labelFollows` ("phone" or "address"), and the label field's
    title says which; the fake bridge gives its phones client ids, keeps labels keyed as the
    session does and offers `movePhone(from, to)` (a phone back from a new IP keeps its label).
  - RED: server tests failed to compile (no `client_id` field, no `announced_identities`); the DTO
    test failed (no `clientId`); 14 of 15 session tests failed against an empty skeleton (the
    reserved-sections one passed vacuously); the command tests failed to compile (no
    `SessionStore`, no `client_id_at`); 6 of 187 UI tests failed.
  - GREEN: `cargo test -p personal-monitoring` 210 passed; `cargo test -p mixlink-desktop` 61
    passed (5 `names.rs` tests replaced by 15 session tests, 2 new command tests, 1 new DTO test);
    `cargo build -p mixlink-desktop` after `cargo clean -p mixlink-desktop` ok; UI 187 passed;
    clippy: no new warnings. IP-change proof: `a_musician_keeps_their_label_after_their_phones_ip_changes`
    (command layer), `a_phone_keeps_its_label_when_its_ip_changes` (session) and the UI's
    `a musician's label stays on screen when their phone comes back from a new IP`. Runtime:
    `name_precedence_probe` imported a copy of the real `musicians.json` (1 label), the phone's
    own name still wins, the real config directory gained no `session.json`.
  - Size: about 1,540 authored changed lines over the two commits, above the 400-line heuristic
    because the session store's recovery, migration and refusal paths each carry their tests
    (about 380 lines of session tests); not split artificially.
  - Commits: d8e5a70 `feat(engine): expose the phone's client id in the status`; this commit
    (`feat(desktop): keep labels in a versioned session file keyed by client id`).
- [ ] T2 (runtime groups): stable group ids; `EngineHandle::set_groups`; layout generation
  re-sending `config`; register and add_target using the live layout; group-level remap on
  delete or reorder; groups persisted in the session and applied at start; Tauri command.
  Route: delegated writer.
- [ ] T3 (channel metadata, engine and phone): the channel table in the engine and session;
  `config` carries names, kinds, hidden and links; hidden channels silent in every mix; link
  rules kept equal on the server; Android shows names, drops hidden channels and collapses
  linked pairs. Route: delegated writer.
- [ ] T4 (Channels and groups screen): board 3 in the desktop UI; the Mix screen uses names,
  icons, hidden channels, linked strips and group ids. Route: delegated writer.
- [ ] T5 (engineer limit and locks): engine fields set only by the engineer path and enforced on
  phone writes; reported to the phone; Android shows the limit and locked controls; the desktop
  master strip and channel strips edit them. Route: delegated writer.
- [ ] T6 (presets, copy and mirror): presets in the session; apply to a musician; copy; one-way
  mirror with the follower locked; the Mix screen's controls. Route: delegated writer.
- [ ] T7 (session save and load): save and load the whole session (RF7), pushing saved mixes to
  connected phones; the deferred Android follow-up (adopt the server's answer after `config`);
  docs. Route: delegated writer.

## Progress

- Branch `feat/desktop-channels` from `feat/desktop-console` (0a448e8). Last reviewed boundary:
  0c926dd (phase 1's review `review-6c08ce0320b9f147`); 0a448e8 holds the applied suggestion.

- T1 done (identity and session file); see its entry for evidence. Follow-ups: a label migrated
  from `musicians.json` stays keyed by address until the engineer renames that musician (it is
  not moved to the phone's key on its own); the session notice is only logged, the UI does not
  show it yet (T7 can surface it with session load).

## Next step

T2.
