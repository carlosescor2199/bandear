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

Status at the close of phase 2 (T7b): every criterion is verified by automated tests; each still
awaits the live test with the Volt and the Galaxy S23.

- Renaming a channel, changing its icon, hiding it or linking a pair shows on the desktop and on
  every phone at once; a hidden channel is silent in every mix.
  Verified by tests: engine `after_linking_a_pair_the_phone_gets_the_channel_details_before_its_moved_pans`
  (real socket), the hidden-silence mixer and every-mix tests, `a_device_switch_restores_the_details_of_inputs_that_come_back`
  (T3a, T3a-fix); desktop `set_channels` and status-on-`ChannelsChanged` tests and the Channels
  screen UI tests (T3a, T4); Android `announcedDetailsNameTheStripsShowTheirKindsAndJoinPairs`,
  `aNewConfigMidSessionRebuildsTheStripsAndKeepsTheMix`, `aPairsPanIsABalanceWrittenAsBothPansAndReadBackAsTheirMean`
  (T3b). Pending: live test.
- Creating, renaming, reassigning and deleting groups while running updates every phone, and no
  musician's group levels move to another group.
  Verified by tests: engine `after_a_group_edit_the_phone_gets_the_new_config_before_its_moved_levels`
  (real socket) and the remap and regroup tests (T2); desktop `set_groups` and edit-order tests
  (T2, T2b); Android `announcedGroupsTagTheirChannelsAndKeepTheirLevels`. Pending: live test.
- After a device switch, phones and newly registered phones get the live groups.
  Verified by tests: `after_a_device_switch_a_connected_phone_is_sent_the_live_groups` and
  `a_phone_registering_after_a_device_switch_mixes_with_the_live_groups` (T2). Pending: live test.
- A musician keeps their label after their phone's IP changes.
  Verified by tests: `a_musician_keeps_their_label_after_their_phones_ip_changes` (command layer),
  `a_phone_keeps_its_label_when_its_ip_changes` (session) and the UI's "a musician's label stays on
  screen when their phone comes back from a new IP" (T1). Pending: live test.
- An engineer limit caps what the phone can reach; a locked channel cannot be changed from the
  phone and shows as locked there.
  Verified by tests: engine `a_locked_channel_and_the_engineers_limit_hold_against_the_phones_whole_mix`
  and `a_registering_phone_is_told_the_engineers_controls_before_it_sends_a_mix` (real socket,
  T5a, T5a-fix); desktop and UI master-limit and lock tests (T5a, T5a-fix); Android
  `aLockedChannelCannotBeChangedFromThePhone`, `aPairIsLockedAsOneStripWhenEitherSideIs`,
  `theMusicianHearsTheLowerOfTheirCapAndTheEngineersLimit`,
  `aLimitOnlyReportMovesThePhonesOwnCeilingWithNoMasterEdit` and the locked-label text tests (T5b,
  T5b-fix). Pending: live test.
- A preset can be saved from one musician and applied to another; copy and mirror behave as
  decided above.
  Verified by tests: engine preset, copy and mirror tests with
  `a_followers_phone_is_told_whom_it_follows_and_cannot_move_its_channels` and
  `a_leaders_phone_change_reaches_its_follower_at_once` (real socket, T6a, T6a-fix); desktop preset,
  copy and circle tests and the mix-tools UI tests (T6a-T6a-fix2); Android
  `whileFollowingChannelAndGroupEditsChangeNothing`,
  `whileFollowingOnlyTheMasterMovesAndTheAnswerBringsTheLeadersChannels` and
  `whenFollowingStopsTheControlsAreTheMusiciansAgainAndTheMixStaysAsItWas` (T6b). Pending: live test.
- Saving and loading a session restores channels, groups, presets and labels, and pushes the
  saved mixes to connected phones.
  Verified by tests: the snapshot-store, save and load command tests (pushes, skipped and refused
  musicians, controls and mirrors replaced whole) and the session-panel UI tests (T7a-T7a-fix2);
  Android shows a pushed mix (`aPushedMixTakesTheServersValuesForTheAnnouncedChannelsAndGroups`)
  and, on a first connect, the mix the server holds
  (`aConfigArrivingBeforeTheOpeningMixIsSentLeavesTheServersMixAloneAndThePhoneTakesIt`, T7b).
  Pending: live test.

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
- [x] T3a (channel metadata: engine, protocol, desktop): the channel table in the engine and the
  session; `config` carries names, kinds, hidden and links; hidden channels silent in every mix;
  link rules kept equal on the server; the Tauri command and the fake bridge. Route: delegated
  writer (trigger: 2+ non-trivial files across the engine, the desktop and the UI).
  - Engine: `ChannelMeta { name, kind: ChannelKind, hidden, linked_to_next }` (public, with
    `check_channels` and `MAX_CHANNEL_NAME_LENGTH` = 32). Kinds, closed set, wire names: `voice`,
    `guitar`, `bass`, `keys`, `drums`, `percussion`, `horns`, `strings`, `playback`, `click`,
    `other` (default). Default detail: empty name (clients show "CH n"), `other`, shown, unlinked.
    `check_channels`: at most 32 entries, names of at most 32 characters, a pair only between two
    listed entries (not past the last), never overlapping another pair, both sides hidden or both
    shown. The live `ConfigSnapshot` holds exactly one detail per input (`fit_channels`: kept for
    inputs the device has, defaults past the list, dropped past the device, the last input never
    linked); `publish` (switch, groups) keeps the fitted details and `publish_channels` replaces
    them; both return `(previous, live)`. `EngineHandle::set_channels(Vec<ChannelMeta>)` (via
    `edit_channels`); `EngineStatus.channels`; `EngineEvent::ChannelsChanged`.
  - Hidden: every `MixState` holds the hidden channels as one `AtomicU32` (and the pairs as
    another), set for every mix after the publish (`set_channel_flags`) and for a new mix by
    `Membership::new_mix` (`MixState::configured`); `MixValues.hidden_channels` is read by
    `mix_channels` with the rest of the mix (no lock, no allocation): a hidden channel is skipped
    and its solo ignored. Hiding changes no stored value and takes no revision.
  - Link: every writer's merge (`apply`, via `apply_locked` under the writer lock) runs
    `keep_pairs_equal`: a side the update changed carries the other; when both sides changed to
    different values (or the second did not change alone) the first channel wins; pans are left
    as written. Linking a pair pans it apart (first 0, second 100) in every mix through the same
    merge (the second side takes the first's level, mute and solo), each with a new revision,
    announced after `ChannelsChanged`; a mix created later starts panned apart. Balance rule for
    clients (documented on `ChannelConfig`): balance `b` writes pans `max(0, 2b-100)` and
    `min(100, 2b)` and reads back as their mean.
  - Protocol: `config` gains `channels: [{ name, kind, hidden, link }]`, one per source channel in
    channel order, `link` on a pair's first channel only; old phones ignore it.
  - Desktop: the session's `channels` section is typed `[{ name, kind, hidden, link }]` in channel
    order (version stays 1: earlier builds wrote it empty and keep what they read; every field
    defaults; a kind this build does not know is kept as written and applies as `other`; a
    misshapen section is set aside on its own). Tauri `set_channels(channels)` with the
    `set_groups` semantics (`ChannelRequest` refuses unknown fields and unknown kinds; names
    trimmed; one step under the engine lock with the session's inside; apply then save; answer
    `{ channels, applied, saved, saveError }`; a stopping engine refuses and nothing changes; no
    engine saves for the next start; a `ChannelEngine` trait for the tests). Every start applies
    the session's details after the groups. `ChannelDto` gains `name`, `kind`, `hidden`, `link`;
    the pump sends a status at once on `ChannelsChanged`.
  - UI: `channels.js` (`CHANNEL_KINDS`, `channelLabel`, `balanceToPans`/`pansToBalance`,
    `checkChannels`, `fitChannels`, `keepPairsEqual`), `formatChannelsSet` (guards an empty reason
    and a trailing full stop), `setChannels` in the engine API. The fake bridge names the sample's
    inputs, implements `set_channels` with the same rules (status before the moved mixes, pairs
    panned apart, both sides kept equal on desk and phone changes, details applied at start and
    fitted on a switch) and mixes its meters from the shown inputs, so a hidden input is heard by
    no one.
  - RED: (a) the server tests failed to compile (no `ChannelMeta`, `ChannelKind`,
    `check_channels`, `fit_channels`, `edit_channels`, `publish_channels`, `ChannelFlags`,
    `MixState::configured`/`set_channel_flags`, `MixValues.hidden_channels`, `ChannelsChanged`,
    `ControlConfig.channels`); (b) with the plumbing in place and none of the three behaviours, 8
    of 247 failed: hidden silence (mixer and every mix), a hidden channel's solo, a pair's follow
    (one side, both sides), the link's pans (mixer and every mix) and the socket test (no
    `mix_state` came); (c) ordering: announcing the moved mixes before the publish failed the
    socket test ("the moved pans ... [0,0,100,100,...] arrived before the channel details"); during
    GREEN one expectation was corrected (a flag of an equal pair cannot change to different values
    on both sides, so that case is levels only); (d) the desktop tests failed to compile (no
    `SessionChannel`, `ChannelRequest`, `ChannelsSetDto`, `ChannelEngine`, `edit_channels`,
    `set_channels`, `session_channels`), and the pump test with the old arm saw `["mix-changed"]`
    first; (e) the UI suite failed to load (no `channels.js`), then 7 of 210 failed against the old
    fake (no `set_channels`, no details, meters ignoring hidden).
  - GREEN: `cargo test -p personal-monitoring` 247 passed (18 new), three runs; `cargo test -p
    mixlink-desktop` 94 passed (14 new); `cargo build -p mixlink-desktop` after `cargo clean -p
    mixlink-desktop` ok; UI 210 passed (15 new); `cargo fmt` clean; clippy: no new warnings.
    Ordering proof: `after_linking_a_pair_the_phone_gets_the_channel_details_before_its_moved_pans`
    (real socket; it also acks a one-sided phone `mix` with both sides equal).
  - Size: about 1,280 authored changed lines in the engine commit and 1,500 in the desktop commit
    (with this document), above the 400-line heuristic mostly in tests (about 1,300 lines); not
    split artificially.
  - Commits: aa8f8e3 `feat(engine): name, hide and link channels while running`; 42009e6
    (`feat(desktop): keep channel details in the session and apply them live`).
- [x] T3a-fix (review fixes for T3a, review `review-47e2a093d93eabca`): the engine keeps the
  engineer's channel details whole (`ConfigSnapshot.requested_channels`, at most 32 entries, the
  source of truth, as the group definitions are) and fits them only into what clients see
  (`channels`); a device switch refits, so the details of inputs that come back are shown again,
  names, hidden flags and pairs included. The mixer's flags come from the whole table: a hidden
  spare stays silent while its input is missing and from the moment it returns, and a pair whose
  second side is missing is shown as its first input alone (no `link` in `config` or the status)
  but stays linked in every mix, so it returns with its balance, both sides equal and no pan moved.
  An edit that changes only entries past the device is kept without raising the generation (no
  `config`, no `ChannelsChanged`) (R3-switch-drops-details-session-diverges). `checkChannels`
  defaults a missing name and kind (R3-checkchannels-missing-name-typeerror). Route: inline writer
  of the delegated T3a worker (bounded fixes in two engine files and one UI module).
  - RED: against the T3a code 3 of 249 server tests failed:
    `a_device_switch_restores_the_details_of_inputs_that_come_back` ("all restored": Keys R and
    the hidden Spare came back as defaults and Keys L unlinked, so the spare was audible),
    `config_after_a_switch_back_carries_the_details_of_the_inputs_that_returned` (`link` false
    after the switch back) and `details_past_the_device_are_kept_for_later_without_a_new_config`;
    UI: 1 of 210 failed (`checkChannels` threw on an entry without a name).
  - GREEN: `cargo test -p personal-monitoring` 249 passed (3 new, 1 replaced), control tests 34
    passed in three runs; `cargo test -p mixlink-desktop` 94 passed; UI 210 passed (one assertion
    added); `cargo fmt` clean; clippy: no new warnings. About 300 authored changed lines.
  - Commit: this commit (`fix(engine): keep channel details for inputs that come back after a
    switch`).
- [x] T3b (channel metadata, Android): read `config.channels` (lenient: absent on older servers,
  unknown kinds as `other`); show names (else "CH n") and kind icons; drop hidden channels;
  collapse linked pairs into one strip whose level, mute and solo write both channels and whose
  pan is a balance (`balanceToPans` rule above). Route: delegated writer (trigger: 2+ non-trivial
  files across the control channel, the UI state and the screens).
  - Parsing (`ControlChannel.kt`): `ChannelInfo { name, kind: ChannelKind, hidden, link }` and
    `ChannelKind` (wire names as the engine's; `fromWire` maps a missing or unknown kind to `OTHER`)
    live next to `GroupInfo`. `parseChannelDetails(value, count)` returns null when `config` has no
    `channels` array, else exactly one entry per announced channel: an entry past the count is
    dropped, a missing one, a non-object or a field of the wrong type reads as the default.
    `Listener.onConfig` gains `details`; `ConfigReceived(channels, groups, details = null)`.
  - Strips (pure, `ui/state/ChannelLayout.kt`): `stripLayout(count, details)` gives
    `StripChannels(first, second?)` left to right: a hidden channel has none, a link makes one
    strip of a channel and the next when both are shown (a link on the last channel or into a
    hidden one makes no pair), and null details give one strip per channel as before.
    `balanceToPans(b)` = `(max(0, 2b-100), min(100, 2b))`, `pansToBalance` = the mean, halves up
    (the desktop's `Math.round`), `channelLabel(index, detail)` = trimmed name or "CH n".
    `MixUiState.channelDetails` (null from an older server) feeds `layout`, `channels` (a strip is
    known by its first channel; `kind` null without details so no icon shows, `pairedWith`, the
    pair's level/mute/solo from its first side and its pan as the balance), `groups[].members`
    (shown strips by name, a pair once; by number as before without details), `musicianChoices`
    and `stripSides(channel)`.
  - Edits (reducer): a channel edit names a strip; `withStrip` writes `stripSides` (both sides of
    a pair's strip, else the channel alone) and a pair's pan is written through `balanceToPans`.
    The per-channel lists stay positional and whole, so the whole-mix send (with its `seq`) and
    the saved mixes (`MixBankStore`, by channel index) are unchanged; a mix recalled with a pair's
    sides unequal shows the first side and the server makes them equal. More of me lifts both sides
    of a pair (`MixSnapshot.moreOfMeFor`, used at send time through the channel's new
    `musicianChannels` and by the reducer's echo check), so a pushed lifted mix does not turn it off.
  - My channel: the picker offers one entry per strip of the announced channels, by name; a hidden
    channel or a pair's second side cannot be chosen. On every `config` the remembered channel is
    fitted: on a pair's second side it moves to the pair's first (More of me stays on, the store is
    updated, nothing is sent: the server's `mix_state` with the link's pans follows); a hidden one is
    forgotten like a dropped one (More of me off with "your channel is no longer in the mix").
  - UI: the strip's label button shows the kind icon (`kindIcon`, 11 line icons drawn with the
    existing `lineIcon` builder, no new dependency) and the name (one line, ellipsis), and under it
    the pan, preceded for a pair by the stereo rings (`LineIcons.Stereo`); it is read as "Keys
    stereo pair settings" with "Balance centre". The sheet's header shows the icon, the name (two
    lines at most), the group chip, "Stereo pair" in words and "Your channel"; a pair's pan control
    is titled BALANCE and named "Keys balance". Group strips list `members`. Previews: the sample
    mix names its inputs, pairs the keys and hides a spare; a new "older server" preview shows the
    unchanged look.
  - RED: (a) against stubs (every channel its own strip, the balance written to both pans as is,
    labels by number, `parseChannelDetails` returning null), 21 of 400 failed: parsing 2 of 4
    (the older-server and wire-name tests passed), layout 8 of 9 (no details passed), reducer 9 of
    11 (the older-server and single-strip tests passed), ViewModel 2 of 2. (b) against stubs (every
    kind and the stereo mark drawn as the tick, the settings label always by number, pan wording
    ignoring pairs), 5 of 404 failed: the two icon tests and three sheet-text tests.
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 404 tests, 0 failures, 0
    errors (374 before: 30 new; the group-members text test now takes labels and one More of me
    label assertion reads `labelOf`); no compiler warnings.
  - Size: about 785 authored changed lines in the first commit (about 330 of them tests) and 360
    in the second plus this document, above the 400-line heuristic mostly in tests; not split
    artificially.
  - Commits: 73d5d9c `feat(android): read channel names, kinds, hidden channels and pairs`; this
    commit (`feat(android): show named channels and stereo pairs on the mix`).
- [x] T4 (Channels and groups screen): board 3 in the desktop UI; the Mix screen uses names,
  icons, hidden channels, linked strips and group ids; the T2b review suggestions. Route: delegated
  writer (trigger: 2+ non-trivial files across the UI's view models, organisms, screens and the
  fake bridge).
  - T2b suggestions: `formatGroupsSet` and `formatChannelsSet` share `sentenceWithReason` (one full
    stop whatever the reason ends with; no reason, null or blank, gives "... but not saved.")
    (R3-format-groups-set-unguarded-save-error). The concurrency test now forces the overlap: the
    `GroupEngine` stand-in takes a `Park` that holds the first edit right after the engine took its
    groups and signals the test, which only then starts the second edit; the park waits up to
    100 ms for the session's groups to change and reports it. Each round (6, alternating which
    edit goes first) asserts that no save landed during the park, that the later edit wins and that
    the session and the engine agree (R3-concurrency-test-not-discriminating).
  - Screen (`#/channels`, between Mix and Network in the sidebar and `ROUTES`):
    `screens/channels-screen.js` (container: rAF rendering, meters to the table, events to edits,
    the last outcome in words in a `role="status"` line or a `role="alert"` line) with two
    organisms: `<ml-channel-table>` (a row per input: name field, Enter saves, Escape reverts,
    never overwritten while focused; instrument select with its line icon; horizontal live meter;
    Stereo toggle with "Linked with CH n"/"Mono" in words, and when unavailable it stays focusable
    with `aria-disabled` and the reason beside it via `aria-describedby`; the group chip; a Shown box
    with "Shown"/"Hidden" in words, a pair hiding together) and `<ml-group-cards>` (cards kept by
    id; New group up to 16; Edit opens the name field, the inputs as checkboxes, a pair once, with
    "in Vocals"/"hidden" notes, and a guarded Delete: Keep takes its place and the confirmation
    works only after `CONFIRM_DELAY_MS`, disarming after 10 s or on Escape). `kind-icons.js` draws
    the 11 kinds and the stereo rings. The screen's sheet is `@import`ed by `shell.css` (index.html
    is outside this task's surfaces).
  - Pure logic (`lib/channels-view.js`): `channelRows`, `linkChoice` (a pair from either side;
    otherwise the next input must exist, be in no pair and be shown or hidden alike, each refusal
    a sentence), `groupCards`, `canAddGroup`, `KIND_OPTIONS`, and edits of the whole list:
    `renameChannel`, `setChannelKind`, `setChannelHidden` (both sides of a pair, found in the whole
    table), `setChannelLink` (undo from either side), `addGroup` ("Group n", at most 16),
    `renameGroup`, `setGroupMembers` (joining one group leaves any other; a pair joins with both
    inputs), `deleteGroup`; `channels.js` gains `KIND_LABELS` and `pairUnits`.
  - Edits (`console-app.js` `editChannels(change)`/`editGroups(change)`): one at a time, each built
    on the list the last answer gave (the whole list, entries past the device included), else on
    the status's; with neither (no engine yet) the edit is refused instead of overwriting the
    session; answers `{ ok, message }` with `formatChannelsSet`/`formatGroupsSet` or "... were not
    changed: reason.", then reads the status.
  - Mix: `channelStrips` gives one strip per shown unit (`pairUnits`): hidden inputs have none, a
    pair is one strip `{ sides, stereo }` labelled by its first input, with the first side's
    level, mute and solo and `pansToBalance` as its pan; strips are named by `channelLabel` and
    carry `kind`. `channelEdits` writes a pair's level, mute and solo to both inputs and its balance
    as both pans (`balanceToPans`); `stripLevel` meters a pair by its louder side. The strip shows
    the kind icon, the name (ellipsis, full name as title), a STEREO mark (an empty line on mono
    strips keeps pans and faders aligned) and names its pan "... balance". The screen keeps strips
    by key (channel strips by first input, group strips by group id), so a strip stays with its
    group or input when the layout changes.
  - Settle rule (`console-state.js` `settleLayout`, on every status): pending edits are keyed by
    position, so when the groups' ids change order or number every pending `groupLevels.*` edit is
    dropped, and when an input's role changes (hidden, single, first or second side of a pair) its
    pending channel edits are dropped; a rename keeps everything. Dropped rather than remapped: at
    status time the musicians' mixes are still positioned for the old groups (the moved mixes
    follow), so a remapped edit would be sent with stale neighbours.
  - Fake bridge: the running details are the whole table, fitted only for the status
    (`channelRows`); a switch refits nothing else, pairs in the mixes come from the whole table (a
    pair missing its second side stays linked), a change only past the device announces no status;
    its test now proves the details of inputs that come back are shown again and no pan moves.
  - RED: (a) `formatGroupsSet` printed "...: the disk is full..." (1 of 210 failed); the forced
    test, run against `edit_groups` mutated to save before taking the engine lock, failed in round
    0 ("the second edit was saved while the first was between its apply and its save"), and passes
    against the real order (the old 30-round test had passed against the old order too, T2b).
    (b) against stubs 19 of 226
    failed (the 16 new view-model and app tests, the 2 router tests and the flipped fake switch
    test). (c) against stubs 6 of 231 failed (labels and kinds, hidden and pairs, `channelEdits`,
    `stripLevel`, the two settle tests).
  - GREEN: UI 231 passed (21 new; the router, fake switch, group wording and first strip tests
    changed); `cargo test -p mixlink-desktop` 94 passed, the
    group-edit test 5 of 5 runs (0.65 s each); `cargo fmt` clean; clippy: no warnings in the desktop
    crate. Preview (`dev/serve.mjs 5198`, `/index.html?fake#/channels` and `#/mix`): renamed CH 1
    with Enter, Escape reverted a name, a name being typed survived the 4 Hz statuses, paired CH 3-4
    (CH 2 then says "CH 3 is already paired with CH 4.", CH 6 that it is the last input), hid a
    pair, added a group, moved an input between groups, a double click on Delete kept the group and
    a confirmed Delete removed it; the Mix showed names and icons, no strip for hidden inputs, the
    pair as one STEREO strip whose balance (PageUp twice) read back 70 ("R40"); no console errors.
    `cargo build -p mixlink-desktop`: deferred (the app is running for live testing).
  - Size: about 140 authored changed lines in (a), 2,050 in (b) (about 860 in the two organisms
    with their styles, 450 tests) and 440 in (c) plus this document; above the 400-line heuristic in
    (b) because the screen's two organisms and its pure logic carry their tests; not split
    artificially.
  - Commits: 3310343 `test(desktop): force the group edit interleaving and guard outcome
    messages`; bba07a8 `feat(desktop-ui): add the Channels and groups screen`; 5cabdf2
    (`feat(desktop-ui): show named, hidden and paired channels on the mix`).
- [x] T4b (review fixes for T4, review `review-7abd4e4333f80dbb`). Route: inline writer of the
  delegated T4 worker (bounded fixes across the engine, the desktop commands and the UI).
  - Whole table (R3-first-channel-edit-from-device-fitted-status): a `channel_table` command
    answers the whole table the desk keeps, `[{ name, kind, hidden, link }]` in channel order with
    the details of inputs the device lacks: with an engine running its table
    (`EngineHandle::channel_table`, the `requested_channels` last set, so a failed save does not
    matter), else the session's with an unknown kind read as `other`. `bridge.channelTable()`;
    every channel edit reads it afresh and sends it back whole (the window no longer keeps a
    channel list, and `channelTableOf` is gone); the fake answers it as the desktop does;
    `events.rs` no longer says a switch drops details.
  - Group shape (R3-kept-groups-roundtrip-shape-unproved): `groupDefinitions(groups, first)` turns
    a status's groups (inputs from 0) and an answer's (from 1) into copies of `{ id, name, channels }`
    numbered from 1; the app keeps the last answer through it. A chained app test (add a group, then
    with the engine stopped give it an input and rename another on the last answer's list, restart,
    remove an input) asserts every id and 1-based member; the Rust test sends a `set_groups` answer
    back as the next request and gets the same ids and inputs.
  - Pending toggles (R3-checkbox-reverts-while-edit-in-flight): `pendingToggles()` holds a clicked
    value under a token (`release` with an older token keeps a later click), and
    `withPendingShown` (both rows of a pair follow) and `withPendingMembers` show the held values;
    the screen holds the Shown and group member boxes as clicked until their edit is answered
    (`group-member` events now carry the choice key).
  - RED: (a) the engine test, against a body returning the fitted list, failed ("the inputs the
    device lacks are in it": 2 entries instead of 4); the desktop tests failed to compile (no
    `read_channel_table`); UI 3 of 234 failed (the first edit on the 2-input device sent 2 entries,
    the unreadable-table refusal, the fake's `channel_table`). (b) against stubs UI 2 of 237 failed
    (`groupDefinitions`, the pending toggles); the chained app test and the Rust round-trip test
    passed at once: they pin shapes that already matched.
  - GREEN: `cargo test -p personal-monitoring` 250 passed (1 new); `cargo test -p mixlink-desktop`
    97 passed (3 new); UI 237 passed (6 new; one assertion on the removed `channelTableOf` dropped);
    `cargo fmt` clean; clippy: no warnings in the desktop crate or the new engine code. Preview
    (`?fake#/channels`): hiding an input and putting one in a group work as before, no console
    errors (the fake answers at once, so the held boxes are proved by the pure test). `cargo build
    -p mixlink-desktop`: deferred (the app is running).
  - Size: about 320 authored changed lines in (a) and 300 in (b) plus this document.
  - Commits: 56fdea7 `fix(desktop): edit the whole channel table, not the device's part`; this
    commit (`fix(desktop-ui): keep group ids and members across chained edits and hold pending
    toggles`).
- [x] T5a (engineer limit and locks: engine, protocol, desktop): engine fields set only by the
  engineer path and enforced on phone writes; reported to the phone; kept per musician in the
  session and re-applied on registration; the desktop master strip and channel strips edit them.
  Route: delegated writer (trigger: 2+ non-trivial files across the engine, the desktop and the UI).
  - Engine (`mix.rs`): `EngineerControls { limit_percent: u8 (default 100), locked: u32 }` (bit k =
    channel k) and the partial `EngineerUpdate { limit_percent: Option<i32>, locked:
    Option<Vec<bool>> }` (list from CH 1 on, at most 32, limit clamped; a pair's sides follow the
    side that changed, the first wins when both differ), both public. Every `MixState` holds them
    in two atomics (`engineer_limit: AtomicU8`, `locked: AtomicU32`); `MixValues.engineer` carries
    them; `MixUpdate::merge_into` preserves them, so no `mix` can set them. Writers: `apply` (the
    desk, may change locked channels), `apply_from_phone` (the control connection: after the merge
    `keep_locked` puts back the level, pan, mute and solo of every locked channel, then
    `keep_pairs_equal`), `apply_engineer` (desk path, new revision, no value moves) and
    `restore_engineer` (whole controls, limit capped at 100, pairs locked whole, new revision only
    when changed). Invariant: both sides of a linked pair are locked alike (`lock_pairs` on
    restore, the follow rule on edits, and `set_channel_flags` locks a newly linked pair whole when
    either side was locked).
  - Ceiling: `mix_channels` clamps to `min(max_level_percent, engineer.limit_percent)`, both read
    from the lock-free snapshot (no new lock or allocation on the audio path).
  - Book and identity (`targets.rs`): `MusicianKey { Client(id), Address(SocketAddr) }` (public).
    `TargetRegistry` keeps the engineer's controls per key (an address by IP) behind its own mutex
    (taken after the table's lock, never while a mix writer lock is held). `set_controls` applies
    and keeps them under the row's client id when it has one (dropping any kept for its IP), else
    its address. `register` (new row, IP move by client id, re-registration, adopted configured
    target) and `add` put the kept controls in force under the table's write lock, client id first
    then IP, so the phone's first `mix` after registering already meets its locks and limit (no
    event is emitted there: the ack carries them, the desk sees them with the next status).
    `keep_controls` stores and re-resolves every row.
  - API: `EngineHandle::set_engineer_controls(address, &EngineerUpdate) -> (MusicianKey,
    MixValues)` (refuses while stopping; unknown musician or oversized list is an error and nothing
    changes; emits `MixChanged{Engine}`, so the phone gets `mix_state`) and
    `EngineHandle::keep_engineer_controls(&MusicianKey, EngineerControls)` (seeds the book; a
    present musician whose mix changed is announced).
  - Protocol (`MixReport`, `mix_ack` and `mix_state`): `"engineer_limit_percent": 60` (0-100, 100 =
    no limit, apart from the phone's own `max_level_percent`) and `"locked": [true, false, ...]` (32
    entries, aligned with `channels`), documented on the struct; old phones ignore them; a phone's
    `mix` that carries them changes nothing.
  - Session: a musician entry gains optional `"engineer": { "limitPercent": 60, "lockedChannels":
    [1, 2] }` (1-based numbers) under the label's key (`client:<id>` else `address:<ip:port>`),
    absent at the defaults; version stays 1 (an earlier build reads the entry and keeps the label
    but drops `engineer` when it saves); an unreadable `engineer` reads as none and costs nothing
    else. `engineer_for`, `engineer_controls`, `set_engineer` (client key drops the address key's
    controls, as labels do).
  - Desktop: Tauri `set_engineer_controls(address, controls: { limitPercent?, lockedChannels? })`
    (`deny_unknown_fields`), one step under the engine lock with the session's inside: a running
    engine takes them first, then they are saved under the key it returned; a stopping engine or an
    unknown musician refuses and nothing is saved; no engine merges into the address's kept
    controls (`applied: false`); answer `{ address, limitPercent, lockedChannels, keptFor:
    "phone"|"address", applied, saved, saveError, mix }`. Every start hands the engine the session's
    controls after the groups and channels (`session_engineer_controls`: limit capped, numbers
    outside 1-32 and unreadable keys skipped). `MixDto` gains `engineerLimitPercent` and
    `lockedChannels` (flat on status rows, nested in `mix-changed`, in the `set_mix` answer);
    `set_mix` refuses them.
  - UI: the mix model carries `engineerLimitPercent` and `lockedChannels` (`ENGINEER_FIELDS`;
    older payloads read as open); their edits are pending edits like a fader's (`viewMix` shows
    them, a layout change drops a lock edit as it drops a level edit) but go out through their own
    coalescer to `set_engineer_controls` (`splitEdits`, `buildEngineerRequest`); a refusal is the
    usual `mixError`, an answer that did not both apply and save is `controlsNotice`, said above
    the strips (`formatEngineerControlsSet`). The master strip has a LIMIT fader ("Engineer limit",
    sent when a gesture ends) beside the phone's CAP ("Phone level cap"), a readout "85% · cap 90% ·
    limit 60%" and the line "The lower of the phone's cap and your limit wins: at most 60%." (both
    faders described by it). Each channel strip has a lock toggle whose text is its state
    ("Locked"/"Unlocked", plus the pressed fill and lamp), named "Lock Kick on the musician's
    phone"; a pair locks as one (`channelEdits`). The fake bridge implements `set_engineer_controls`
    with the same rules (pairs, phone kept off locked channels, desk free, meter ceiling = lower
    of cap and limit, kept by client id else address, idle saves for the address, every start
    re-applies, a pair linked after a lock locked whole) and `engineerKeys()`.
  - RED: (a) with the plumbing and stubbed behaviour (the phone path was the desk's, no-op
    `apply_engineer`/`restore_engineer`, ceiling ignoring the limit, reports without the fields)
    19 of 270 server tests failed (10 mix, 3 targets, 3 engine, 2 real-socket control, 1
    protocol); the protocol guard that a `mix` cannot set the controls passed (true by
    construction). (b) against stubs (controls never saved, DTO without them, start handing none)
    9 of 111 desktop tests failed; the refusal, not-saved, unreadable-entry, newer-session and
    request-parsing tests passed (guards). UI against stub exports: 23 of 260 failed (22 of the 23
    new tests and the updated mix field list); "set_mix cannot carry the engineer's controls"
    passed (guard).
  - GREEN: `cargo test -p personal-monitoring` 270 passed (20 new), control tests 36 passed in
    three runs; real-socket proofs `a_locked_channel_and_the_engineers_limit_hold_against_the_phones_whole_mix`
    (lock CH 1 and limit 60: `mix_state` shows them, the phone's whole mix moving CH 1 and its cap
    is acked with seq, CH 1 unchanged, cap 90 kept, limit and lock reported, CH 1 at full scale
    held at 60 %) and `a_phone_back_from_a_new_ip_after_the_grace_period_meets_the_engineers_controls`
    (row removed after the grace period, the phone registers from another IP with its client id
    and its first mix is held). `cargo test -p mixlink-desktop` 111 passed (14 new); UI 260 passed
    (23 new; the mix field list, the first strip, the master strip and the mix messages
    expectations gained the new fields); `cargo fmt` clean; clippy: no new warnings. Preview
    (`?fake#/mix`): every strip shows "Unlocked", Kick's toggle reads "Locked" once pressed, PageDown
    x4 on LIMIT gives "limit 60%" and "at most 60%" with the output meters lower, the demo phone
    kept moving other channels while Kick stayed put, no console errors; the master readout now
    wraps inside the strip. `cargo build -p mixlink-desktop`: deferred (the app is running).
  - Size: about 1,280 authored changed lines in the engine commit and 2,000 in the desktop commit
    (about 480 of them the new UI suite) plus this document; above the 400-line heuristic mostly
    in tests; not split artificially.
  - Commits: d15c36f `feat(engine): let the engineer limit and lock a musician's mix`; this commit
    (`feat(desktop): keep engineer limits and locks per musician and edit them on the mix`).
- [x] T5a-fix (review fixes for T5a, reviews `review-0ea360ebfcf62163` over 0408441..d15c36f and
  `review-dc5567254620fe48` over d15c36f..bd0d75b). Route: inline writer of the delegated T5a
  worker (bounded fixes in two engine files, three desktop files and the UI).
  - R3-reused-row-inherits-previous-occupants-locks: a row reused by another musician (a different
    client id, or none where there was one, or the other way round) gets that musician's kept
    controls or the defaults (`replace_controls`), never the previous occupant's; the same musician
    still keeps theirs. `Registration` gains `controls_changed`.
  - R3-register-restore-not-announced: when a registration changes the controls the connection
    emits `MixChanged{Engine}`, so the phone is sent `mix_state` with `engineer_limit_percent` and
    `locked` right after it registers, before anything else it sends is handled, and the desktop
    follows.
  - R3-idle-controls-shadowed-by-phone-key: `set_engineer_controls` takes the row's `clientId`
    (the window sends it; Tauri `client_id: Option<String>`); with no engine the edit builds on the
    controls kept for the phone and is kept under `client:<id>`, dropping the address's (the key
    the next engine resolves first, so nothing stands in for it); without one the address must
    parse as a musician address and is kept under its canonical `address:` key. A running engine
    still decides the key itself. The fake bridge does the same.
  - R3-master-limit-change-wiring-untested: the master strip's wiring moved to
    `lib/master-controls.js` (`wireMasterControls`, `showMasterValues`), tested with stand-in faders
    on the real slider gesture: a finished limit gesture or key press sends one
    `engineerLimitPercent` edit and moves send none, a re-render mid-drag does not move the thumb,
    the level and cap go out as they move.
  - RED: engine 2 of 272 failed (`a_different_phone_at_a_reused_address_gets_its_own_controls_or_none`:
    the other phone's mix kept them; `a_registering_phone_is_told_the_engineers_controls_before_it_sends_a_mix`:
    no `mix_state` came); desktop 2 of 113 (`idle_controls_for_a_known_phone_are_kept_under_its_client_id_for_the_next_start`,
    `an_idle_edit_must_name_a_musician_address`); UI 7 of 265 against a stub wiring module and the
    old API (4 master wiring tests, the fake idle test, the API and app argument expectations).
  - GREEN: `cargo test -p personal-monitoring` 272 passed (2 new), control tests 37 passed in three
    runs; `cargo test -p mixlink-desktop` 113 passed (2 new); UI 265 passed (5 new, 2 expectations
    updated); `cargo fmt` clean; clippy: no new warnings. Preview (`?fake#/mix`): PageDown x3 on
    LIMIT gives "limit 70%" and "at most 70%", no console errors. `cargo build -p mixlink-desktop`:
    deferred (the app is running).
  - Size: about 175 authored changed lines in the engine commit and 330 in the desktop commit plus
    this document.
  - Commits: 57dd604 `fix(engine): give a new phone at a reused address its own engineer
    controls`; this commit (`fix(desktop): keep idle engineer edits under the musician's strongest
    key`).
- [x] T5b (engineer limit and locks, Android): read `engineer_limit_percent` and `locked` from
  `mix_ack` and `mix_state` (lenient: absent on older servers means no limit, nothing locked);
  show the engineer's limit apart from the phone's level cap and say the lower one wins; show a
  locked channel's controls disabled with the state in words (a pair as one strip); keep sending
  whole mixes (the server keeps the locked values and the ack carries them). Route: delegated
  writer (trigger: 2+ non-trivial files across the control channel, the UI state and the screens).
  - Parsing: `EngineerControls { limitPercent = 100, lockedChannels: Set<Int> = {} }` and
    `MixReport(mix, engineer)` next to `ChannelInfo`; `parseMixReport` returns a `MixReport` and
    `parseEngineerControls` reads leniently (a limit that is not a number is no limit, out of range
    is held to 0-100; `locked` absent or not an array is none, a short list leaves the rest open, a
    non-boolean entry is open). `Listener.onMixPushed(report)` / `onMixAnswered(seq, report?)`;
    `MixPushed`/`MixAnswered` gain `engineer` (default: open). The `mix` message never carries them.
  - State: `MixUiState.engineer` (never sent, never saved), `effectiveCap` = min(level cap, limit),
    `stripLocked(channel)` (a pair when either side is), `ChannelUiState.locked`. The controls of any
    readable report hold at once (`withEngineer`), even from a push a pending change overtakes: the
    phone never sends them and the server reports them in order; the values settle through the
    existing `seq` rules (the answer to the newest change carries the kept values). A refusal
    carries none; a report outside a session changes nothing; `SessionStarted` resets them to open
    (a new server reports its own, an older one never does).
  - Locks in the reducer: `withStrip` refuses any edit (level, pan/balance, mute, solo) of a locked
    strip, so a whole-mix send never carries a changed locked value and nothing is sent; a recalled
    saved mix keeps a locked channel's level, pan, mute and solo (`recalled`).
  - More of me (decided): a locked channel is neither lifted nor lowered (`moreOfMeFor(channels,
    locked)`, used at send time through the channel's new `lockedChannels` and by the echo check, so
    the server's kept values do not turn it off); when the musician's own channel is locked More of
    me goes off with "More of me is off: the engineer locked your channel" (`CHANNEL_LOCKED`), turning
    it on is refused with that notice while it stays locked, and choosing a locked channel while it
    is on turns it off (My channel can still be a locked channel). When the lock comes in a report,
    nothing is sent and the mix stays as the server plays it, as for any desktop change.
  - Playback: the receiver's local ceiling is `effectiveCap`; the phone sends its own cap.
  - UI: a locked strip shows a "Locked" tag with a padlock (`LineIcons.Lock`) where its group chip
    was, its fader and Mute/Solo keys fade and take no input, and TalkBack hears "Vox level, locked
    by the engineer", "Mute Vox, locked by the engineer" and "Vox settings, locked by the engineer".
    The sheet shows "Locked by the engineer" with the padlock over its disabled pan/balance, level and
    keys. The Master card keeps the phone's Level cap and adds "Limited to 60% by the engineer · at
    most 60%" (the lower of the two) when the engineer set a limit; the cap control is described
    with it. Previews: drums locked and a 60% limit under an 80% cap; a locked channel's sheet.
  - RED: (a) against stubs (controls never read or adopted, edits and recall unaware of locks, the
    lift ignoring them, the effective cap the phone's cap), 13 of 421 failed: parsing 2 of 4 (the
    older-server and never-sent guards passed), reducer 9 of 11 (the older-server and session-reset
    tests passed vacuously), ViewModel 2 of 2. (b) against stub wording, 4 of 425 failed (three
    Mix-screen text tests and the locked label button).
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 426 tests, 0 failures, 0 errors
    (404 before: 22 new); no compiler warnings.
  - Size: about 485 authored changed lines in the first commit (about 260 of them tests) and 185 in
    the second plus this document.
  - Commits: 2c01a83 `feat(android): read the engineer's limit and locks`; this commit
    (`feat(android): show locked channels and the engineer's limit`).
- [x] T5b-fix (review fixes for T5b, review `review-34d9af3b149c510c`). Route: inline writer of the
  delegated T5b worker (bounded fixes in three UI-state files and their tests).
  - R3-recall-and-lift-split-half-locked-pair: `MixUiState.lockedSides` applies the strip rule
    (each locked channel, and both sides of a pair when either side is locked), and recall, the More
    of me lift and lower (the channel's `lockedChannels` now reads it) and the reducer's echo check
    all use it, as edits already did through `stripLocked`, so a half-locked pair is never split.
  - R3-limit-only-push-playback-unproved: the report path already republished the receive thread's
    ceiling because the engineer's controls are part of `mix`, which `serverMixReported` compares;
    the rule is now documented there and tested: a `mix_state` that only sets the limit to 60, then a
    `mix_ack` that only lowers it to 45, move `maxLevelPercent` with no master edit.
  - RED: 3 of 430 failed against the per-channel rule (`recallKeepsAPairWholeWhenOnlyOneSideIsLocked`:
    the open side took the saved values; `moreOfMeLeavesAPairWholeWhenOnlyOneSideIsLocked`: the open
    side was lowered and the echo turned More of me off; the ViewModel's half-locked send was
    `[50, 100, 100]`). The limit-only test passed against the T5b code (characterisation); a mutant
    that ignores the engineer's controls when deciding to republish fails it.
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 430 tests, 0 failures, 0 errors
    (426 before: 4 new); no compiler warnings.
  - Commit: this commit (`fix(android): treat a half-locked pair as locked and follow a new limit
    at once`).
- [x] T5c (the audio input the engineer chose is reopened and remembered; bug from the live test
  on 2026-10-15): the engineer switched the input live under WASAPI to "INPUT 1/2 (Volt 4)" and a
  later Start reopened the Windows default ("Varios micrófonos (Realtek)"). The live switch
  (`switchDevice` in `console-app.js`) never recorded the device in the applied audio choices, so
  the only start path (`startRequest`, phase 1's T7b) asked for none; and the choices lived only in
  memory, lost when the app closed. Route: delegated writer (trigger: 2+ non-trivial files across
  the desktop Rust and the UI).
  - Choices: `{ host, device, bufferFrames, asioDriver }`: `device` is the input picked under auto
    or WASAPI (null for the system's default), `asioDriver` the one picked under ASIO. A plain start
    opens `rememberedDevice` (ASIO: the picked driver; else `device`). A successful live switch
    records `device`; a restart on a named device (auto) records it too; a change of driver that
    names no input starts on the new driver's default (`audioChoice`); a buffer restart still
    reopens the running device. A refused switch or a failed start changes nothing.
  - Session (`session.rs`): optional `audio` section `{ host, device, bufferFrames, asioDriver }`,
    every field optional, read on its own (a damaged one is set aside as
    `session.json.audio-damaged-<ms>`, the rest kept); version stays 1 (an earlier build drops it
    when it saves). Exposed by a read command rather than the status, because the window needs it
    before any engine runs: `audio_settings` (null before any) and `set_audio_settings` (kept only
    while an engine runs, for a known driver, never ASIO without a picked driver; names trimmed; a
    refusal or a failed save changes nothing). The fake bridge mirrors both (`savedAudio` option and
    accessor) and now fails a start on an input it lacks, as the engine does (a part of the name, in
    any case).
  - Window: reads the choices once, before the automatic start (`restoredAudio`: none gives the
    defaults; ASIO without a driver, from an older or odd file, is the system's own driver). They
    are saved only once an engine started or switched with them (`applyAudio`, a live switch, or the
    next start of a choice kept while nothing ran, `setAudio`); a failed save is said ("The audio
    input runs but was not saved for the next start: ...") and tried again at the next start.
  - Missing input: a plain start (Start, Try again, the automatic start, a start after a stop) that
    fails while its input is no longer listed for its driver (matched as the engine matches)
    starts on `fallbackAudio`: the same driver's default input, or the system's own driver when the
    choice was ASIO, so never ASIO. It says "The saved input ... was not found, so the engine
    started on the system's default input. The saved input is kept and tried again at the next
    start." (Mix and Network screens, `role="status"`, for that run), saves nothing, and the next
    start tries the saved input again. A failure while the input is listed is shown as before; a
    default input that fails too is said with both. While a run is on the default input a buffer
    change waits for the next start instead of restarting it.
  - Network screen: the driver and buffer selects show the remembered choice; the device select
    shows the running input, or with no engine the one the next start opens.
  - RED: (a) Rust against stubs (`audio()` none, `set_audio` a no-op, `keep_audio_settings`
    accepting anything and keeping nothing, `audio` not read on its own) 8 of 121 failed (4 session,
    4 command; the damaged-section test lost the labels as the whole file was moved aside). (b) UI
    against stub exports and the old controller 26 of 286 failed: the 21 new tests and 5
    expectations that gained `device` or `audio` (the live-switch test: `audio.device` undefined and
    the next start on the default). The test for a default input that fails too, added after that
    run, failed with the old controller (10 of 287 then; the refused-switch test passed there as a
    guard, since nothing was ever recorded).
  - GREEN: `cargo test -p mixlink-desktop` 121 passed (8 new); UI 287 passed (22 new, 5
    expectations updated); `cargo fmt` clean; clippy: no warnings in the desktop crate; `cargo build
    -p mixlink-desktop` after `cargo clean -p mixlink-desktop` ok. Preview (`index.html?fake`):
    WASAPI, a live switch to "Microphone (USB audio)" (confirmed), Stop and Start: the engine
    reopened it, and the selects show WASAPI and that input.
  - Follow-ups: live check with the Volt (switch, restart the app, then start with it unplugged); a
    missing saved input is not offered in the device list; after an ASIO fallback the driver select
    shows ASIO while the default input runs (the notice says why); a driver change drops the saved
    input, as the old restart on the driver's default did.
  - Size: about 450 authored changed lines in the desktop commit and 870 in the UI commit (about 460
    of them tests) plus this document; above the 400-line heuristic mostly in tests; not split
    artificially.
  - Commits: 32b1eaf `feat(desktop): keep the audio input in the session for the next start`; this
    commit (`fix(desktop-ui): reopen the input the engineer chose, also after a restart`).
- [x] T5c-fix (review fix for T5c, review `review-a8a8d8e4b1bf18da`,
  R3-read-choice-latched-on-failure): the window marked the kept audio choices read before
  `audio_settings` answered, so after a failed read a reconnect never read them again and the
  automatic start opened the defaults. Now the choices are known only once a read succeeded (or
  once the engineer made a choice, which a later read never replaces); a failed read is tried again
  by the next connect, Try again or Start. While they are unknown nothing starts on its own: the
  console shows the connection error "The saved audio input could not be read, so nothing was
  started: ..." with Try again, and Start says the same without starting. Route: inline writer of
  the delegated T5c worker (one file and its tests).
  - RED: 2 of 290 UI tests failed (`a kept input that could not be read is read again on
    reconnect, and the start opens it`: no start after the reconnect read nothing;
    `while the kept input cannot be read nothing starts, and the console says why`: the default
    input started). The third new test (`a choice the engineer makes while the kept one is unread is
    not replaced by a later read`) passed against the old code as a guard, since nothing was read
    again.
  - GREEN: UI 290 passed (3 new); preview (`index.html?fake`): the automatic start still runs.
  - Commit: this commit (`fix(desktop-ui): retry reading the kept input instead of starting the
    default`).
- [x] T6a (presets, copy and mirror: engine, protocol, desktop): presets in the session; apply to
  musicians; copy; one-way mirror with the follower's phone held off its channels and groups; the
  Mix screen's controls; the items folded into T6. Route: delegated writer (trigger: 2+
  non-trivial files across the engine, the desktop and the UI).
  - Engine (`mix.rs`): `MixPreset { channels: Vec<PresetChannel { gain, pan, muted, solo }>,
    groups: Vec<(id, level)> }` (public). `MixPreset::of(values, layout, count)` takes the device's
    inputs and every group level by the live layout's ids; `MixState::apply_preset` (desk writer)
    puts each listed input (up to 32) and the level of every live group the preset names by id, a
    live group it does not name and an input past it keeping their own, a pair's second side taking
    the first side's level, mute and solo (the first wins), locked channels included.
    `MixState::follow(&leader)` (desk writer) copies every channel value and group level in place
    (both mixes are placed for the same groups) and returns `None` (no revision) when they already
    match. Neither ever touches the master volume, the phone's cap, the master mute or the
    engineer's controls. `MixState::set_following(name)` (new revision, no value moves) and a
    `following` name per mix (a mutex beside the writer lock, never on the audio path);
    `apply_from_phone` runs `keep_followed` (every channel value and group level kept) before
    `keep_locked` while it follows, so only the phone's master, cap and mute apply.
  - Book and identity (`targets.rs`): `Mirror { leader: MusicianKey, name }` kept per follower key
    (client id, else IP; a phone's key drops the IP's) beside the controls, with the same lock rules
    (taken after the table's, never with the controls' or a mix writer lock). `set_mirror(follower,
    Some((leader, name)) | None)`: refuses an unknown musician, following oneself, and a circle
    (`reaches` walks the leaders from the new leader, resolving a present musician client id first
    then IP, and counts a chain longer than the book as a circle); starting puts the name on the mix
    and copies the leader's mix at once; stopping leaves the mix as it is. `propagate(ip)` makes every
    follower take its leader's mix after any change (breadth first, each mix once, down chains).
    `register` and `add` put the kept mirror in force like the controls (`restore_mirror`: kept or
    none, and the present leader's mix copied at once; `Registration.followed` tells the connection
    to carry it on); a leader who registers changes nobody's mix (a fresh neutral mix is never
    copied: their next change is). `keep_mirror` (the desk's saved mirrors at start) refuses
    oneself and circles. `mirror_statuses` gives `MirrorStatus { leader, name, leader_address }`
    (`leader_address` null while the leader is away; the follower keeps the last mix and stays held).
  - API (`engine.rs`): `EngineHandle::mix_preset(address)`, `apply_preset(&preset, musicians)`,
    `copy_mix(from, to)` (refuses copying onto itself), `set_mirror(follower, Some(MirrorSource {
    address, name }) | None) -> MirrorOutcome { follower, leader, name, mix }` and
    `keep_mirror(follower, leader, name)`; a preset or copy onto a follower is refused ("… follows
    Ana's mix: stop that mirror before changing their mix") and nothing changes. Every desk path
    (`set_mix`, presets, copies, mirror starts) and the phone's `mix` announce the followers moved
    (`MixChanged{Engine}`, each after its leader's), so their phones get `mix_state` at once. The desk's
    `set_mix` may still change a follower's mix (the UI does not offer it); the leader's next change
    replaces it. `MusicianStatus.mirror`.
  - Protocol: `mix_ack` and `mix_state` gain `"following": "<name>"` or `null` (always present),
    documented on `MixReport`; old phones ignore it.
  - Session: `presets` takes its shape (version stays 1: earlier builds wrote it empty and keep what
    they read): `[{ id, name, mix: { channels: [{ gain, pan, muted, solo }] in channel order (inputs
    past the device kept; a field left out defaults to 100, 50, false, false), groups: { <groupId>:
    level } } }]`; a misshapen section is set aside on its own. A musician entry gains `"mirror": {
    "leader": "client:<id>" | "address:<ip:port>", "name": "Ana" }` (lenient: unreadable is none).
  - Desktop: Tauri `presets`, `save_preset(address, name)` (a running engine gives the mix; id `p` +
    16 hex digits; names trimmed, 1-40 characters, unique in any case; answer `{ preset, presets }`),
    `rename_preset(id, name)`, `delete_preset(id)` (no engine needed), `apply_preset(id, musicians)`
    and `copy_mix(from, to)` (engine required; answer `{ mixes: [{ address, mix }] }`) and
    `set_mirror(address, clientId?, leader: { address, clientId?, name } | null)`: one step under the
    engine lock with the session's inside; a running engine takes it first, then it is saved under
    the follower key the engine returned with the leader's key; no engine keeps it under the
    strongest key the window gave (an empty client id is none; the address must parse), refusing
    oneself and a circle over the session's keys; answer `{ address, leader, name, keptFor, applied,
    saved, saveError, mix }`. Every start hands the engine the session's mirrors after the controls
    (`session_mirrors`; unreadable keys skipped and logged). `MusicianDto.mirror: { leader,
    leaderAddress, name } | null`.
  - UI: `lib/mixes.js` (`checkPresetName`, `presetOf`, `withPreset`, `withShared`, `sameShared`,
    `closesCircle`, `mirrorChoice`, `mixTargets`, `mixToolsView`, `joinNames`, `formatMirrorSet`,
    `formatCopied`, `formatPresetApplied`); the engine API (`presets`, `savePreset`, `renamePreset`,
    `deletePreset`, `applyPreset`, `copyMix`, `setMirror`); the console keeps `presets` (read once
    connected, `loadPresets` again) and answers each tool `{ ok, message }` in words, reading the
    status after. `<ml-mix-tools>` beside the tabs (the design's row): Mirror select (Off and every
    other musician except one that would close a circle; an absent leader shows as "Ana (not here)"
    with a note that the mix stays as Ana left it), Copy mix to… (one or more musicians; a follower
    offered disabled with "follows Bass") and Presets (save this mix as…, a preset select, Apply to
    with this mix ticked first, Rename, Delete behind the delayed confirm with Keep in its place);
    both panels are disclosures closed by Escape. The outcome is said above the strips (status or
    alert). A follower's channel and group strips say "Follows Lead vocal" (and in their accessible
    names) and their level, pan, mute and solo or group fader are disabled; the lock stays. The fake
    bridge implements all of it with the engine's rules (`isSocketAddress` reads addresses as Rust's
    `SocketAddr`).
  - Folded: (a) the desktop treats an empty or blank `clientId` as none in `set_engineer_controls`
    (`phone_id`), and the fake checks idle edits as the desktop does (`isSocketAddress`: a host name
    and port is refused; a blank client id is none) (R3-fake-idle-validation-diverges); (b) a kept
    input that can never be read says "… so nothing was started: <reason>. Choose an input in Network
    and system to start without it." and choosing one starts the engine and keeps it
    (R3-persistent-unreadable-choice-blocks-start); (c) a failed read while an engine runs leaves it
    running and Start says why through the command error (R3-running-engine-read-failure-untested).
  - RED: (a) server against stubs (`MixPreset::of` empty, no-op `apply_preset`/`follow`/
    `set_following`/`keep_followed`, registry mirror methods refusing or doing nothing, reports
    without `following`) 19 of 291 failed: 5 mix, 7 targets, 4 engine, 2 real-socket control, 1
    protocol. (b) desktop tests failed to compile (no `MixesEngine`, `save_preset_with`,
    `rename_kept_preset`, `delete_kept_preset`, `apply_preset_with`, `copy_mix_with`, `edit_mirror`,
    `session_mirrors`, `SessionMirror`, `MirrorRequest`, store `set_presets`/`set_mirror`). (c) UI:
    `mixes.test.js` against a stub module 9 of 9 failed; against the committed console, fake and
    view code (with `mixes.js` present and a stub `isSocketAddress`) 11 of 308 failed (the 9 new
    fake and console tests, and the 2 strip expectations that gained `following`). (d) folded: UI 4
    of 311 failed (the wording in three console tests, the fake accepting a host name), desktop 1
    of 134 failed (an empty client id kept for "phone").
  - GREEN: `cargo test -p personal-monitoring` 291 passed (19 new), control tests 39 passed in three
    runs; real-socket proofs `a_followers_phone_is_told_whom_it_follows_and_cannot_move_its_channels`
    (mirror start pushes `following` with the rack's channels, a desk change of the rack reaches the
    phone, its whole mix keeps the channels and applies its master and cap, stopping frees it) and
    `a_leaders_phone_change_reaches_its_follower_at_once` (the rack's mix equals the phone's before
    the ack, announced as an engine change after the phone's). `cargo test -p mixlink-desktop` 134
    passed (13 new, 1 session test reshaped for typed presets); UI 311 passed (21 new; 3 expectations
    updated); `cargo build -p mixlink-desktop` after `cargo clean -p mixlink-desktop` ok; `cargo fmt`
    clean; clippy: no new warnings. Preview (`index.html?fake#/mix`): Bass follows Lead vocal (the
    outcome line, the note, every Bass strip "Follows Lead vocal" and disabled, Lead's demo moves
    followed live), a preset saved, applied to Lead vocal and Keys, deleted after Keep/Delete, a copy
    to Drums; no console errors.
  - Follow-ups: the name a follower's phone is told is the leader's name when the mirror was set (a
    later rename shows after the mirror is set again); a mirror kept with no engine is checked for
    circles over the session's keys only (one musician known by phone in one mirror and by address
    in another is not joined; the engine refuses such a circle at start and logs it); the Mix tools'
    row wraps the tabs at narrow widths; live check with the desktop app and phones (T6b).
  - Size: about 2,000 authored changed lines in the engine commit, 1,560 in the desktop commit,
    1,300 and 710 in the two UI commits and 260 in the folded fixes with this document; above the
    400-line heuristic mostly in tests (about 2,100 lines), split by work unit.
  - Commits: 38317f1 `feat(engine): copy, mirror and apply mixes from the desk`; 62d8ddb
    `feat(desktop): keep presets and mirrors in the session`; 49e8a8f `feat(desktop-ui): handle
    presets, copies and mirrors in the console`; 413ab3c `feat(desktop-ui): add presets, copy and
    mirror to the mix`; this commit (`fix(desktop): say how to get past an unreadable input and
    check idle edits as the desktop does`).
- [x] T6a-fix (review fixes for T6a, reviews `review-fa0392e2f0a9b62d`, `review-bad720d5ff930d4c`
  and `review-c8654069e3cf6af8`). Route: inline writer of the delegated T6a worker (bounded fixes
  in the engine, the desktop commands and the UI, one commit each).
  - R3-mirror-propagation-can-apply-stale-leader-values: a follower takes its leader's mix under a
    registry propagation lock, from the read of the leader's mix to the store of every copy (in
    `propagate_from` and in `restore_mirror`), so two changes of one leader carried on at the same
    moment cannot leave the older mix last; lock order: table, propagation, mirrors, mix writer.
  - R3-ack-following-read-separately-from-applied-values: `MixState::apply_from_phone_report`
    answers the values and the hold read under the same writer lock, and `mix_ack` uses both
    (`apply_from_phone` is now a test helper).
  - R3-row-takeover-clears-mirror-untested: tests for a different phone taking over a follower's
    row (no hold; the first phone's mirror still kept for it) and a target added where a mirror is
    kept (it follows at once).
  - R3-offline-circle-key-mismatch: with no engine each musician is known by every key the window
    gave (phone and address), keys are compared as the engine's book compares them (phones by client
    id, addresses by IP) and each step looks the mirror up by every known key, phone first.
  - R3-offline-same-ip-refusal: checked against the engine, whose book keeps an address by its IP
    (`BookKey::Ip`) and whose table holds one musician per IP (a second target on one IP is
    refused), so two ports of one host are one musician to it. The offline check now uses the same
    comparison (`same_musician`) and keeps refusing that case; tests pin both sides
    (`kept_mirrors_refuse_two_addresses_on_one_ip_as_one_musician`,
    `with_no_engine_a_mirror_on_oneself_is_refused_as_the_engine_refuses_it`).
  - R3-engine-path-raw-address-key: the engine-running path writes the address as the status does
    before saving, so the follower's address entry is found and cleared.
  - R3-untested-save-failure-and-start-apply: tests for a save that fails after the engine took a
    mirror, a follower kept by address while running, a stopping engine refusing presets and
    copies (the stand-in now refuses copies while stopping, as the engine does), and the start
    hand-off, now `hand_kept_mirrors` over a `MirrorKeeper` trait, which names each refusal for the
    log.
  - R3-mirror-select-stale-after-refusal and R3-mix-tools-component-untested: the tools' rules
    moved to `lib/mix-tools.js` (`createMixTools`, DOM-free with a stand-in clock and timers):
    the Mirror control shows a choice only while it is on its way and the desk's mirror after any
    answer, also after its options were rebuilt; ticks follow the open musician and a follower
    loses its tick; nothing but a rename goes out without an engine; Escape closes a panel and names
    the button that takes the focus back; the delete waits `CONFIRM_DELAY_MS` and disarms after 10 s;
    an answer empties a done save or rename. `followingState` words and disables a follower's
    strips. `<ml-mix-tools>`, `<ml-channel-strip>` and `<ml-group-strip>` render what it answers.
  - RED: engine 2 of 296 failed (the held propagation left the follower on `[11, 11]` after the
    leader's `[22, 22]`; the ack's hold was none); the takeover, add and one-IP tests passed at once
    (they pin behaviour that held). A first run of the interleaving test passed against the old code
    because the test hook held its own mutex while parked, which serialised the propagations; the
    hook now releases it first. Desktop 3 of 141 failed (the stubbed hand-off handed nothing, a
    non-canonical address left the address's entry, a mixed-key circle was saved); the other four
    passed at once. UI: `mix-tools-logic.test.js` 8 of 8 failed against a stub controller.
  - GREEN: `cargo test -p personal-monitoring` 296 passed (5 new), control tests 39 passed in three
    runs; `cargo test -p mixlink-desktop` 141 passed (7 new); UI 319 passed (8 new); `cargo build -p
    mixlink-desktop` after `cargo clean -p mixlink-desktop` ok; `cargo fmt` clean; clippy: no new
    warnings. Preview (`index.html?fake#/mix`): a refused mirror choice on the focused select
    returns to the desk's mirror, mirrors start and stop, Escape gives the focus back to Presets, the
    delete waits and confirms.
  - Commits: 8531b9d `fix(engine): keep mirrors in order and report the hold the ack used` (about
    190 changed lines); d5e87ac `fix(desktop): check mirror circles and addresses like the engine`
    (about 420); this commit (`fix(desktop-ui): show the mirror the desk has and test the mix tools`).
- [x] T6a-fix2 (review fixes for T6a-fix, review `review-af815f7391283cf6`). Route: delegated
  writer (bounded fixes in the desktop commands and session, and in the UI, one commit each).
  - R3-offline-circle-later-steps-single-key: with no engine every step of the circle walk knows
    the musician by all the keys the desk has for them (the window's keys for the follower and the
    leader, and each kept mirror's pairs), joined as the engine's book compares keys (phones by
    client id, addresses by IP), and looks the mirror up by each of them, phones first. A mirror
    kept with a phone's key now keeps the address beside it (`leaderAddress`, `followerAddress` in
    the session, both optional, written with and without an engine): with no engine that is the
    only way the desk knows a phone and an address are one musician. Mirrors kept by earlier builds
    carry no addresses and are joined only through the keys the window gives.
  - R3-pending-mirror-cleared-only-by-unseen-answer: the screen did call `answered('mirror')` (its
    `tool("mirror", …)` handed every answer to `<ml-mix-tools>`), but a command that failed or
    found no musician open left the choice on show, a desk mirror changed by another route did not
    replace it, and between the answer and the next frame the control showed the old mirror (seen
    in the preview). The wiring moved to `mixToolCommands` (`lib/mix-tools.js`, DOM-free): every
    command answers the tools (in a `finally`), the screen hears the outcome first and gives the
    tools the store's data at once, and `setData` drops a choice on its way when the desk's mirror
    changes.
  - RED: desktop 1 of 143 failed (the keys player's mirror on Ana was saved over a three-musician
    circle with mixed keys); the non-circle guard passed at once. UI 2 of 323 failed against the
    screen's wiring moved unchanged into `mixToolCommands` (a desk change kept the old choice; a
    failed command never answered the tools), the done and refused wiring tests passing at once;
    then the console-level check of what the control shows when the answer lands, 1 of 323 failed
    (`''`, the old mirror, instead of Keys).
  - GREEN: `cargo test -p mixlink-desktop` 143 passed (2 new; the session's round-trip test pins
    the two addresses); UI 323 passed (4 new); `cargo fmt` clean; clippy: no new warnings. Preview
    (`index.html?fake#/mix`): choosing Bass, Keys and Off goes from the choice straight to the
    desk's mirror and says it at once; save and copy still answer; no console errors.
  - Follow-up: the fake bridge's idle circle check still compares keys as strings (not by IP, and
    without the kept addresses).
  - Commits: 56b82ed `fix(desktop): follow every key of each musician when checking mirror
    circles` (about 320 changed lines); this commit (`fix(desktop-ui): clear a pending mirror
    choice when the desk answers or changes`, about 240 plus this document).
- [x] T6b (presets, copy and mirror, Android): read `following` from `mix_ack` and `mix_state`
  (lenient: absent on older servers is null); while it is set say "Following <name>'s mix", show
  every channel strip's level, pan or balance, mute and solo and every group fader disabled (as
  locks do, groups included), keep the master volume, level cap and master mute; keep sending whole
  mixes (the server keeps the channels and groups and the ack carries them); decide what recall of
  a saved mix and More of me do while following (likely: refused with the notice, as for a locked
  channel). Route: delegated writer (trigger: 2+ non-trivial files across the control channel, the
  UI state and the screens).
  - Reading: `EngineerControls.following: String?` (one more of the engineer's controls);
    `parseEngineerControls` reads it leniently (absent, null or not a string follows nobody; an
    empty name is still followed and shown as "another musician"). It therefore holds at once, even
    from a push a pending change overtakes, while the leader's values settle through the existing
    `seq` rules; a refusal keeps it; `SessionStarted` resets it; the `mix` never carries it.
    `MixUiState.following`, `ChannelUiState.following`, `GroupUiState.following`, `leaderName`.
  - Rules (reducer): while following, every channel edit (`withStrip`) and group edit
    (`setGroupLevel`) changes nothing, so nothing is sent; the master volume, level cap and master
    mute still apply and are sent in the whole mix. Decided: a saved mix is not recalled at all
    while following (not even its master), with the notice "\"Verse\" was not recalled: your mix
    follows Ana's", and the ViewModel sends nothing; More of me goes off with "More of me is off
    while your mix follows another musician's" (`FOLLOWING`) when following starts (reports,
    `config` and a chosen channel go through `offIfHeld`, which also covers a locked channel), and
    turning it on is refused with that notice while it lasts. When `following` turns null every
    control is the musician's again, the mix stays as it was, and a `FOLLOWING` notice is cleared
    (More of me stays off until turned on).
  - UI: the Mix screen shows a blue banner (a polite live region) "Following Ana's mix" with "Your
    channels and groups follow that mix. Your master, level cap and mute are still yours."; channel
    strips show a "Following" tag (`LineIcons.Follow`) where the group was, or "Locked" when the
    channel is locked too, and their fader and keys fade and take no input; group strips show the
    tag under their chip and a disabled fader; TalkBack hears ", following Ana's mix" (after ",
    locked by the engineer" when both) on faders, keys, the label button and the pan control; the
    channel sheet shows "Following Ana's mix" and, when locked, "Locked by the engineer" over its
    disabled controls. Previews: following Lead vocal with the kick locked (channels and groups).
  - RED: (a) against stubs (`following` never read, `MixUiState.following` null), 6 of 436 failed
    (2 parsing, 3 reducer, 1 ViewModel); the never-sent guard (one more assertion) passed. (b)
    against stubs (no blocking, recall and More of me unaware, strips not told, empty wording), 11
    of 448 failed: 6 of 7 reducer tests (the master-still-works guard passed), 2 ViewModel and 3
    text tests; the follow icon's test came with the icon.
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 449 tests, 0 failures, 0 errors
    (430 before: 19 new); no compiler warnings.
  - Size: about 125 authored changed lines in the first commit and 410 in the second plus this
    document.
  - Commits: ef67872 `feat(android): read whose mix the phone is following`; this commit
    (`feat(android): show and respect a followed mix`).
- T7 (session save and load, RF7) is split in two: T7a (desktop) and T7b (Android).
- [x] T7a (session save and load, desktop): named session snapshots saved, listed, loaded, renamed
  and deleted; a load pushes the saved mixes to the phones; the items folded into T7 for the
  desktop. Route: delegated writer (trigger: 2+ non-trivial files across the desktop Rust, the UI
  and the fake bridge); no engine change was needed (the status already lists every musician's
  mix with its client id, and the live group ids).
  - Snapshot store (`apps/desktop/src-tauri/src/sessions.rs`): one file per snapshot,
    `sessions/<id>.json` beside `session.json` (id `s` + 16 hex digits; only ids of letters,
    digits, `-` and `_` are read or written, so none reaches outside the folder), written atomically
    like the session (`write_atomically` is now generic, the temporary file `<name>.tmp`). Shape:
    the whole session at version 1 (labels, controls, mirrors, `channels` as kept, unknown kinds
    included, `groups`, `presets`, `audio`) plus `name`, `savedAt` (unix ms) and `mixes`: `{
    "client:<id>" | "address:<ip:port>": { channels: [{ gain, pan, muted, solo }] (the device's
    inputs at save time), groups: { <groupId>: level }, volumePercent, muted } }`, never the phone's
    own cap (the engineer's limit and locks stay with the musician entry). Read as tolerantly as the
    session (`versioned_fields` and `read_session` are shared): a damaged section, or one musician's
    unreadable mix, is left out and named; a file that is not JSON, has no usable version or
    unreadable labels, or comes from a newer version is listed with its `problem`, cannot be loaded
    or renamed, is never changed, and can be deleted. Names trimmed, 1-40 characters, unique in any
    case; listed newest first.
  - Commands: `session_notice`, `sessions`, `save_session(name)` (`{ session, engineRunning }`; the
    mixes are read from the running engine's status under the engine lock, with the session inside
    it; no engine saves the session without mixes), `rename_session`, `delete_session` and
    `load_session(id)`.
  - Load (`load_session`): the saved groups and channel details are checked first (`check_groups`,
    `check_channels`), and a session file the desk must not replace (newer version) refuses the load
    before the engine takes anything. With an engine running, one step under the engine lock with
    the session's inside: groups, then channel details (as a start applies them; `config` before
    any moved mix, as the engine guarantees), then the engineer's controls (every key either session
    keeps controls under: the loaded controls, or what the loaded session resolves for that musician,
    so a phone key the old session had never shadows the loaded address entry) and mirrors (a
    musician here whose mirror changes or goes is stopped first, then every kept mirror is handed to
    the engine; a refusal is named), then each saved mix pushed through the desk's `set_mix` path to
    the musician listed under its key (client id first, then address), so the phone gets
    `mix_state`: channels from CH 1 on, group levels by id onto the live positions (a live group the
    mix does not name keeps its level), master volume and mute, never the cap. Push rule (decided:
    on reconnect the phone's own mix wins): only musicians here; a phone whose control channel is
    down is skipped (`notConnected`), a saved mix with nobody here too (`notHere`); a configured
    target with no phone gets its mix (it plays it); a follower gets only its master volume and mute.
    Then the session is replaced (`SessionStore::replace`); a save that fails after the engine took
    it is reported. With no engine the session is replaced and every saved mix is `notRunning`.
    Audio: applied only at the next start (the engine is never restarted), and taken only as the
    engineer could choose it (`session_audio`: a known driver, ASIO only with its picked driver);
    otherwise the desk keeps its own and `audioSkipped` says why. Answer `{ session, applied, saved,
    saveError, mixes: [{ address, follows, mix }], skipped: [{ key, label, reason, error }], refused,
    damaged, audio, audioLoaded, audioSkipped, presets }`.
  - UI: `lib/sessions.js` (`checkSessionName`, `formatSavedAt`, `sessionRows`, `sessionPanelView`,
    `formatSessionSaved`, `formatSessionLoaded`, `skippedName`, `createSessionPanel`,
    `sessionPanelCommands`); the engine API (`sessionNotice`, `sessions`, `saveSession`,
    `renameSession`, `deleteSession`, `loadSession`); the console reads the list and the notice once
    connected, answers each command `{ ok, message }` and reads the list again. A load runs in turn
    after any channel or group edit on its way, drops every waiting edit (`dropEdits` when it is
    asked for, `sessionLoaded` when it is answered), forgets the last group answer (so the next group
    edit builds on the loaded groups; channel edits already read the table afresh), takes the loaded
    presets and, when taken, the loaded audio input for the next start. `unsavedEdits()` counts the
    waiting mix edits and the list edits on their way. `<ml-session-panel>` in the Network and system
    screen's aside under the audio input: the desk's notice, "Save session as", the saved sessions
    (select with when and how many mixes, or why not), Load (asks "N changes are still on the way to
    the engine. Loading … drops them." with Cancel and Load anyway while edits wait), Rename, Delete
    behind the delayed confirm (Keep in its place), the outcome in words (status or alert). The fake
    bridge implements all of it with the same rules (`savedSessions`, `sessionNotice`, `clock`).
  - Folded items: (a) T4 follow-up: after a load the window's group list is the loaded one and
    waiting edits are dropped; (b) WARNING R3-idle-channel-edit-rewrites-unknown-kinds: `edit_channels`
    keeps a kind this build does not know that the session stores for an input while the edit leaves
    that input's kind as it applies (`other`), with or without an engine (`keep_unknown_kinds`);
    choosing another kind replaces it; (c) SUGGESTION R3-screen-pending-hold-wiring-untested: the
    Channels screen's wiring moved to `channelsScreenCommands` (channels-view.js, DOM-free) and is
    tested with a bridge that defers `set_channels`/`set_groups` (effect included) until released: a
    clicked Shown or group box stays as clicked, also through a status with the old value, until the
    answer, and a refused edit lets it show the desk's value; (d) `SessionStore::notice` is shown in
    the Session panel ("When the console opened its saved session: …"); (e) the fake's offline mirror
    check compares keys as the desktop does (`sameMusician`: phones by client id, addresses by host)
    and follows every key of each musician through the kept `leaderAddress`/`followerAddress`
    (`closesCircleOffline`, a port of `closes_circle`); the fake now keeps those addresses as the
    desktop does.
  - RED: (a) Rust against stubs (no mixes captured or read, no name or id checks, a load applying
    only groups and channels, the audio taken unchecked) 13 of 162 failed: 6 of 8 snapshot-store
    tests and 7 of 11 command tests (the guards for an unreadable snapshot, a failed save, a save with
    no engine, refused groups or channels, a newer session and a stopping engine passed); the
    refused-mirror naming test was added after that run. (b) UI against a stub `sessions.js` and the
    T6-era console, state, fake and bridge: 20 of 343 failed (all new). (c) the unknown-kind test
    failed (`kind: "other"` saved for the Theremin); the channels-screen tests failed 3 of 3 against a
    mutant that never holds a box; the fake's circle test failed 1 of 11 against the old string
    comparison (one expectation gained `followerAddress`).
  - GREEN: `cargo test -p mixlink-desktop` 164 passed (21 new: 8 snapshot store, 2 save, 10 load, 1
    unknown kinds); UI 347 passed (24 new); `cargo fmt` clean; clippy: no warnings in the desktop
    crate; `cargo build -p mixlink-desktop` after `cargo clean -p mixlink-desktop` ok.
    `personal-monitoring` untouched. Preview (`index.html?fake#/network`): Save as "Sunday
    service" (4 musicians' mixes), Load (pushed to Lead vocal, Bass, Keys and Drums), the question
    while edits wait (Cancel focused), Delete only after the pause; no console errors.
  - Size: about 1,390 changed lines in the save commit, 1,240 in the load commit, 1,970 in the UI
    commit (about 600 of them tests) and 520 in the fix commit plus this document; above the
    400-line heuristic mostly in tests, split by work unit under the 2,000-line review budget.
  - Follow-ups: a load replaces the labels too, so a label set after the snapshot is gone; export
    and import to arbitrary files is phase 4; live check with the desktop app and the phones (a load
    while a phone moves a fader: the later writer wins). Resolved by T7a-fix: a control or mirror the
    loaded session dropped for a musician who was not here stayed in the running engine's book until
    the next start (the engine could not forget one); the engine now replaces its books whole.
  - Commits: c5eacf2 `feat(desktop): save named sessions with every musician's mix`; abcea45
    `feat(desktop): load a named session and push its mixes to the phones`; 196a5a1
    `feat(desktop-ui): save and load sessions from the console`; 34012d6 `fix(desktop): keep
    unknown kinds through edits and test the screen's held boxes`.
- [x] T7a-fix (review fixes for T7a, reviews `review-0c47b67a0faa9250` and
  `review-65ac6b51cab567d0`, and the reported engine gap). Route: inline writer of the delegated
  T7a worker (bounded fixes in the engine, the desktop commands and the UI, one commit each).
  - Engine gap (T7a follow-up): `EngineHandle::replace_kept(controls, mirrors)` replaces both books
    whole under the registry's locks (table read lock held throughout; the controls book, then the
    mirror book, each released before any mix writer lock): `TargetRegistry::replace_kept_controls`
    clears and refills the controls book and resolves every musician here again to the kept controls
    or the defaults; `replace_mirrors` clears the mirror book, admits each mirror in order as
    `keep_mirror` does (shared `admitted`: never on oneself, never a circle with those admitted
    before; a refusal is returned) and puts every musician here on their mirror or frees them
    (shared `restore_mirrors`); each changed mix is announced as an engine change, the controls'
    first. `load_session` hands it the loaded session's controls and mirrors in place of the per-key
    keep and stop calls, so a lock, limit or mirror the loaded session dropped never comes back when
    a musician who was away registers.
  - R3-refused-mix-untested: the test stand-in can refuse the hand-over of the controls and mirrors
    and a push; tests pin a refused hand-over (named, the mixes still pushed, the session saved), a
    refused mirror (named) and a refused push (`SkippedMixDto { reason: "refused", error }`, the
    other mixes pushed).
  - R3-refused-load-drops-waiting-edits: the console no longer drops waiting edits when a load is
    asked for; while it loads the mix and engineer coalescers send nothing (the edits stay pending),
    a successful load drops them (`sessionLoaded`) and a refused one sends them as usual
    (`sendWaiting`). The `dropEdits` action is gone.
  - R3-explicit-other-cannot-clear-unknown-kind: `channel_table` reads a kind this build does not
    know as written (the session's; with an engine running, where the engine's table has `other`
    and the session keeps an unknown kind for that input), so the window sends it back untouched;
    `channel_details` takes an unknown kind only where the session keeps exactly that kind for that
    input (kept as written, applied as `other`), and an explicit `other` replaces it. The UI's
    `checkChannels(channels, kept)` passes such a kind where the table read has it, the edit
    functions check against the table they were given, and the fake keeps the same rule (its status
    shows the kind as it applies).
  - RED: engine 3 of 3 new tests failed against insert-only books (the dropped lock and mirror came
    back on registration, the rack's limit and mirror stayed); desktop 3 of 166 failed against
    mutants that drop refusals (the refused hand-over, mirror and push went unnamed); the kind tests
    2 of 166 failed against the T7a code (the table read `other`); UI 2 of 10 session-console tests
    failed against the committed console (the edits were dropped before the answer and lost on a
    refusal), and the two kind tests 2 of 350 against the T7a channel code.
  - GREEN: `cargo test -p personal-monitoring` 299 passed (3 new), control tests 39 passed in three
    runs; `cargo test -p mixlink-desktop` 166 passed (3 new, 1 replaced, 2 updated); UI 350 passed (3
    new, 1 replaced); `cargo fmt` clean; clippy: no new warnings; `cargo build -p mixlink-desktop`
    after `cargo clean -p mixlink-desktop` ok.
  - Commits: 44be346 `fix(engine): let a loaded session replace kept controls and mirrors` (about
    760 changed lines); 2b4ba22 `fix(desktop): keep waiting edits when a load is refused and
    round-trip unknown kinds` (about 420).
- [x] T7a-fix2 (review fixes for T7a-fix, review `review-386947d3e30791b9`). Route: inline writer of
  the delegated T7a worker (one engine fix, one desktop fix).
  - R3-replace-kept-not-atomic: `TargetRegistry::replace_kept(controls, mirrors)` holds one read of
    the table from the controls' replacement through the last musician resolved for the mirrors
    (the books' locks taken and released inside it, before any mix writer lock, as before), so a
    registration meets both old books or both new ones; `replace_target_kept` makes one call and
    announces the controls' changes, then the mirrors'. A test hook (`between_replacements`) tries
    a registration between the two steps.
  - R3-channel-table-stale-session-kind: `SessionStore::channels_behind` (set when a running engine
    took channel details, or a loaded session, whose save then failed; cleared by every save of the
    details and by a start, whose engine takes the session's) gates the raw kind: with an engine
    running and the session behind, `read_channel_table` reads the engine's kinds and
    `channel_details` takes no unknown kind as untouched, so an explicit `other` the engine took is
    not undone by the stale kind on the next edit. With no engine the session is the truth.
  - RED: the hooked test failed against a mutant that releases the table between the two steps (the
    registration got in and met the dropped mirror); the failed-save test failed against a mutant
    that ignores the flag (`"theremin"` read back instead of `"other"`).
  - GREEN: `cargo test -p personal-monitoring` 300 passed (1 new), control tests 39 passed in three
    runs; `cargo test -p mixlink-desktop` 167 passed (1 new); `cargo fmt` clean; clippy: no new
    warnings; `cargo build -p mixlink-desktop` ok. The desktop crate's incremental cache hit a rustc
    panic once (`dep_graph` `try_mark_green`, index out of bounds); `cargo clean -p
    mixlink-desktop` cleared it (likely the transient build failure seen in T7a-fix too).
  - Commits: 4a688ca `fix(engine): replace kept controls and mirrors in one step` (about 170 changed
    lines); 262942b `fix(desktop): read a stored unknown kind only while the session matches the
    engine` (about 150).
- [x] T7a-fix3 (tests; review `review-c9bf544c067c5dbb` over 2b4ba22..05f3a6a, two SUGGESTIONs on
  T7a-fix2). Route: inline writer of the delegated T7a worker (one desktop file).
  - R3-channels-behind-load-and-start-untested: the start's hand-over of the session's groups and
    details moved to `hand_session_layout` (generic over the group and channel stand-ins) and is
    tested to clear the flag; a load the engine took but the desk could not save is tested to set it
    (and the next save that works to clear it).
  - R3-behind-drops-unrelated-unknown-kinds: decided to pin the current behaviour. A per-input
    comparison cannot tell an untouched unknown kind from the explicit `other` the earlier WARNING
    was about (the engine reports both as `other`, every other field equal), so while the session is
    behind every input reads as the engine applies it; a one-line comment in `read_channel_table`
    says so and a test pins it (an unedited unknown kind reads, and is saved next, as `other`).
  - Evidence: `cargo test -p mixlink-desktop` 170 passed (3 new); against a mutant that neither sets
    the flag on a failed load save nor clears it at start, the two flag tests fail (2 of 170); the
    pinned test passes against the T7a-fix2 code by design; `cargo fmt -p mixlink-desktop --
    --check` clean.
  - Commit: this commit (`test(desktop): cover the session-behind flag on load and start`).
- [x] T7b (session save and load, Android): the deferred Android follow-up (adopt the server's
  answer after `config`, R3-config-pairs-musician-channel-moreofme-on-unsent) and whatever the
  phone needs to show a loaded session's pushed mix; docs. Route: delegated writer (two bounded
  fixes in the control channel, the reducer and the ViewModel). A loaded session's pushed mix
  needed nothing new: it is a `mix_state`, shown through the push path since phase 1.
  - First connect (desktop-console T3c follow-up). Found: the opening mix was read when the
    control thread got to it, and the server's `config` follows the open at once; when `config` was
    handled first, the opening mix carried the neutral strips `config` had just made over the
    server's channels and groups. (With the opening mix sent first, it already carried empty lists,
    the server kept its mix and its answer, which follows `config`, was shown.) Decided: the opening
    mix is read on the socket's thread when the connection opens, before any of its messages is
    handled, and only sent on the control scheduler (`ControlChannel.sendOpeningMix`, sharing
    `outgoingMixMessage` with every other send). A phone with no channels yet sends no channel
    values and no group levels, so the server keeps what it holds and its answer gives it to the
    phone; the master volume, level cap and mute are the phone's own and are sent as always. A
    reconnect still sends the phone's own mix (the phone's mix wins, as decided). The `seq` rules,
    More of me, locks and following are unchanged (the same message builder and answer path). Known
    window: a change the musician makes between `config` and the server's first answer (a few
    milliseconds) is sent whole, as any change is.
  - R3-config-pairs-musician-channel-moreofme-on-unsent. Decided: with More of me on, a `config`
    that makes the musician's channel part of a new stereo pair (on the second side, where My channel
    moves to the first, or on the first) turns More of me off with "More of me is off: your channel
    is now part of a stereo pair" (`CHANNEL_PAIRED`, `offIfNewlyPaired` beside `offIfHeld`), and
    nothing is sent: the link's own `mix_state` follows with the pans it moved, which a send would
    overwrite with the phone's old ones, and it is then taken as the server plays it. Turned on
    again, More of me lifts both sides. A pair the strip already was, or one elsewhere, changes
    nothing; a `config` that drops or hides the channel still resends as before.
  - RED: 3 of 454 failed against the T6b code: the held-scheduler first connect (the opening mix
    was `[100, 100, 100]`, not `[]`), and the pairing tests in the reducer (More of me stayed on)
    and the ViewModel. The plain first-connect ordering and the reconnect tests passed (they pin
    behaviour that held). A mutant that resends when the pairing turned More of me off fails the
    ViewModel test (one more `mix`).
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 454 tests, 0 failures, 0 errors
    (449 before: 5 new; the T3b test that expected More of me to stay on now pins the move alone);
    no compiler warnings.
  - Size: about 115 authored changed lines in the first commit and 90 in the second plus this
    document.
  - Commits: 2520b56 `fix(android): take the server's mix after the first config`; 05f3a6a
    `fix(android): keep More of me consistent when a link moves my channel`.
- [x] T7b-fix (review fix for T7b, review `review-c9bf544c067c5dbb`). Route: inline writer of the
  delegated T7b worker (the reducer, the ViewModel and their tests).
  - R3-paired-moreofme-off-keeps-server-lift: when a link turned More of me off, the server still
    played the lift the phone had sent, and the link's `mix_state` (taken as the server plays it)
    baked that lift into the phone's own mix, so the musician heard a lift the screen said was off
    and a new lift went on top of it. Now the `config` keeps the musician's own levels
    (`MonitorUiState.ownLevelsToRestore`; the lift is applied at send time and never stored); the
    first report after it (the link's `mix_state`, or the answer to a change made meanwhile) is
    taken with those levels put back on every channel the lift touched (`dropLinkedLift`: not a
    locked channel; a pair's second side at its first side's level, as the link sets it; nothing
    while following) and its pans as the link moved them, and is numbered as the phone's newest mix
    when it differs, so the ViewModel sends it once through the `seq` path (`serverMixReported`). A
    new session clears it. The T7b decision (off with a notice, nothing sent on the `config`) stands.
  - RED: 2 of 455 failed against the T7b code with non-neutral levels: the reducer kept the lifted
    `[40, 30, 30, 35, 45]` instead of `[80, 60, 60, 70, 90]`, and the ViewModel sent nothing after
    the link's push (8 mixes, not 9).
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 455 tests, 0 failures, 0 errors
    (454 before: 1 new; the T7b ViewModel test, which used neutral levels, now checks the restored
    levels, the moved pans and a single lift afterwards); no compiler warnings.
  - Commit: c91d0d2 `fix(android): drop the More of me lift when a link turns it off`.
- [x] T7b-fix2 (review fix for T7b-fix, review `review-4c6fd738e50cbb30`). Route: inline writer of
  the delegated T7b worker (the reducer and its tests).
  - R3-own-levels-stale-after-meanwhile-edit: a change the musician makes while a lift restore
    waits is sent with their own unlifted levels (More of me is off), so every sent change now
    clears `ownLevelsToRestore` in the same step that numbers it (`reduceChange`), and the report
    that follows settles through the `seq` rules; before, the levels kept at the `config` were
    written over the newer change and sent.
  - RED: 1 of 456 failed against the T7b-fix code (`anEditMadeBeforeTheLinksMixArrivesStandsAndNoOlderLevelsAreRestored`:
    the restore was still waiting after the edit; with that check set aside, the answer to the edit
    came back as `[80, 60, 60, 70, 90]`, the edit to 55 undone, and was numbered to be sent).
  - GREEN: `:app:testDebugUnitTest assembleDebug --rerun-tasks` ok, 456 tests, 0 failures, 0 errors
    (455 before: 1 new); no compiler warnings.
  - Commit: this commit (`fix(android): let a new edit cancel a pending lift restore`).

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
- Review `review-150044cc10868604` over 8ef33ad..85e4409 (T2b) approved and acknowledged; next
  review base 85e4409. Two SUGGESTIONs folded into T4:
  - R3-concurrency-test-not-discriminating: the two-edits test also passed against the old
    order (Windows hands the lock over in arrival order); force the interleaving with a hook
    that parks one edit between apply and save.
  - R3-format-groups-set-unguarded-save-error: `formatGroupsSet` can print a double period or
    "not saved: null."; guard both.

- T3a done (channel metadata in the engine, protocol and desktop); see its entry for evidence.
  Follow-ups: an older phone shows a pair's sides apart and the server keeps them equal; the Mix
  screen still labels strips by number and shows hidden inputs and both sides of a pair (T4 uses
  `channelLabel`, drops hidden inputs and collapses pairs with `balanceToPans`). The earlier
  follow-up (a switch dropping details the session keeps) is resolved by T3a-fix in the engine.
- Review `review-47e2a093d93eabca` over 85e4409..42009e6 (T3a) approved and acknowledged; next
  review base 42009e6. Both findings (R3-switch-drops-details-session-diverges,
  R3-checkchannels-missing-name-typeerror) are fixed by T3a-fix. Follow-ups left by the fix: the
  fake bridge (`fake-bridge.js`, outside the fix's edit surfaces) still fits its running details on
  a switch and so drops them, and its test "a device switch keeps the fake details of the inputs
  that remain and drops the rest" pins that; T4 should keep the fake's whole table and fit it only
  when showing, and flip that test to "restores". The desktop event docs (`events.rs`) still say a
  switch "drops" details (true of what is shown). Linking a pair whose second side the device
  lacks pans its first input hard left while it shows alone.

- T3b done (channel metadata on Android); see its entry for evidence. Follow-ups: live check on
  the Galaxy S23 against the engine (names, icons and long-name ellipsis on the 104 dp strips and
  the "My channel" button, a pair's balance and its stereo mark, a hidden input gone, a mid-session
  rename, hide and link from the desktop); with every channel hidden the empty row and the picker
  still say they are waiting for the server's channels; an older phone still shows a pair's sides
  apart (T3a); T7's adopt-after-`config` follow-up still applies (a `config` sends nothing).

- Review `review-ab49ad766e14ed1c` over 42009e6..879dd8b (T3a-fix and T3b) approved and
  acknowledged; next review base 879dd8b. One WARNING folded into T7 (which reworks the phone's
  handling of `config`): R3-config-pairs-musician-channel-moreofme-on-unsent — with More of me
  on, a `config` that makes the musician's channel the second side of a pair moves My channel to
  the first side and expects both sides lifted, but nothing is sent, so the server keeps the old
  one-sided lift and the following `mix_state` can trip the echo check. Send the lifted mix (or
  turn More of me off with a notice) and test it.
- Live (2026-10-15): the new phone app (T3b) was installed on the Galaxy S23 and the desktop app
  rebuilt with T1-T3; channel and group editing awaits T4's screen.

- T4 done (Channels and groups screen, Mix names and pairs, T2b suggestions); see its entry for
  evidence. Follow-ups: rebuild the desktop app and check T4 live (deferred while the app ran);
  the group list the window edits from after an answer is that answer's, so T7's session load must
  reset it (channel edits read the whole table afresh since T4b); index.html could link
  `channels-screen.css` directly (it is imported by `shell.css`); pairing two inputs in different
  groups leaves the second side in its own group (the strip and the card follow the first side),
  so linking could offer to join them; a pending edit is dropped on a layout change, and a
  `set_mix` already in flight with the old group positions can still land after the regroup (last
  writer wins, as for phones); the fake bridge never reports a failed save, so the "not saved"
  wording is covered by the format tests only. Resolved by T4b: a first channel edit built on the
  status's part of the table (now `channel_table`), and the `events.rs` wording.
- Review `review-7abd4e4333f80dbb` over 879dd8b..5cabdf2 (T4) approved and acknowledged; next
  review base 5cabdf2. Its findings (R3-first-channel-edit-from-device-fitted-status,
  R3-kept-groups-roundtrip-shape-unproved, R3-checkbox-reverts-while-edit-in-flight) are fixed by
  T4b.

- Review `review-8f6510a3d9fdca0a` over 5cabdf2..0408441 (T4b) approved and acknowledged; next
  review base 0408441. Folded into T7 (session work):
  - WARNING R3-idle-channel-edit-rewrites-unknown-kinds: with no engine, `read_channel_table`
    returns the session's table with unknown kinds already turned into `other`, and the window
    sends it back whole, so an idle edit permanently rewrites a kind written by a newer build on
    every entry. Keep the stored kind string through the round trip unless the engineer changes
    that entry, and test it.
  - SUGGESTION R3-screen-pending-hold-wiring-untested: add a screen-level test with a deferred
    fake answer proving a clicked Shown or group box stays as clicked until the answer.

- T5a done (engineer limit and locks in the engine, the protocol and the desktop); see its entry
  for evidence. Follow-ups: a musician whose mix is created afresh (an engine restart, or back after
  the 15 s grace period) gets their locks and limit at once, but the locked channels start at the
  neutral values and the phone cannot set them, so until T7's session load restores saved mixes the
  engineer sets them again on the desk; an earlier build that saves the session drops the
  `engineer` field; controls set with no engine are not paired (the engine locks a pair whole when
  it applies them); the engine keeps the controls by IP for configured targets, so a phone
  that registers at such an IP without controls of its own inherits them, as it inherits the
  address's label. Rebuild the desktop app and check T5a live (deferred while the app ran).
- T5a was reviewed in two parts because the whole slice exceeded the reviewer's lens context
  budget: the engine part 0408441..d15c36f (`review-0ea360ebfcf62163`) and the desktop part
  d15c36f..bd0d75b (`review-dc5567254620fe48`), both approved and acknowledged; next review base
  bd0d75b. Their findings (R3-reused-row-inherits-previous-occupants-locks,
  R3-register-restore-not-announced, R3-idle-controls-shadowed-by-phone-key,
  R3-master-limit-change-wiring-untested) are fixed by T5a-fix.

- Review `review-722972c3e330801a` over bd0d75b..c248e3b (T5a-fix) approved and acknowledged; next
  review base c248e3b. One SUGGESTION folded into T6 (which extends the fake bridge):
  R3-fake-idle-validation-diverges — with no engine the fake accepts addresses the desktop's
  `SocketAddr` parse rejects (a host name and port) and keys an empty-string `clientId` under the
  address while the desktop keys `Some("")` under the phone; make the desktop treat an empty
  client id as none and make the fake validate exactly like the desktop.

- T5b done (engineer limit and locks on Android); see its entry for evidence. Follow-ups: live
  check on the Galaxy S23 against the engine (lock and limit from the desktop while the phone moves
  the locked fader, the registration `mix_state`, the Locked tag and the limit line on the 390 dp
  Master card); with the musician's channel locked More of me stays off and the levels it lowered
  stay as the server plays them until the musician moves them back (as after a desktop change).
- Review `review-34d9af3b149c510c` over c248e3b..71b9aff (T5b) approved and acknowledged; both
  WARNINGs (R3-recall-and-lift-split-half-locked-pair, R3-limit-only-push-playback-unproved) are
  fixed by T5b-fix.

- T5c done (the audio input reopened and remembered); see its entry for evidence and follow-ups.
- Review `review-a8a8d8e4b1bf18da` over 71b9aff..d379367 (T5c) approved and acknowledged; next
  review base d379367. Its WARNING (R3-read-choice-latched-on-failure) is fixed by T5c-fix.

- Review `review-04bcc805c7f78bcf` over d379367..39f4b32 (T5c-fix and T5b-fix) approved and
  acknowledged; next review base 39f4b32. Folded into T6:
  - WARNING R3-persistent-unreadable-choice-blocks-start: if `audio_settings` fails on every read,
    Start, Try again and the automatic start all refuse with "could not be read" and nothing tells
    the engineer that choosing an input in Network and system gets past it. Say so in the message,
    offer the way out, and test a read that never succeeds.
  - SUGGESTION R3-running-engine-read-failure-untested: test that a failed read with an engine
    already running leaves it running and shows the command error as intended.

- T6a done (presets, copy and mirror in the engine, the protocol, the desktop and the UI, and the
  three items folded into T6); see its entry for evidence and follow-ups. Review base 39f4b32 (the
  last reviewed boundary; 67c3e7e is this document only). The slice is about 5,900 authored changed
  lines in five commits, so it is reviewed commit by commit (each under about 2,000 lines).
- T6a was reviewed in three parts because the whole slice exceeded the reviewer's lens context
  budget: the engine 39f4b32..38317f1 (`review-fa0392e2f0a9b62d`), the desktop session
  38317f1..62d8ddb (`review-bad720d5ff930d4c`) and the UI 62d8ddb..d0dcdfd
  (`review-c8654069e3cf6af8`), all approved and acknowledged; next review base d0dcdfd. Their
  findings are fixed by T6a-fix.
- Review `review-af815f7391283cf6` over d0dcdfd..97dcc8e (T6a-fix) approved and acknowledged; next
  review base 97dcc8e. Both WARNINGs (R3-offline-circle-later-steps-single-key,
  R3-pending-mirror-cleared-only-by-unseen-answer) are fixed by T6a-fix2.

- Review `review-178839bd3ed80301` over 97dcc8e..aaf926b (T6a-fix2) approved and acknowledged;
  next review base aaf926b. One WARNING kept as a known limitation (no further fix round):
  R3-stale-kept-address-joins-musicians — with no engine running, the offline circle check joins
  keys through the `leaderAddress`/`followerAddress` kept when a mirror was set; if an IP later
  belongs to another musician (a new DHCP lease, or two musicians on one host), the desk can
  refuse a valid mirror as a circle until the engine runs. The refusal is shown, nothing wrong is
  saved, and with the engine running the engine's own check decides. Revisit if offline mirror
  editing becomes common; the fake's offline circle check also still compares keys as strings.

- T6b done (a followed mix on Android); see its entry for evidence. Follow-ups: live check on the
  Galaxy S23 with the desktop setting and stopping a mirror (the banner and tags appearing and
  going at once, the leader's changes following live, master, cap and mute still moving the
  phone's output); the leader's name is the one the desk had when the mirror was set (T6a); a
  recalled master volume is refused with the rest of a saved mix while following.
- Review `review-59d6a66ac3a8d487` over aaf926b..f7cc192 (T6b) approved and acknowledged; next
  review base f7cc192. Its one SUGGESTION (the Compose `enabled` wiring of followed strips has no
  test of its own) is covered by the previews and the live check; no fix round.

- T7 split into T7a (desktop) and T7b (Android). T7a done (named sessions saved, listed, loaded,
  renamed and deleted; a load applies live and pushes the saved mixes to the phones here; the
  folded desktop items); see its entry for evidence and follow-ups. Folded items resolved: the T4
  follow-up (the window's group list and waiting edits after a load), WARNING
  R3-idle-channel-edit-rewrites-unknown-kinds, SUGGESTION R3-screen-pending-hold-wiring-untested,
  the session notice shown in the UI, and the fake's offline mirror check comparing keys as the
  desktop does. Still open for T7b: R3-config-pairs-musician-channel-moreofme-on-unsent (Android).
  Review base f7cc192; the T7a slice is four commits of about 1,390, 1,240, 1,970 and 640 changed
  lines, so it is reviewed commit by commit.
- T7a was reviewed in two parts because the whole slice exceeded the reviewer's lens context
  budget: the backend f7cc192..abcea45 (`review-0c47b67a0faa9250`) and the UI and fixes
  abcea45..34012d6 (`review-65ac6b51cab567d0`), both approved and acknowledged; next review base
  34012d6. Their findings (R3-refused-mix-untested, R3-refused-load-drops-waiting-edits,
  R3-explicit-other-cannot-clear-unknown-kind) and the engine gap reported with T7a (no way to
  forget kept controls or mirrors) are fixed by T7a-fix.
- Review `review-386947d3e30791b9` over 34012d6..2b4ba22 (T7a-fix) approved and acknowledged; next
  review base 2b4ba22. Both WARNINGs (R3-replace-kept-not-atomic,
  R3-channel-table-stale-session-kind) are fixed by T7a-fix2.

- T7b done (the phone takes the server's mix after the first `config`; More of me goes off when a
  link pairs the musician's channel); see its entry for evidence. Phase 2's tasks are all done and
  every acceptance criterion is verified by tests (see Acceptance criteria); the live test with
  the Volt and the Galaxy S23 is pending for all of them. Live checklist from the follow-ups: channel
  names, icons, hidden inputs and pairs on the phone; group edits; limits, locks and mirrors set
  and stopped from the desk; a session saved and loaded with phones connected; a first connect to
  a server that already holds the musician's mix (the phone shows it, no neutral strips).
- Review `review-c9bf544c067c5dbb` over 2b4ba22..05f3a6a (T7a-fix2 and T7b) approved and
  acknowledged; next review base 05f3a6a. Its WARNING (R3-paired-moreofme-off-keeps-server-lift)
  is fixed by T7b-fix; its two desktop SUGGESTIONs go to a desktop follow-up commit.
- Review `review-4c6fd738e50cbb30` over 05f3a6a..44b5572 approved and acknowledged; next review
  base 44b5572. Its WARNING (R3-own-levels-stale-after-meanwhile-edit) is fixed by T7b-fix2.

- Review `review-c0bc7cb0513ac7d0` over 44b5572..ff2b413 (T7b-fix2) approved with no findings and
  acknowledged. Every commit of phase 2 is now reviewed (last reviewed boundary ff2b413).

## Next step

Live test of phase 2, then PRs (stacked-to-main) on the user's approval.
