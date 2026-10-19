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

- [ ] T1 (log file): log sink in the server crate, file writer with rotation in the desktop,
  existing `println!`/`eprintln!` routed through it. Route: delegated writer.
- [ ] T2 (mixes backup): debounced autosave of live mixes to a rolling backup, restore from the
  session panel. Route: delegated writer.
- [ ] T3 (health history): capabilities on `register`, RTT from ping/pong, history ring per
  musician, phone stats extras (jitter, cushion) parsed when present, DTO and events. Route:
  delegated writer.
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

## Next step

T1 (log file).
