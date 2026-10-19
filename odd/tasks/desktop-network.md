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
- **Redundancy**: the same datagram is sent again one or more blocks later (a delay offset, so a
  Wi-Fi burst does not take both copies). The PMON wire format stays byte-identical: old phones
  already drop duplicates by sequence (`SequenceTracker.kt:53-56`). Per musician on/off; costs
  double bandwidth for that musician. The held copy is preallocated; the network sink keeps its
  no-allocation guarantee.
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
    `feat(server): keep a link history with round-trip time` (+1181/-7); this commit
    `feat(desktop): serve a musician's link history`.
- [ ] T4 (health chart): chart on the Network and system screen. Route: delegated writer.
- [ ] T5 (Android): announce capabilities, report jitter and cushion in `stats`. Route: delegated
  writer.
- [ ] T6 (buffer from the desk): server message, desk control per musician, Android applies Auto
  or fixed. Route: delegated writer.
- [ ] T7 (redundancy): delayed duplicate send per musician, desk toggle, recovered count. Route:
  delegated writer.
- [ ] T8 (demo source): generator reader, engine start without a capture device, audio-setup
  entry. Route: delegated writer.
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

## Next step

T4 (health chart).
