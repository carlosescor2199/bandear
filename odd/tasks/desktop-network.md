# Desktop redesign, phase 4: network and system

## Objective

Make the show reliable and visible: a link health history per musician with a chart, the phone's
playback buffer set from the desk, packet redundancy, a log file, a demo source and an automatic
backup of the musicians' mixes.

## Problem

- Only the latest link report per musician is kept (`apps/server/src/health.rs:136`); there is no
  history, no round-trip time, and the phone's jitter, backlog and cushion never leave the phone.
- Each phone adapts its own playback cushion (1 to 4 packets, `PlaybackLatency.kt:15-77`); the
  engineer cannot set it.
- A lost UDP packet is a dropout; nothing is sent twice.
- The release desktop has no console (`main.rs:2`), and there is no log crate, so about 102
  `println!`/`eprintln!` diagnostics are lost in production.
- Without an interface there is no way to run or show the app.
- Musicians' live mixes are only kept in named snapshots saved by hand; a crash loses every change
  since the last save.

## Why

PRD RF5 (per-client loss) and RF11 (stability indicator and buffer adjustment), the 3-hour
no-dropout goal and "warn about bad networks" (`PRD.md:38,52,57,70`). Phase plan in
`odd/tasks/desktop-console.md:43-44`.

## Design

Design canvas "MixLink desktop redesign" (https://claude.ai/artifact/GcjxqS8ME4eBYVSzxjbQtB),
board "Network and system".

## Design decisions

- **Buffer from the desk (user decision, 2026-10-19)**: per musician, "Auto" (today's adaptive
  cushion) or a fixed value in milliseconds that the phone applies without adapting. The phone
  converts milliseconds to packets at its block size and clamps to what it can play.
- **Capabilities**: a phone announces what it supports in an optional `capabilities` list on
  `register`; old servers ignore unknown fields. The server only sends new message types to phones
  that announced them, and the desk shows "needs a newer app" otherwise. Old phones keep working.
- **Redundancy (user decision, 2026-10-19: "immediate copy + optional delay")**: the same datagram
  is sent twice, per musician on/off, double bandwidth for that musician. By default the copy goes
  right after its original (delay 0): no added latency, and every phone takes it (a lost
  original's copy arrives before the next packet and plays in order; a second copy is dropped as a
  duplicate), which covers isolated losses. Optionally the copy goes 1-4 blocks later, so a Wi-Fi
  burst must last that long to take both: only for a phone that announces `redundancy.hold`, which
  the server tells the delay (`{"type":"redundancy","delay_blocks":N}`) and which then holds packets
  back that many blocks so a late copy fills its gap, at that much latency for that musician; the
  desk refuses a delay for any other phone ("needs a newer app"). The PMON wire format stays
  byte-identical. Delayed copies are held in a store preallocated off the engine thread, immediate
  ones are the datagram just sent; the network sink keeps its no-allocation guarantee.
- **Log file**: an own small writer, no new crate. Server and desktop lines go through one sink to
  `<app log dir>/mixlink.log` with size-based rotation (a few files of a few MB) and still to
  stderr in development. A line is timestamped and tagged by area.
- **Mixes backup**: musicians' live mixes are autosaved (debounced) to a rolling backup with a
  fixed number of copies, written atomically. Restoring is the engineer's choice from the session
  panel; a phone's own opening mix on reconnect is not overridden automatically.
- **Demo source**: a generated signal (per-channel tones/noise/clicks at -18 dBFS) through the same
  reader-and-timer path the virtual soundcheck uses, selectable as an audio "device" so the engine
  runs with no interface. Never the default when a real interface exists.
- **Health history**: a ring per musician of 1-second samples (loss, underruns, timeouts, RTT from
  the control channel's ping/pong, and the phone's jitter and cushion when it reports them),
  bounded in memory, shown as a chart on the Network and system screen.

## Constraints

- Clean-room: never copy StageWave code, UI, name or assets.
- Old phones and old servers keep working (no PMON header change, no required field).
- ASIO is never chosen by default and never enumerated implicitly.
- The engine thread and audio callbacks never allocate, lock or panic.
- **Build safety**: `CARGO_BUILD_JOBS=2`, gradle `--max-workers=2`, one build at a time, never
  `cargo clean`, stop and report on a rustc crash or a corrupt generated file.

## Scope

All six items above, the Android changes they need (capabilities, stats extras, fixed buffer,
optional recovered-packet count), the Network and system screen, the PRD update and a live test.

## Acceptance criteria

- A musician's link history (at least the last 10 minutes) is visible as a chart; RTT is shown.
- Setting a fixed buffer on the desk changes that phone's cushion within a second; Auto restores
  adaptation; an old phone shows "needs a newer app".
- With redundancy on, a lossy link shows recovered packets instead of dropouts; bandwidth doubles
  only for that musician; an old phone still plays.
- A log file exists in the app's log dir, rotates, and records engine, network and session events.
- The app runs and streams to phones with the demo source and no interface.
- After a forced crash, the last musicians' mixes can be restored from the backup.

## Tasks

- [x] T1 (log file): log sink in the server crate, file writer with rotation in the desktop,
  existing `println!`/`eprintln!` routed through it. Route: delegated writer (trigger: 2+
  non-trivial files, server facade plus desktop file sink and about 70 call sites).
  - API: `personal_monitoring::logging` (`Level`, `LogSink`, `set_sink`, `flush`, `format_line`,
    `set_max_level`) and the macros `log_error!/log_warn!/log_info!/log_debug!(area, ...)`. Line:
    `2026-10-18T19:04:05.123Z WARN  [control] message` (UTC, RFC 3339 to the millisecond; line
    breaks in a message written as `\n`). Stderr until a host installs a sink; debug lines dropped
    in release builds.
  - Desktop: `logfile.rs` installs `<app_log_dir>/mixlink.log` first thing in `setup`, rotated at
    5 MB keeping `mixlink.log.1..5`, created lazily, one write per line with no own buffer (the
    exit path in `on_run_event` also flushes), a failed line goes to stderr (explained once,
    counted, retried on the next line), mirrored to stderr in debug builds. Startup line with app
    version, OS/arch and log file; Tauri command `log_location` answers `{ directory, file }`.
  - Routed: 25 server lines (capture 2, control 10, discovery 5, engine 8) and 47 desktop lines
    (the `[mixlink-desktop]` prefix replaced by the areas `desk`, `session`, `app`, `events`,
    `soundcheck`, `recording`, `local`).
  - Left out on purpose: the UDP send error in `NetworkSink::process` (engine thread) is no longer
    printed at all, the per-musician `discarded` counter already reports it; the cpal error
    callback in `capture.rs` `open_typed` keeps its `eprintln!` (it may run on the driver's audio
    thread); the CLI's two `main.rs` lines stay `println!` (its user interface); test-only prints
    and the `examples/` probes are unchanged.
  - Real-time check: test `logging::tests::nothing_logs_on_a_real_time_path` reads the production
    source of `pipeline.rs`, `output.rs`, every `impl Sink for` in `network.rs` and `recorder.rs`,
    and `capture.rs` `open_typed`, and fails on any log call (and on any print except the capture
    error callback's).
  - RED: server, with `format_line` stubbed to an empty string, 4 of 6 logging tests failed (the
    three format tests and the real-time check, which caught `eprintln!` in the `NetworkSink`);
    desktop, with `append` stubbed to write nothing, 7 of 7 logfile tests failed. GREEN below.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 440 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 257 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings.
  - Commits: 72f857b `feat(engine): route diagnostics through a log sink`; this commit
    `feat(desktop): write a rotating log file`.
- [x] T1-fix (review-b891debe9d10ebc9 findings): Route: delegated writer (trigger: findings across
  the desktop log file and the server's network sink, done with T2).
  - WARNING fixed: a rotation that fails for a lasting reason (old file held or not removable) now
    appends the line to the current file anyway and backs off until the file has grown by another
    limit's worth (`retry_rotation_at`), instead of losing every line and retrying on each. Seam:
    `FileOps` (remove/rename) injected in tests.
  - SUGGESTION fixed: the stderr explanation is re-armed after a line reaches the file again;
    `log_location` answers `failedLines` (live count shared with the sink).
  - SUGGESTION fixed: the network sink keeps the kind of the last failed UDP send in an `AtomicU8`
    per musician (no allocation on the engine thread); `MusicianCounters.last_send_error` and the
    window's `lastSendError` name it. The no-allocation test stays green.
  - RED: logfile, 3 of 11 failed (rotation-fails and remove-fails lost lines, `warned` not
    re-armed); network, `a_failed_send_is_remembered_per_musician` failed (kind not kept).
  - Commits: 8eb4f3d `fix(desktop): keep logging when the log file cannot rotate` (+226/-31);
    e645bf4 `fix(engine): report the last send error per musician` (+132/-4).
- [x] T2 (mixes backup): debounced autosave of live mixes to a rolling backup, restore from the
  session panel. Route: delegated writer (trigger: 2+ non-trivial files, Rust store, scheduler,
  commands and the console UI).
  - Files: `backups/mixes-<UTC stamp>.json` (stamp `20261018T190405123Z`) in the app config dir,
    version 1, `{ version, savedAt, musicians: [{ key, name, clientId, address, engineer?, mix }] }`
    (`mix` as a saved session keeps one), written atomically (`write_atomically`), the newest 10
    kept. A damaged or newer file is listed with its `problem` and never restored. A stamp is read
    only in its exact form and the file must be directly in the folder (canonicalized).
  - When: the event pump notes each musician `mix-changed` (`PumpSource::mix_backup`); a backup is
    due 3 s after the mixes go still, at most 20 s after the first change (`Debounce`, injected
    clock), read under the engine lock (in memory) and written by its own thread; also on engine
    stop and app exit. Empty or unchanged backups are not written. Limit and locks are recorded,
    not restored (the session keeps them).
  - Commands: `list_mix_backups()` (newest first: stamp, savedAt, musicians, names, problem) and
    `restore_mix_backup(stamp)`, through `push_saved_mixes` (the load path): restored rows, and
    skipped `notConnected` / `notHere` / `refused`, never queued; no engine is an error.
  - Console: state `mixBackups`, `loadMixBackups()` and `restoreMixBackup(stamp)` answering `{ ok,
    message }`; fake bridge mirrors both (debounced by `clock`, keep 10, dedupe). Session panel:
    "Mix backups" list (time to the second, musicians and names), Refresh, Restore with a confirm
    step and the outcome; logic in `lib/mix-backups-view.js`.
  - RED: Rust store 7 of 7 failed (stubs), scheduler and backup-of 4 failed; UI suite failed to load
    (module missing). GREEN below.
  - Browser check (`desktop-ui`, `?fake#/network`): the demo's phone changes produced backups
    listed with time and 4 musicians; Restore asked first, Cancel dropped it, the confirming Restore
    said "Restored the mixes backed up at ... to Lead vocal, Bass, Keys and Drums."; no console
    errors.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 442 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 277 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 470 passed.
  - Commits: 67fba08 `feat(desktop): back up the musicians' mixes as they change` (+1380/-3);
    this commit `feat(desktop-ui): restore a mix backup from the session panel`.
- [x] T2-fix (review-8872f9deff0e3f2d WARNING): a backup whose old copies could not be pruned was
  reported as failed, so the writer never recorded it and rewrote every identical backup while
  pruning kept failing. `BackupStore::write` now answers `Written { stamp, prune_problem }`: the
  backup counts as written, the writer warns once per problem (info when pruning works again) and
  the next write drops the surplus. Seam: an injected remover (`with_remover`). Route: delegated
  writer (with T3).
  - RED: 2 of 16 backup tests failed (store returned the prune error; the writer rewrote the same
    backup: 4 files instead of 2). GREEN: 16 of 16, desktop 279 passed.
  - Commit: 6bd3e86 `fix(desktop): count a backup as written when pruning fails` (+163/-15).
- [x] T3 (health history): capabilities on `register`, RTT from ping/pong, history ring per
  musician, phone stats extras (jitter, cushion) parsed when present, DTO and events. Route:
  delegated writer (trigger: 2+ non-trivial files across protocol, registry, control server,
  engine status, desktop DTOs, a new command and the fake bridge).
  - Protocol (for T5, documented in `protocol.rs`): `register` takes an optional
    `capabilities: [string]`; names kept are 1-32 of `[a-z0-9._-]`, at most 16, each once,
    unknown names kept, anything unreadable dropped (never an error). Constants
    `CAPABILITY_STATS_V2 = "stats.v2"` (stats carry the extras), `CAPABILITY_PLAYBACK_BUFFER =
    "playback.buffer"` (T6), `CAPABILITY_REDUNDANCY_RECOVERED = "redundancy.recovered"` (T7). Each
    registration replaces the musician's list. `stats` takes optional `jitter_ms` (interarrival
    jitter over the interval), `cushion_ms` (the cushion it aims for, in milliseconds, not
    packets, so phones with different block sizes read on one scale), `backlog_ms` (audio queued at
    the end of the interval) and `recovered` (packets only a redundant copy brought in time; they
    count in `packets_received`, not in `packets_lost`); a value out of range (a duration outside
    0-60000 ms, a count that is not an unsigned integer) is read as absent.
  - RTT: each heartbeat ping (every 2 s) carries an 8-byte number (`PingClock`); the pong that
    echoes it gives the round trip, kept as the last value and an EWMA (weight 1/8, as TCP's
    SRTT); a pong for an older ping measures nothing. Any phone has one (WebSocket libraries answer
    pings themselves). Never on the audio paths.
  - History (`apps/server/src/link_history.rs`): per musician a `VecDeque` of 600 one-second samples
    allocated once (`HISTORY_SAMPLES`). Sample: `at_ms` (Unix ms, UTC), `gap`, `reconnected`,
    `health`, `interval_ms`, `packets_received`, `packets_lost`, `loss_percent`, `underruns`,
    `receive_timeouts`, `rtt_ms`, `jitter_ms`, `cushion_ms`, `backlog_ms`, `recovered`. A second
    with no report while the session runs becomes a gap sample once the next report is 2 s late
    (the control accept loop's tick, about 10 a second, or the late report itself); no gaps before
    a session's first report or after its last connection closed, so a phone that never reports
    has no history. A reconnect keeps the history and marks the next sample `reconnected`: within
    the grace period, from a new IP with the client id, and after the grace period by client id
    (up to 32 departed phones kept). A different phone taking a row over gets its own. An engine
    restart starts every history empty.
  - Status: `LinkStatus` gains `round_trip` and `latest` (newest sample); the musician DTO gains
    `capabilities`, `rttMs` (smoothed), `rttLastMs` and `linkLatest`; existing fields unchanged.
    `EngineHandle::link_history(key, since_ms)`.
  - Desktop: command `link_history(musicianKey, sinceMs?)` (`client:<id>` / `address:<ip:port>`)
    answers `{ musicianKey, samples: [LinkSampleDto], maxSamples: 600, periodMs: 1000 }`, samples
    after `sinceMs`, oldest first; an unreadable key, nobody and no engine are errors. Bridge
    `linkHistory(musicianKey, sinceMs = null)`; the fake bridge generates deterministic histories
    (`syntheticLinkHistory`, `syntheticRoundTrip`): good, warning with one reconnect every ten
    minutes, lost with the last 20 s as gaps, none for a phone that never reported; Lead vocal and
    Bass announce `stats.v2`.
  - RED: protocol and control 6 of 450 failed with stub readers (capabilities, extras); link
    history 12 of 12 failed with stub bodies; the wiring tests 3 of 5 failed with the status copy,
    the client-id return and the pong recording disabled; desktop 4 of 4 failed with the command and
    the row fields stubbed; UI link-history suite 3 of 7 failed on its first run (row defaults
    overrode the capabilities, the reconnect marker moved with the clock), fixed. GREEN below.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 467 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 284 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 477 passed.
  - Commits: 46876b9 `feat(server): keep the capabilities a phone announces` (+311/-1); 01aed61
    `feat(server): keep a link history with round-trip time` (+1181/-7); 0d689e1
    `feat(desktop): serve a musician's link history`.
- [x] T3-fix (review-6419b655bb738c08 findings). Route: delegated writer (trigger: findings across
  the history, the registry, the control server, the desktop command, DTO, bridge and fake; done
  with T4).
  - WARNING fixed: gaps, order and the read cursor ran on the wall clock, so a clock stepped back
    starved `sinceMs` readers and one stepped forward filled the ring with gaps. The history now
    runs on the monotonic clock (`LinkMoment { at: Instant, wall_ms }`): each sample has `seq`
    (1, 2, ... per history, never reused) and `mono_ms` (since the history began); `at_ms` is for
    display only, a gap's read back from the wall clock in force. A tick adds at most the seconds
    that really passed (and at most the ring). Each history has a process-unique `history_id`
    (`LinkHistoryRead { history_id, samples }`), so a reader tells an engine restart or another
    phone on the row from the history it follows. Command `link_history(musicianKey, sinceSeq?)`
    answers `{ musicianKey, historyId, samples, maxSamples, periodMs }`, each sample with `seq` and
    `monoMs` first; bridge `linkHistory(musicianKey, sinceSeq = null)`; the fake numbers a sample by
    its second, uses the clock as `monoMs` and gives each musician a `historyId` that an engine
    start replaces. `EngineHandle::link_history(key, since_seq)`.
  - SUGGESTION fixed: a report outside a session (a configured-target phone that never registers)
    starts one, marked as a reconnect after earlier samples; a closed connection
    (`end_session`) drops the round trip, so the old one is never carried into the next session
    while a pong measured on the new connection before its first report is kept.
  - SUGGESTION fixed: control tests `a_connected_phone_that_stops_reporting_gets_gaps_from_the_
    accept_loop` and `a_closed_connection_stops_the_gaps` (they cover the tick and the PeerGuard
    path; green on their first run).
  - RED: link history 3 of 19 failed with the old wall-clock gap logic, the old implicit session
    and the old end of session (step forward, real seconds only, configured-target reconnect); the
    step-back test already passed there, the `seq` cursor being in place. UI link-history suite 4 of
    8 failed before the bridge and fake changes (`sinceSeq`, `seq`/`monoMs`, `historyId`).
  - Commit: e04e0b1 `fix(server): order the link history on a monotonic clock` (+607/-177).
- [x] T4 (health chart): chart on the Network and system screen. Route: delegated writer
  (trigger: 2+ non-trivial files, the view logic, a new chart element, the table and the screen).
  - Pick a musician (History in the Link cell, or a click on their row outside its fields) to open
    `<ml-link-health>` below the table; again, or Close, closes it, and it goes when the musician
    leaves the list. Range buttons 1 / 5 / 10 min (10 by default) in one row above the chart; a
    summary line with the level (`ml-link-state`, mark and word, as the table), the loss over the
    window, the round trip now and on average, the underruns, and when any the timeouts, seconds
    without reports and reconnects.
  - Chart (SVG, no package): small multiples on one time axis, each with its own scale: Loss (%),
    Round trip (ms), then Jitter and Cushion (ms) for a phone that announces `stats.v2` or reports
    them ("Jitter and cushion: not reported by this phone (it needs a newer app)." otherwise).
    Lines (2px, 1.25px past a second every two pixels) break at gaps, missing values, missing
    seconds and reconnects; a second alone is a dot. Gaps are shaded bands; reconnects a dashed
    vertical marker labelled "Reconnected" (turned leftwards near the edge, not labelled twice
    within 90px); underruns and timeouts tick marks in their own lanes. The axis counts back
    (−10 min ... or −60 s ...) and ends at the newest second's local time. One series colour
    (`--ml-blue`, contrast >= 3:1 on the surface by the dataviz validator); status colours only on
    the level mark, with its word. Dark only, as the app.
  - Hover and keyboard (arrows, Page, Home/End, Escape): crosshair on the nearest second and a
    readout (values first, its local time, a note for a gap or a reconnect). Text alternative: the
    summary as the chart's description, and "Show as table" (newest first, every value).
  - Polling: `createHistoryFollower` reads the whole history once, then every second only after the
    newest `seq` held, appended up to the ring (600); a new `historyId` reads it whole again; a
    failed read is said and retried; an answer for a musician no longer followed is dropped. Console
    app `linkHistory(musicianKey, sinceSeq)`.
  - Logic in `lib/link-health-view.js`; RED: the suite failed to load (module missing), then 23 of
    23 passed; one more test (the level's word from `linkState`) came with a change found in the
    browser.
  - Browser check (`desktop-ui`, `?fake#/network`): Bass (10 min) showed four panels, the
    reconnect marker and its label, the underrun ticks; hover showed the crosshair and the readout
    ("Warning 23:36:40, Loss 0.50%, Round trip 20.1 ms, ..."); Drums (1 min) showed the last 20 s
    shaded with the line broken before them and the extras note; Lead vocal (5 min) opened by a
    row click, the table showed 300 rows newest first; arrows moved the readout, Close closed the
    panel and unpressed History. Fixed from the screenshots: the range group stretched full width,
    the crosshair looked like the reconnect line, dense lines, the level word ("Poor" vs the
    table's "Link lost"). No console errors.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 474 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 284 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 502 passed.
  - Commits: 95f1769 `feat(desktop-ui): link health view logic` (+1114); this commit
    `feat(desktop-ui): link health chart on the network screen`.
  - Not built from the design board: the phone buffer and redundancy columns (T6, T7), demo mode
    (T8), log viewer and export/restore of mixes from a file.
- [x] T4-fix (review-e6137cfb0b4da4c8 findings). Route: delegated writer (trigger: findings across
  the network screen, the link health logic and the control server tests; done with T5).
  - WARNING fixed: the screen's link health wiring moved into a DOM-free controller,
    `lib/link-health-controller.js` (`createLinkHealthController({ read, onChange, schedule, cancel
    })`: `select(musicians, address)`, `view(musicians)` answering `{ selected, data }`, `setRange`,
    `showTable`, `close`, `dispose`); the screen only forwards events and draws. Node tests: picking
    the open musician again closes the panel and stops the follower, another one is followed
    instead, the panel closes and the follower stops when the musician leaves the list or the
    engine stops (nobody listed), a failed read reaches the panel and the next good read clears it,
    range and table, unmounting stops the follower and drops a late answer.
  - SUGGESTION fixed: `followedMusician(musicians, key, address)` (link-health-view.js): when an
    `address:` key is gone, the row at the same address that now has a client id is the same phone,
    so a phone that registers keeps its panel open and is followed by its client id (the history
    held stays shown until the new key's read); a phone that already had a client id is never
    followed to another at its old address.
  - SUGGESTION fixed: `a_closed_connection_stops_the_gaps` drives the tick by hand
    (`fill_link_gaps` with a moment seconds ahead): the same tick keeps gaps while connected and none
    a minute ahead once the connection closed; no fixed sleep (the test went from about 3 s to
    0.01 s).
  - RED: UI suite failed to load (controller module missing); with the guard's `end_link_session`
    disabled, the server test failed (60 samples, 55 of them gaps, against 5). GREEN below.
  - Browser check (`desktop-ui`, `?fake#/network`): History opened Bass (600 samples), History again
    closed the panel and unpressed it, Drums opened, 1 min set the range, Close closed it; no console
    errors.
  - Commits: 769ea43 `fix(desktop-ui): test the link health wiring and follow a musician who
    registers` (+359/-61); 90b9cfc `test(server): prove gaps stop after a close without a fixed
    sleep` (+25/-4).
- [x] T5 (Android): announce capabilities, report jitter, cushion and backlog in `stats`. Route:
  delegated writer (trigger: 2+ non-trivial files, the control channel, the link stats, the player
  and their tests).
  - Capabilities: `CLIENT_CAPABILITIES = listOf(CAPABILITY_STATS_V2)` in `ControlChannel.kt`, sent
    as `capabilities` in every registration (first open, reconnect, rename); T6 and T7 add their
    name by one line. An empty list puts no key.
  - Stats extras (`stats.v2`), measured on the receive thread over each report interval:
    `jitter_ms` the mean of each packet's spacing distance from the smoothed spacing (the deviation
    the local jitter already smooths, `LinkStatsAccumulator.onArrivalDeviation`), absent with no two
    packets in the interval; `cushion_ms` the buffer asked of the track
    (`PlaybackLatency.cushionMs`: cushion packets times packet frames at the track's rate), absent
    before the first packet; `backlog_ms` the audio queued when the interval ends (track timestamp
    read at the poll). Hand-off: the existing once-a-second `LinkStatsReport` carries them as
    primitive doubles (`NaN` = not measured, nothing boxed) to `ControlChannel.sendLinkStats`, which
    builds the JSON on the control scheduler; per packet only two primitive fields change, no
    allocation or lock. `recovered` is left for T7.
  - Wire: extras rounded to 0.01 ms; a value not measured or outside 0-60 000 ms is left out.
  - Old servers: main before phase 4 (4f2ae0c) has no `deny_unknown_fields` in `protocol.rs`, and its
    tests `an_address_in_a_registration_is_ignored_because_there_is_no_such_field` and
    `parses_a_stats_report_with_its_interval_and_counts` (an unknown `battery`) prove unknown fields
    in `register` and `stats` are ignored.
  - RED (stub API: no capabilities, extras not measured or sent, cushion 0):
    `gradle -p apps/android :app:testDebugUnitTest --max-workers=2 --no-daemon`: 467 tests, 11
    failed (2 stats message, 3 link stats, 2 cushion, 4 registration); the 3 that passed (no extras
    without measures, names valid, an empty list) are kept as guards. GREEN below.
  - Build: no Gradle wrapper; Gradle 9.6.0 from `~/.gradle/wrapper/dists` with `JAVA_HOME` set to
    Android Studio's JBR and `ANDROID_HOME` to the local SDK, `--max-workers=2 --no-daemon` (the user
    approved Android builds with this limit); no Java process left after.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 474 passed;
    `node --test apps/desktop/ui/tests`: 512 passed;
    `gradle -p apps/android :app:testDebugUnitTest --max-workers=2 --no-daemon`: 467 tests, 0
    failures (456 before, 11 new); `gradle -p apps/android :app:assembleDebug --max-workers=2
    --no-daemon`: BUILD SUCCESSFUL, APK `apps/android/app/build/outputs/apk/debug/app-debug.apk`
    (not installed).
  - Commit: this commit `feat(android): announce capabilities and report jitter, cushion and
    backlog`.
- [x] T5-fix (review-d62775e702018934 SUGGESTION): the receive loop's wiring of the stats extras
  (backlog from the track timestamp, NaN without a track or a timestamp; rate 0 while unset; cushion
  from packet frames times cushion packets) was untested. It is now the pure
  `PlaybackLatency.reportExtras(packetFrames, cushionPackets, playbackRate, timestampRead,
  framesWritten, presentedFrame)` the loop calls; 5 tests, distinct packet frames and cushion
  packets, and the asymmetric cases that catch a swap (before the first packet: no cushion; frames
  written behind the presented frame: 0 ms). Route: delegated writer (with T6).
  - RED: with the function stubbed to NaN, 3 of 19 PlaybackLatency tests failed (the 2 no-measure
    guards passed). GREEN: 472 tests.
  - Commit: eac9cac `test(android): cover the stats extras taken from the track` (+114/-8).
- [x] T6 (buffer from the desk): server message, desk control per musician, Android applies Auto
  or fixed. Route: delegated writer (trigger: 2+ non-trivial files on every layer: protocol,
  registry, control server, engine, desktop session, command, DTO, console, fake bridge, table,
  screen, Android control channel, player and controller).
  - Wire (documented in `protocol.rs`, `playback_message`): server to phone
    `{"type":"playback","buffer":{"mode":"auto"}}` or `{"type":"playback","buffer":{"mode":"fixed",
    "ms":40}}` (`ms` integer 5-500, `crate::playback::MIN/MAX_FIXED_BUFFER_MS`). Sent only to a phone
    that announced `playback.buffer`: on every registration (Auto included, so the choice survives a
    reconnect and a phone never keeps one an earlier server set) and on every change. No ack: the
    phone's next `stats.cushion_ms` says what it plays. Older phones never see it.
  - Server: `PlaybackBuffer { Auto, Fixed { ms } }` (`playback.rs`, `PlaybackBuffer::fixed` checks
    the range). The registry keeps a buffer book by client id, else IP (Auto kept as nothing; a
    client-id buffer drops the IP's), for the engine's run, whether the musician is here or not.
    `EngineHandle::set_playback_buffer(key, buffer) -> BufferSetting { kept_for, address, outcome }`
    with `BufferOutcome::{Sent, NotConnected, Unsupported}` (an engine stopping or a key naming
    nobody is an error); `replace_playback_buffers(kept)`; event
    `EngineEvent::PlaybackBufferChanged { address }`; the connection's push feed answers `Push::{Mix,
    Buffer, Everything}` (a lag pushes both). `MusicianStatus.playback_buffer`.
  - Desktop: the session keeps an entry's `buffer` (`{ "mode": "fixed", "ms": 40 }`, absent for
    Auto, read leniently, a mode it does not know kept as written; within version 1 like `engineer`
    and `mirror`); every start and every session load hands the readable ones to the engine
    (`replace_playback_buffers`; one it cannot use is logged and skipped). Command
    `set_musician_buffer(musicianKey, mode, ms?)` answers `{ musicianKey, outcome, buffer, saved,
    saveError, message, minMs, maxMs }`, outcome `sent` / `notConnected` (kept, sent when it is back)
    / `unsupported` (needs a newer app, nothing kept) / `invalid` (nothing changed); a key it cannot
    read, nobody and no engine are errors. Saved under the key the engine kept it under. The DTO row
    gains `buffer` and `bufferSupported`. The mix backups do not carry it (they restore mixes; the
    session keeps the setting).
  - UI: a Buffer column on the Network and system musicians table: Auto / Fixed with a ms field
    (Enter or leaving sends, Escape reverts), "plays 42.7 ms" from `linkLatest.cushionMs`, "needs a
    newer app" with no control for a phone that takes none, the outcome said on the row. Logic in
    `lib/playback-buffer.js` (`bufferView`, `bufferRequest`, `formatBufferSet`, `nameForKey`);
    console `setMusicianBuffer(musicianKey, mode, ms)` in turn with desk edits and loads; the fake
    bridge keeps buffers per key across starts and in named sessions and makes the phone's reported
    cushion the fixed length rounded up to 256-frame packets from the second it was set.
  - Android: `playback.buffer` in `CLIENT_CAPABILITIES`; `parsePlaybackBuffer` (anything unusable
    ignored) to `ClientMixState.deskBufferMs` (Auto = 0; reset to Auto at every session start,
    nothing stored). The receive loop's `CushionController` follows it per packet (one volatile
    read): fixed is `ceil(ms * rate / (1000 * packetFrames))` packets, at least 1, at most the whole
    packets the track holds, and the controller stops adapting (underruns are still noted, so they
    never count against Auto); Auto resumes from the cushion played clamped to 1-4 packets with its
    clean seconds counted anew. The track is opened with room for 500 ms in whole packets
    (`trackCapacityFrames`, about 94 KB at 48 kHz) and the working size set down at once, as before.
  - Interval fix: the adaptive cushion was adjusted every 100 packets, which is one second only for
    480-frame blocks (256-frame blocks are about 187 a second, so it adjusted every ~0.53 s). It is
    now time-based: every 1 s of audio played, counted in frames with the remainder carried
    (`adjustIntervalFrames`, `CushionController.onPlayed`).
  - RED: server, with stubs (no range check, empty message, the registry keeping and finding
    nothing, no buffer push), 10 of 486 failed (the in-range check, the mode names and the older
    phone test passed). Desktop, with the session, DTO and command stubbed, 11 of 294 failed. UI: 4
    of 519 failed (the pure `playback-buffer.js` logic was written before its tests ran, so its 3
    suites passed at once). Android, with compile-only stubs, 10 of 482 failed (the "unusable
    message is ignored" guard passed). GREEN below.
  - Browser check (`desktop-ui`, `?fake#/network`): Lead vocal set to Fixed 40 ms showed "plays
    42.7 ms" and "Sent Lead vocal's phone a fixed 40 ms buffer."; 501 ms said "A fixed buffer is from
    5 to 500 ms, not 501 ms." and stayed; back to Auto said "Lead vocal's phone is back on Auto." with
    "plays 40.0 ms"; Bass, Keys and Drums showed "needs a newer app" and no control. No console
    errors.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 486 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 294 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 519 passed; `gradle -p apps/android :app:testDebugUnitTest
    --max-workers=2 --no-daemon`: 482 tests, 0 failures; `:app:assembleDebug`: BUILD SUCCESSFUL, APK
    `apps/android/app/build/outputs/apk/debug/app-debug.apk` (not installed).
  - To confirm in the live test (T9): the latency of the larger track capacity on the S23 (the
    working size is set right after `play()` as before; if the platform refused it, the track could
    queue more than before).
  - Commits: 6afc117 `feat(server): set a phone's playback buffer from the desk` (+939/-44);
    280837e `feat(desktop): keep and send each musician's buffer` (+775/-10); f363377
    `feat(desktop-ui): buffer control per musician` (+664/-12); this commit `feat(android): follow
    the buffer the desk sets`.
- [x] T6-fix (review-1e5f9aa8d353b411 findings). Route: delegated writer (trigger: findings across
  the server control tests, the Android player and the desktop UI; done before T7).
  - Server: `an older phone never gets a buffer` announced the change before the registration was
    visible, so it could pass with the capability check gone; it now waits for a round trip,
    announces the change and checks two more round trips carry no playback message.
  - Android: the track opened with room for 500 ms relied on the platform honouring the smaller
    size set after it; it now sets the one-packet Auto cushion, reads the size applied and keeps the
    large track only when it came down (5 ms tolerance) or no higher than a minimum-size track would
    queue, else reopens at the minimum (fixed buffers clamp to what it holds); `stats.cushion_ms` is
    the size applied. The loop's cushion wiring is the pure `CushionController.onPacket` step, with
    tests.
  - Desktop UI: the Buffer cell settled a typed length against the reported one as strings (a typed
    040 never settled against 40); `pendingBufferChoice` in `playback-buffer.js` compares numbers,
    with tests.
  - RED: recorded only for 64ac183 (its message: with the push path ignoring the capability, the
    test fails); not recorded for 89197ff and 701f3e0.
  - Commits: 64ac183 `test(server): prove an older phone never gets a buffer` (+42/-9); 89197ff
    `fix(android): keep auto latency when the track will not shrink` (+228/-30); 701f3e0
    `fix(desktop-ui): settle the buffer choice by value` (+67/-8).
- [x] T7 (redundancy): delayed duplicate send per musician, desk toggle, recovered count. Route:
  delegated writer (trigger: 2+ non-trivial files on every layer: network sink and target table,
  engine, desktop session, command, DTO, console, fake bridge, table, link health panel, Android
  tracker, link stats and control channel). A first writer was interrupted after the engine commit
  with the desktop session, DTO and command module uncommitted; the second judged them complete
  but unwired (no `mod`, no start or load handoff, no Tauri registration, a test DTO literal
  missing the new fields) and finished them.
  - Engine (5c4294e): per musician `Redundancy::Off | On { delay_blocks }` (1-4, 2 by default); the
    network sink sends the exact datagram again `delay_blocks` blocks later, right after that
    block's own (PMON unchanged); copies held in a `HeldCopies` per row allocated off the engine
    thread (the no-allocation test runs with redundancy on); kept per client id, else IP;
    `EngineHandle::set_redundancy` / `replace_redundancies`; status `redundancy`,
    `duplicates_sent`, `recovered`.
  - Desktop: the session keeps an entry's `redundancy` (`{ "delayBlocks": 2 }`, absent when off,
    read leniently, a delay it cannot use kept as written); every start and session load hands the
    readable ones to the engine (`replace_redundancies`, after the buffers, before the mixes).
    Command `set_musician_redundancy(musicianKey, on, delayBlocks?)` answers `{ musicianKey,
    outcome, redundancy, bandwidthFactor, saved, saveError, message, minDelayBlocks,
    maxDelayBlocks, defaultDelayBlocks }`, outcome `set` (bandwidth factor 2 on, 1 off) or `invalid`
    (nothing changed); a key it cannot read, nobody and no engine are errors (the engine answers for
    musicians here only, so there is no `notConnected`). Saved under the key the engine kept it
    under. The row gains `redundancy {on, delayBlocks}`, `duplicatesSent`, `recovered` and
    `redundancySupported` (the phone announced `redundancy.recovered`; every phone takes the
    copies).
  - UI: a Redundancy column on the musicians table: a switch, the cost under it ("2× bandwidth, each
    copy 2 blocks later" or "Sends once; on doubles this musician's bandwidth"), while on the
    copies sent and the packets recovered (or "Recovered packets not reported by this phone"), the
    outcome said on the row ("Bass's audio is now sent twice, each copy 2 blocks later: their
    bandwidth doubles (×2)."). The link health panel counts recovered packets in the summary ("15
    packets recovered by redundancy in 10 min"), the readout and a table column for a phone that
    reports them, and notes "Recovered packets: not reported by this phone (it needs a newer app)."
    while redundancy is on for one that does not. Logic in `lib/redundancy.js` (`redundancyView`,
    `formatRedundancySet`, `pendingRedundancyChoice`) and `link-health-view.js`; console and bridge
    `setMusicianRedundancy(musicianKey, on, delayBlocks)`; the fake keeps it per musician across
    starts and in named sessions, Bass's phone announces `redundancy.recovered` and recovers three
    in four of its losses from the second it is turned on.
  - Android: `redundancy.recovered` in `CLIENT_CAPABILITIES`; `stats.recovered` always sent (an
    integer). Recovered, as counted: a sequence the phone receives for the first time only after a
    newer one already arrived, within the last 64 sequences (`RECOVERY_WINDOW_PACKETS`, one bit
    each in a `Long` in `SequenceTracker`, no allocation): only a delayed copy or Wi-Fi reordering
    can do that, so reordering can inflate it slightly; a second copy, or one older than the window,
    is not counted, and a new stream forgets the window. `LinkStatsAccumulator.poll(...,
    cumulativeRecovered)` reports the difference per interval, as underruns.
  - Closed by T7b (the user chose "immediate copy + optional delay"). Was open: the Android player
    plays packets as they arrive, so a copy that fills a gap always comes after newer audio was
    played and is still dropped: today
    redundancy recovers no audio on Android, and `recovered` counts the copies that would have
    filled a gap (they stay in `packets_lost`; `protocol.rs` says so now). Recovering them needs the
    phone to hold newer packets while copies are arriving (up to the copy delay, which its cushion
    must cover), as `redundancy.rs` describes. The acceptance criterion "recovered packets instead
    of dropouts" is not met in audio until then.
  - RED: desktop, with the load not handing redundancy to the engine, 1 of 302 failed (the load
    order test); the uncommitted tests were proven by mutation: with the recovered capability check
    reading `playback.buffer`, the session keeping the address's setting, the delay range unchecked
    and `session_redundancy` returning nothing, 6 of 302 failed. UI: the suite failed to load
    (module missing), then 7 of 531 failed before the rows, bridge, console and fake were wired.
    Android, with compile-only stubs, 9 of 496 failed (the "copy of a played packet" guard passed).
    GREEN below.
  - Browser check (`desktop-ui`, `?fake#/network`): Bass on showed "2× bandwidth, each copy 2
    blocks later", "750 copies sent · 9 packets recovered" and the outcome with ×2; its panel said
    "15 packets recovered by redundancy in 10 min" and its table had a Recovered column; Drums on
    showed "Recovered packets not reported by this phone" on the row and the note in its panel; Bass
    off said "Bass's audio is sent once again: their bandwidth is back to normal (×1)." No console
    errors. Fake only: turning it off forgets the seconds recovered while it was on.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 501 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 302 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 531 passed; `gradle -p apps/android :app:testDebugUnitTest
    --max-workers=2 --no-daemon`: 496 tests, 0 failures; `:app:assembleDebug`: BUILD SUCCESSFUL,
    APK `apps/android/app/build/outputs/apk/debug/app-debug.apk` (not installed).
  - Commits: 5c4294e `feat(engine): send each block again a few blocks later` (+898/-20); 531ece6
    `feat(desktop): set redundancy per musician` (+725/-7); a2eead1 `feat(desktop-ui): redundancy
    toggle and recovered packets` (+703/-24); this commit `feat(android): report packets a
    duplicate recovered`.
- [x] T7b (redundancy that prevents dropouts; user decision "immediate copy + optional delay").
  Route: delegated writer (trigger: 2+ non-trivial files on every layer: network sink, protocol,
  registry, engine, control server, desktop command and DTO, redundancy view, table, screen,
  console, fake bridge, Android player, a new reorder hold, control channel and view model).
  - Engine: `Redundancy::On { delay_blocks }` takes 0-4, 0 by default
    (`MIN/DEFAULT_REDUNDANCY_DELAY_BLOCKS = 0`): a delay of 0 sends the datagram just sent again
    right after it in the same block's send loop (no store, `HeldCopies::new(0)` holds nothing);
    1-4 as before. `Redundancy::hold_blocks()` is the hold a phone needs (the delay; 0 off or
    immediate). The no-allocation test runs with one phone at delay 2 and one at delay 0.
  - Wire (documented in `protocol.rs`, `redundancy_message`): server to phone
    `{"type":"redundancy","delay_blocks":N}`, N 0-4, 0 = no hold (off or immediate copies); sent
    only to a phone that announced `redundancy.hold` (`CAPABILITY_REDUNDANCY_HOLD`), on every
    registration (after `playback`, none included) and on every change
    (`EngineEvent::RedundancyChanged`, `Push::Redundancy`; a lagged feed pushes it too). No ack.
  - Engine gate: `EngineHandle::set_redundancy` sets a delay > 0 only for a phone that announced
    `redundancy.hold`; any other musician answers `RedundancyOutcome::NeedsHold` and nothing changes
    (`RedundancySetting.outcome`). Immediate copies are set for anyone. `replace_redundancies`
    announces the hold-capable phones whose hold changed; a delay kept in a session for a phone
    without the hold is put in force as kept (its copies are dropped as duplicates, as before T7b).
  - `recovered` (protocol.rs, LinkStats.kt, ReorderHold.kt): packets played after filling a gap,
    each arriving for the first time after a newer one while the phone held that newer one back;
    they count in `packets_received`, not `packets_lost`. A late packet not played, and an immediate
    copy (it arrives in order and cannot be told from its original), are never counted.
  - Hold reporting: decided to count it in `stats.cushion_ms` (audio held back before playing, as
    the track's buffer is: `PlaybackLatency.reportExtras(holdPackets)`), so the Buffer cell's "plays
    X ms" and the cushion chart show the real latency; no new stat.
  - Desktop: command `set_musician_redundancy(musicianKey, on, delayBlocks?)` takes 0-4, 0 when
    absent; outcome `set`, `unsupported` (a delay for a phone without the hold: "needs a newer app",
    nothing changed or saved, `redundancy` null, bandwidth factor 1) or `invalid`; `minDelayBlocks`
    0, `defaultDelayBlocks` 0. The row gains `redundancyHoldSupported`. Sessions keep
    `{ "delayBlocks": 0 }` like any delay (absent reads as 0).
  - UI: the Redundancy cell shows, while on, a Copy select: "Copy: immediate" and "Copy: N blocks
    later (+X ms)", the cost from the engine's block (`blockMsOf(captureFormat)`: Fixed(n) at the
    device rate, else 256 frames at 48 kHz); for a phone without the hold the delays are disabled
    with ", needs a newer app" and a note "Delayed copies: needs a newer app". Cost line "2×
    bandwidth, no added latency" or "2× bandwidth, +10.7 ms latency on this phone"; for immediate
    copies the recovered line says "Recovered packets are counted for delayed copies only". Outcome
    words with the latency. Event `musician-redundancy { address, key, on, delayBlocks }`; the fake
    mirrors the range, the default and `unsupported`, Bass announces `redundancy.hold`, immediate
    copies save three in four losses for any phone (never counted), delayed ones only for a phone
    that holds (counted, and the hold added to its cushion).
  - Android: `redundancy.hold` in `CLIENT_CAPABILITIES`; `parseRedundancyHold` (a whole number 0-4,
    anything else ignored) to `ClientMixState.redundancyHoldPackets` (0 at every session start).
    `ReorderHold<T>` replaces `SequenceTracker` in front of the AudioTrack write: preallocated slots
    (`MAX_HOLD_PACKETS + 2` references, sequences, late flags), nothing allocated per packet (no
    boxing). A packet is played in sequence order once the newest is `holdPackets` ahead of it; a
    missing one is waited for until the newest is more than `holdPackets` ahead (its copy can no
    longer come), then skipped and counted once as lost before the next packet played; duplicates
    and packets already played or given up on are dropped; a sequence 256 or more behind the newest
    starts a new stream (what the old one left held is dropped); a long jump ahead plays what was
    held first; a receive timeout releases everything held; a shorter hold plays what is now due.
    With a hold of 0 it is exactly the old tracker (its tests ported). The receive loop reads the
    hold once per wake-up, offers each arrival (jitter measured at arrival) and plays every packet
    `next()` answers (stats, track, cushion at play).
  - RED: server, with `redundancy_hold` stubbed, 4 of 27 filtered tests failed (delay 0 refused,
    hold, empty store, immediate copy), then with the hold support and push stubbed 2 of 507 failed
    (the engine gate and the control push; the feed and replace tests passed, written with their
    code). Desktop, with the outcome unmapped and the row field false, 2 of 303 failed. UI: the
    suite failed to load against the old `redundancy.js` (missing exports); the pure logic was
    written before its tests ran, then 4 of 10 failed before the rows, fake and console were wired.
    Android, with compile-only stubs, 22 of 518 failed (18 hold, control, registration, view model,
    cushion report; the "unusable message is ignored" guard passed).
  - Browser check (`desktop-ui`, `?fake#/network`): Bass switched on showed "Copy: immediate", "2×
    bandwidth, no added latency", "Recovered packets are counted for delayed copies only" and
    "Bass's audio is now sent twice, each copy right after its original: their bandwidth doubles
    (×2), with no added latency."; Copy "2 blocks later (+10.7 ms)" showed "2× bandwidth, +10.7 ms
    latency on this phone", packets recovered and "... their phone plays 10.7 ms later."; Keys on
    showed delays 1-4 disabled with ", needs a newer app" and "Delayed copies: needs a newer app".
    No console errors.
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 507 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 303 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 532 passed; `gradle -p apps/android :app:testDebugUnitTest
    --max-workers=2 --no-daemon`: 506 tests, 0 failures (12 tracker tests removed with it, 23
    added); `:app:assembleDebug`: BUILD SUCCESSFUL, APK
    `apps/android/app/build/outputs/apk/debug/app-debug.apk` (not installed).
  - To confirm in the live test (T9): a delayed copy filling a real gap on the S23, and the hold's
    latency against the cushion.
  - Commits: 0233252 `feat(engine): send the redundant copy right away by default` (+144/-50);
    c0c3f24 `feat(server): tell a capable phone how long to hold for copies` (+406/-41); 0d4ea3f
    `feat(desktop): redundancy delay with a latency cost` (+516/-157); this commit
    `feat(android): hold packets so a late copy can fill its gap`.
- [x] T7-fix (findings of review-b5f8ba40d9e19a3d, review-483392b647c8df00 and the correction
  review-4a4559b6b013fee4). Route: delegated writer (trigger: findings across the target table,
  the control server, the protocol, the Android player and the desktop UI; done with T8). Each
  finding checked against b82d3a3 first.
  - WARNING fixed (e5398f9): `hold_copies` kept the held-copies store when the delay was unchanged,
    also when another musician took the row over at the same IP, so the previous occupant's
    personal-mix datagrams could be sent to the new phone. A takeover now always gets a fresh
    store (`new_held`, off the engine thread); the same musician registering again keeps theirs.
    Test `a_row_taken_over_at_the_same_delay_never_sends_the_previous_musicians_copies` (another
    client id, and a phone with no id). RED: 1 of 2 failed (same store kept).
  - SUGGESTION fixed (8f9c893): `recovered` is bounded at the parse by `packets_received` (they
    are among the packets received) and the running total saturates (`fetch_update` with
    `saturating_add`). Tests: protocol `recovered_packets_are_never_more_than_the_packets_received`
    and control `the_recovered_packets_a_phone_reports_add_up` (two reports of `u64::MAX` end at
    `u64::MAX`, not wrapped). RED: 2 of 4 failed.
  - WARNING (piece 2) found already fixed in 0d4ea3f: the desktop command maps
    `RedundancyOutcome::NeedsHold` to `unsupported`, nothing saved
    (`commands/redundancy.rs:165`, test `a_delayed_copy_for_a_phone_that_cannot_hold_is_
    unsupported_and_changes_nothing`).
  - WARNING (piece 2) found already fixed in 0d4ea3f: `DEFAULT_DELAY_BLOCKS = 0` and the wording
    "Copy: immediate" / "2× bandwidth, no added latency" (`lib/redundancy.js:21,68,101`, tests in
    `redundancy.test.js` for the default path and the fake's default).
  - WARNING (piece 2) found already fixed in 0d4ea3f: the fake's `set_musician_redundancy` takes
    0-4, 0 by default, says "from 0 to 4 blocks" and answers `unsupported` without
    `redundancy.hold` (`fake-bridge.js` `set_musician_redundancy`).
  - WARNING (piece 2) fixed (8f9c893, 44790a9): the semantics already agreed in the docs
    (`protocol.rs` `StatsCommand::recovered`, `LinkStats.kt`, `ReorderHold.kt`); now a contract
    fixture on both sides: `HoldPlayoutTest` plays a delayed copy into its gap and checks the
    `stats` it sends equal `STATS_AFTER_A_COPY_FILLED_A_GAP` (4 received, 0 lost, 1 recovered),
    and the server's `the_stats_a_phone_sends_after_a_copy_filled_a_gap` reads that same literal.
  - SUGGESTION fixed (d54a698): `openLowLatencyTrack`'s steps are `openTrackKeepingLowLatency`,
    generic over the track: a track that fails to start or to take the cushion is closed before
    the failure goes on (a failing close kept as suppressed); `TrackOpeningTest` (6 tests: kept
    when it shrinks, reopened at the minimum, large or minimum failing to start, cushion refused).
  - SUGGESTION fixed (44790a9): the receive loop's hold wiring is `HoldPlayout` (hold read per
    wake-up, a timeout releasing what is held, each played packet counted with its gap, the
    report taking the hold's recovered count); `HoldPlayoutTest` (4 tests).
  - RED (Android, both pieces stubbed): `:app:testDebugUnitTest --tests HoldPlayoutTest --tests
    TrackOpeningTest`: 10 tests, 7 failed. GREEN below (T8).
- [x] T8 (demo source): generator reader, engine start without a capture device, audio-setup
  entry. Route: delegated writer (trigger: 2+ non-trivial files on every layer: a generator, the
  pipeline, the soundcheck clock, engine start, capture hosts, desktop command and DTO, console
  logic, fake bridge, shell and chip).
  - Generator (`apps/server/src/demo.rs`): 8 voices of a four-bar loop at 120 BPM (8 s): kick,
    snare, hi-hat, bass, keys, pad, a vocal-ish line with vibrato and a sweeping tone, pink-ish room
    noise; more channels repeat them. Every sample is a pure function of its frame (noise is a hash
    of the frame), so it is the same on every run and in any block size. Each voice is -18 dBFS RMS
    over the loop with peaks at most -1 dBFS (gain measured once per process off the engine thread;
    measured: every voice -18.00 dBFS, peaks -1.25 to -11.21 dBFS). No samples or assets.
  - Engine: `AudioHost::Demo` ("demo"), one device `DEMO_DEVICE_NAME` "Demo (no interface)",
    `DEMO_CHANNELS` 8. `list_input_devices_for(Demo)` answers it without touching any host;
    `start` with it lists, opens and loads nothing (no cpal stream, `capture_device` none): the
    pipeline's `SourceReader` is a ring or `SourceReader::demo`, whose blocks the engine thread
    writes when its clock makes them due. The clock is the soundcheck's, extracted as
    `soundcheck::BlockPacer` (the local output's callbacks when one plays, else the timer;
    `playback_clock.set_active(true)` so callbacks wake the engine). Blocks of the buffer asked
    for at 48 kHz, F32, metered and counted in `samples_received` as a capture. Local outputs are
    the system's (as Auto). `EngineStatus.demo`, `EngineHandle::is_demo`.
  - Decisions: recording from the demo is allowed (a source like any other; the take's device is
    "Demo (no interface)"); a soundcheck is refused (`SoundcheckError::DemoSource`: a take plays in
    place of an interface's inputs); a switch to any device but its own is refused. Auto never
    resolves to the demo; a machine with no input device is told it can choose the Demo driver.
  - Desktop: `demo` accepted as a driver (start, device list, session `audio` kept and restored
    like any driver), DTO `demo`, refusal words for the soundcheck.
  - UI: `lib/demo-source.js` (`DEMO_HOST`, `DEMO_DEVICE`, `demoDevices`, `isDemoRunning`,
    `demoChip`); driver "Demo (no interface)" last in `HOST_OPTIONS`; `HOSTS` includes it;
    `DEFAULT_AUDIO` stays auto; the top bar's badge "Demo source — no interface in use" (an
    `ml-rec-chip` of kind `demo` linking to Network and system, added by the shell and shown by
    `mountRecChip`); the window title "MixLink Server — Demo source". The fake mirrors it (its one
    device, 8 inputs at 48 kHz F32, `demo` on the status, other devices and a soundcheck refused).
  - RED: server, `demo::` 2 of 4 failed with a silent generator; the engine start test failed
    with `is_demo` stubbed false (the pipeline and capture tests were written with their code and
    passed on their first run). Desktop: 1 of 3 failed (the DTO had no `demo`). UI: 7 of 539
    failed (module and wiring missing).
  - No allocation: `pipeline::tests::the_demo_source_plays_through_the_engine_thread_without_
    allocating` (2,000 blocks with pump, tick and wait: 0 allocations, 0 frees).
  - Browser check (`desktop-ui`, `?fake#/network`): Driver "Demo (no interface)" restarted on it;
    the device list showed only "Demo (no interface) — 8 channels", the format "8 ch · 48 kHz ·
    32-bit float · 256"; the top bar showed "Demo source — no interface in use" (link to
    #/network); Channels and groups showed CH 1-8 with moving level meters; Bass's redundancy on
    said "Copy: immediate", "2× bandwidth, no added latency" and "... each copy right after its
    original: their bandwidth doubles (×2), with no added latency."; no console errors. (The
    800 px pane cuts the strip's right edge, as it does for the REC chip.)
  - Checks: `CARGO_BUILD_JOBS=2 cargo test -p personal-monitoring`: 518 passed;
    `CARGO_BUILD_JOBS=2 cargo test -p mixlink-desktop`: 306 passed;
    `CARGO_BUILD_JOBS=2 cargo check -p mixlink-desktop --all-targets`: finished, no warnings;
    `node --test apps/desktop/ui/tests`: 539 passed; `gradle -p apps/android :app:testDebugUnitTest
    --max-workers=2 --no-daemon`: 517 tests, 0 failures; `:app:assembleDebug`: BUILD SUCCESSFUL,
    APK `apps/android/app/build/outputs/apk/debug/app-debug.apk` (not installed).
  - Commits: e5398f9 `fix(server): drop held copies when another musician takes the address`
    (+43/-1); 8f9c893 `fix(server): bound the recovered packets a phone reports` (+62/-5); d54a698
    `fix(android): release the large track if it fails to start` (+192/-13); 44790a9
    `test(android): cover the receive loop's hold wiring` (+189/-12); e5d0d32 `feat(engine): a demo
    source that needs no interface` (+964/-91); 645a324 `feat(desktop): choose the demo source in
    the audio setup` (+78/-5); this commit `feat(desktop-ui): demo source entry and badge`
    (+337/-18 before this document).
- [ ] T9 (close): PRD update, docs, live test with the S23. Route: inline or writer.

## Delivery

Strategy `ask-on-risk` with chain strategy `stacked-to-main` (as in phases 1-3); one pull request
per reviewed slice, only with the user's approval.

## Progress

- Branch `feat/desktop-network` from main 4f2ae0c (phase 3 merged as PRs #39-#56).
- Exploration done (read-only map with file evidence); the user chose "Auto or fixed" for the
  buffer.

- T1 done (log file), two commits.
- review-b891debe9d10ebc9 (4f2ae0c..d787a05, T1) approved and acknowledged, with three findings:
  the log rotation failure that loses lines (WARNING), the one-time stderr explanation never
  re-armed and the failure count not exposed (SUGGESTION), and the discarded UDP send error
  (SUGGESTION); all fixed in T1-fix. Next review base d787a05.
- T1-fix and T2 done (mixes backup), four commits.
- review-8872f9deff0e3f2d (d787a05..81155c8, T1-fix and T2) approved and acknowledged, with one
  WARNING: a prune failure counted the backup as failed, so identical backups piled up (fixed in
  T2-fix). Next review base 81155c8.
- T2-fix and T3 done (health history), four commits.
- review-6419b655bb738c08 (81155c8..0d689e1, T2-fix and T3) approved and acknowledged, with three
  findings: the history's gaps, order and cursor on the wall clock (WARNING), a configured-target
  phone's reconnect never marked and its old round trip kept (SUGGESTION), and no control-level
  tests for the tick's gaps and a closed connection (SUGGESTION); all fixed in T3-fix. Next review
  base 0d689e1.
- T3-fix and T4 done (health chart), three commits.
- review-e6137cfb0b4da4c8 (0d689e1..e26e4f3, T3-fix and T4) approved and acknowledged, with three
  findings: the screen's link health wiring untested (WARNING), a phone listed by address that
  registers closing its panel (SUGGESTION), and the closed-connection test's fixed 3 s sleep
  (SUGGESTION); all fixed in T4-fix. Next review base e26e4f3.
- The user approved Android builds with `--max-workers=2`.
- T4-fix and T5 done (Android capabilities and stats extras), three commits.
- review-d62775e702018934 (e26e4f3..88ecaff, T4-fix and T5) approved and acknowledged, with one
  SUGGESTION: the receive loop's stats extras wiring untested (fixed in T5-fix). Next review base
  88ecaff.
- T5-fix and T6 done (buffer from the desk), five commits.
- review-65facc2806258c4e (88ecaff..6afc117) and review-1e5f9aa8d353b411 (6afc117..a5cfa2c), T5-fix
  and T6, approved and acknowledged, with four findings fixed in T6-fix. Next review base a5cfa2c.
- T6-fix and T7 done (redundancy), seven commits. Open before the live test: the Android player
  cannot play a late copy yet (see T7).
- The user chose "immediate copy + optional delay" for redundancy; T7b done, four commits, closing
  T7's open note. Next review base stays a5cfa2c: T6-fix, T7 and T7b are reviewed together.
- review-b5f8ba40d9e19a3d (a5cfa2c..531ece6) and review-483392b647c8df00 (531ece6..c0c3f24)
  approved and acknowledged, with findings fixed or found already fixed in T7-fix.
  review-4a4559b6b013fee4 (c0c3f24..230778a) returned `correction_required` for a CRITICAL
  ReorderHold stale-waiting bug, corrected in b82d3a3, validated and acknowledged. Next review base
  b82d3a3.
- T7-fix and T8 done (demo source), seven commits.

## Next step

T9 (close: PRD update, docs, live test with the S23).
