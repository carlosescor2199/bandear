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
  - Commits: d8e5a70 `feat(engine): expose the phone's client id in the status`; 78dc65d
    `feat(desktop): keep labels in a versioned session file keyed by client id`.
- [x] T1b (review fixes for T1, review `review-c1a7cf3a01994207`): after a session file is moved
  aside, the empty session is saved at once (same atomic write), so the next start never imports
  `musicians.json` again (R3-legacy-reimport-after-recovery); a file that cannot be moved is still
  not saved over. `set_musician_name` routes through `label_musician(session, rows, address,
  name)`, now tested at the command level (R3-set-musician-name-routing-unproved). Route: inline
  writer of the delegated T1 worker (two files, bounded fixes).
  - RED: `after_a_recovery_the_old_label_file_is_never_imported_again` failed (the stale label came
    back), and the two recovery tests updated to expect the saved empty session failed; the routing
    tests failed to compile (no `label_musician`). The no-engine routing test characterises the
    existing address-keyed path.
  - GREEN: `cargo fmt -p mixlink-desktop -- --check` clean; `cargo test -p mixlink-desktop` 64
    passed (3 new: the recovery re-import case, the no-engine address-keyed rename, the
    client-keyed rename); clippy: no warnings in the desktop crate.
  - Commit: this commit (`fix(desktop): never re-import legacy labels after a session recovery`).
- [x] T2 (runtime groups): stable group ids; `EngineHandle::set_groups`; layout generation
  re-sending `config`; register and add_target using the live layout; group-level remap on
  delete or reorder; groups persisted in the session and applied at start; Tauri command.
  Route: delegated writer (trigger: 2+ non-trivial files across the engine, the desktop and the UI).
  - Live configuration (`control.rs`): `ConfigState` holds one immutable `ConfigSnapshot` (channel
    count, `GroupStatus` list with validity, mixer `GroupLayout`) behind `RwLock<Arc<_>>` plus an
    `AtomicU64` generation. `publish` stores a changed snapshot and raises the generation under the
    write lock (an unchanged one keeps it). Every `config` is built from the snapshot; each
    connection's `ConfigFeed` remembers the generation it sent and re-sends on any change (watcher
    poll, top of the read loop, and before every send). New mixes take their membership through
    `Membership::new_mix`, evaluated under the registry's write lock (`register`, `add`), and
    `apply_groups` publishes while holding the registry's read lock, so a registration racing an
    edit or a switch never keeps the old membership. The engine lost its duplicated `groups` and
    `current_layout` fields; the device switch publishes through the same `apply_groups`.
  - Ids: `GroupDefinition.id` (command-line groups "1", "2", ... by order; the desk generates
    `g` + 16 random hex digits, unique in the request), carried by `Group`, `GroupStatus`, the
    `config` message (`id`, a field old phones ignore), `GroupDto` and `ChannelDto.groupId`.
  - `EngineHandle::set_groups` (via `edit_groups`): `check_groups` (at most 16, non-empty unique
    ids, non-empty names, a channel in one group even beyond the device; now public) then the
    lenient revalidation (a missing channel only invalidates its group). When every id keeps its
    position (rename, other channels, device switch) mixers only take the new membership; else
    every mix runs `MixState::regroup` under its writer lock (new revision): a kept group's level
    moves with it, a new group gets 100, a deleted group's level is dropped and unused slots are
    100. `EngineEvent::GroupsChanged` is emitted before the `MixChanged` of each moved mix.
  - Config before mix: `apply_groups` publishes before it moves any level; every mix report
    (`mix_state` push via `current()`, `mix_ack` via `apply`) is read under the mix's writer lock
    and sent through `send_with_config`, which sends `config` first when the generation moved, in
    one hold of the writer. Proof: `after_a_group_edit_the_phone_gets_the_new_config_before_its_moved_levels`
    (real socket) and the unit `a_message_to_a_phone_behind_the_configuration_follows_a_new_config`.
    The desktop window gets the same order: the event pump answers `GroupsChanged` with a status at
    once (`a_change_of_groups_sends_a_status_at_once_ahead_of_the_moved_mixes`).
  - Desktop: `set_groups` Tauri command (`[{ id?, name, channels }]`, 1-based) checks, saves the
    session's `groups` (now typed `[{ id, name, channels }]`; version stays 1 because every earlier
    build wrote the reserved section empty and keeps whatever it reads; a misshapen section is moved
    aside like any misshapen file), then applies live; a refused list or a failed save changes
    nothing; an engine that is stopping refuses and the saved groups apply at the next start. Start
    rule: a request that names no groups (the window's) gets the session's groups applied right
    after the start through `set_groups`, so a channel the device lacks marks its group invalid
    instead of failing the start; a request that names groups uses them strictly and leaves the
    session alone. UI: `setGroups` in the engine API; the Mix screen joins channels to groups by
    `groupId`; the fake bridge keeps groups like the session, applies them at start, re-checks them
    on a device switch and implements `set_groups` with the same remap and event order.
  - Android (read, not changed): `ControlChannel.parseGroups` reads `name`/`channels` with
    `optString`/`optJSONArray`, so `id` is ignored; a mid-session `config` reaches
    `MonitorReducer.applyConfig`, which replaces the definitions and sizes `groupLevels` to the new
    group count by position (new ones neutral), and sends nothing; the following `mix_state` sets
    the server's levels.
  - RED: (a) `after_a_device_switch_a_connected_phone_is_sent_the_live_groups` got
    `Vocals [2,3]` instead of `[]`, and `a_phone_registering_after_a_device_switch_mixes_with_the_live_groups`
    mapped CH 3 to the invalid group (`Some(1)`), against the start-up layout reproduced with the
    new API; (b) 7 failures with positional levels and no move (6 remap/regroup tests and the edit
    test), then the ordering test alone ("the moved levels ... [70,30,...] arrived before the new
    groups") once levels moved; id and check tests failed to compile; `GroupsChanged`: the two edit
    tests and the pump test (`["mix-changed"]` first); (c) session, DTO and command tests failed to
    compile (no `SessionGroup`/`set_groups`/`group_definitions`/`id`); UI 9 of 194 failed.
  - GREEN: `cargo test -p personal-monitoring` 229 passed (19 new), three runs, control tests 31
    passed three times; `cargo test -p mixlink-desktop` 74 passed (10 new); `cargo build -p
    mixlink-desktop` after `cargo clean -p mixlink-desktop` ok; UI 194 passed (7 new); clippy: no
    new warnings. Preview: the Mix screen shows CH 1-3 Drums and CH 5-6 Vocals joined by id.
  - Size: about 2,800 authored changed lines over three commits (about 670, 1,080 and 1,050 with
    this document), above
    the 400-line heuristic mostly in tests (socket ordering, remap rules, session and fake bridge);
    not split artificially.
  - Commits: 2050f8f `fix(control): send the live groups after a device switch`; 57639f9
    `feat(engine): edit groups while running with stable ids`; 8ef33ad `feat(desktop): keep
    groups in the session and apply them live`.
- [x] T2b (review fixes for T2, review `review-2d016eb5546e25ae`): a group edit is one step under
  the engine lock with the session's inside it (the order every command holding both follows; a
  start now reads the saved groups under it too), so the session and the engine always end with
  the same groups (R3-set-groups-save-apply-not-atomic). With an engine running the engine takes
  the groups first and they are saved only then; a stopping engine refuses and nothing changes;
  with no engine they are saved for the next start. `set_groups` answers `{ groups, applied,
  saved, saveError }` (a save that fails after the engine took them is reported, not an error),
  the fake bridge answers the same and `formatGroupsSet` words it ("Groups saved; they apply at
  the next start.") (R3-set-groups-error-after-persist). The session reads its sections one by
  one: a damaged `groups`, `channels` or `presets` section is kept as
  `session.json.<section>-damaged-<unix ms>`, starts empty, and the session is saved without it
  at once, keeping the labels; only a file that is not JSON, has no usable version or unreadable
  labels is moved aside whole; a section that cannot be written aside stops all saves
  (R3-misshapen-group-discards-labels). Route: inline writer of the delegated T2 worker (bounded
  fixes in three desktop files and the UI).
  - The group edit runs through `edit_groups` over a `GroupEngine` trait (the engine in the app,
    a stand-in in tests), so the running-engine branch is tested without audio hardware.
  - RED: `an_engine_that_is_stopping_refuses_the_groups_and_nothing_is_saved` (the session held
    the refused list) and `groups_a_running_engine_took_but_the_session_could_not_keep_say_so`
    failed against the old save-then-apply order; the concurrency test
    `two_group_edits_at_once_leave_the_session_and_the_engine_with_the_same_groups` (30 rounds,
    both edits arriving while the engine lock is busy) passed against the old order too: Windows
    hands the lock over in arrival order, so that interleaving needs a preemption between save and
    lock; it now holds by construction and the test guards it. UI: `formatGroupsSet` did not exist
    (the suite failed to load). Session: both new tests failed (the label was lost, `None`).
  - GREEN: `cargo test -p mixlink-desktop` 80 passed (6 new, 1 replaced), three runs for the
    commands; `cargo build -p mixlink-desktop` after `cargo clean -p mixlink-desktop` ok; UI 195
    passed (1 new); `cargo fmt --check` clean; clippy: no warnings in the desktop crate.
  - Commits: 9fe0901 `fix(desktop): edit groups as one step and say when they apply later`; this
    commit (`fix(desktop): keep labels when one session section is damaged`).
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
- Review `review-c1a7cf3a01994207` over 0c926dd..78dc65d approved and acknowledged; next review
  base 78dc65d. Its findings (R3-legacy-reimport-after-recovery,
  R3-set-musician-name-routing-unproved) are fixed by T1b.

- T2 done (runtime groups); see its entry for evidence. Follow-ups: a phone's own `mix` that
  carries `group_levels` positioned for the old groups and lands just after an edit is applied to
  the new positions (phones send no ids; last writer wins, as decided); the window's pending group
  edits are keyed by position, so T4's editor should settle or drop them when the groups change;
  the window has no group editor yet (T4) and nothing shows the session notice (T7).
- Review `review-2d016eb5546e25ae` over 78dc65d..8ef33ad approved and acknowledged; next review
  base 8ef33ad. Its three WARNINGs (R3-set-groups-save-apply-not-atomic,
  R3-set-groups-error-after-persist, R3-misshapen-group-discards-labels) are fixed by T2b. The
  window shows `formatGroupsSet` once T4's group editor calls `setGroups`.

## Next step

T3.
