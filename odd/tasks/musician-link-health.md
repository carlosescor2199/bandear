# Musician Link Health

## Objective

Let the engineer see, per musician and at a glance, whether that musician's phone is hearing clean
audio: the phone reports what it experiences, and the desktop shows a traffic light per musician.

## Problem and why

Today the server only knows what it sent. The phone already knows what went wrong - it counts
sequence gaps and reads `AudioTrack.underrunCount` - but that stays on the phone's own screen. When
a musician hears dropouts, the engineer cannot tell which phone it is, or whether the cause is the
network (lost packets) or the phone (underruns), without walking over and looking.

The phone's loss count is also inaccurate: a late or duplicated packet rewinds the expected
sequence (`PmonPlayer.kt` around line 118), so the next packet's gap is counted twice, and the late
packet is still played out of order. Receive timeouts (250 ms with no packet) are swallowed and
never counted. Reporting those numbers as they are would make the traffic light lie.

## Design decisions

- **The phone reports deltas, the server classifies.** Every ~1 s while streaming, the phone sends a
  `stats` control message with the counts for that interval. Classification lives in one pure Rust
  function, so thresholds are tested and change in one place.
- **Wire message** (client to server, on the existing WebSocket control channel):
  `{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}`.
  It is matched to the musician by the peer IP, exactly like `mix`.
- **Health levels** (our own thresholds, tunable constants):
  - `unknown`: no report yet, or the last one is older than 3 s (older phones never report).
  - `bad`: any receive timeout, or underruns >= 2/s, or loss >= 2 %.
  - `warning`: any underrun, or loss >= 0.5 %.
  - `good`: otherwise.
- **Late and duplicate packets are dropped, not played.** A packet whose sequence is not newer than
  the last played one is discarded; gaps are counted once. This is also the dedup guard the planned
  packet-redundancy feature needs.

## Scope

- Server: parse and route `stats`, keep the latest report per musician, classify, expose it in the
  engine status.
- Desktop: a health column with a coloured dot and the numbers on hover.
- Android: an accurate sequence tracker, counted receive timeouts, and a 1 s stats report sent over
  the control channel.

## Out of scope

- Packet redundancy, a simulated input board and mDNS discovery: planned next, each as its own
  feature document.
- Remote adjustment of the phone's playback cushion from the desktop.
- Per-channel DSP and effects (a later phase).

## Constraints

- Clean-room design: own protocol, names and thresholds. Nothing is ported from any other product.
- The audio path is untouched: the network thread never reads or locks the health state.
- The uncommitted `USAGE_MEDIA` change in `PmonPlayer.kt` belongs to separate latency work and must
  not be included in these commits.
- Keep technical artifacts in English.

## Tasks

- [x] T1 (server): `stats` message parse and its own routing branch in `control.rs`; latest report
  stored per target (surviving re-registration); pure `classify` function; health and rates in
  `MusicianStatus`. RED-first unit tests for parse, classify and routing.
- [ ] T2 (desktop): health and rates in `MusicianDto`; a health column with a dot and a tooltip in
  the musicians table.
- [ ] T3 (android): pure `SequenceTracker` (drop late/duplicate, count gaps once), counted receive
  timeouts, a pure stats accumulator and message builder, and the 1 s report sent through
  `ControlChannel`. JVM tests for each pure helper.

## Acceptance criteria

- A phone that streams cleanly shows green within ~2 s of connecting.
- Pulling the phone off Wi-Fi turns its dot red within ~2 s, then grey after 3 s without reports.
- A phone build without stats support shows grey and keeps working.
- A `stats` message from an IP with no target gets the existing no-target error and changes nothing.

## Acceptance checks

- `cargo fmt --all -- --check`
- `cargo test -p personal-monitoring`
- `cargo test -p mixlink-desktop`
- `cargo check --workspace --all-targets`
- `gradle -p apps/android :app:testDebugUnitTest`

## Route

- T1 and T2: delegated direct, one writer (2+ non-trivial files across server and desktop).
- T3: delegated direct, one writer (Android, 2+ non-trivial files).

## Delivery forecast

About 650 authored changed lines (T1 ~260, T2 ~110, T3 ~280), above the ~400-line budget. Strategy:
`ask-on-risk`; the chain strategy is asked before the commit that crosses the budget.

T1 alone came to roughly 800 authored changed lines (about 415 inserted and 6 deleted in existing
files plus the new 379-line `health.rs`), mostly tests: about 540 test lines against about 260 lines
of production code. It crosses the budget on its own.

Chain strategy (chosen by the user): `stacked-to-main`. History stays linear on
`feat/musician-link-health`; each slice branch is created at PR time as a pointer to its commit, and
each slice lands on `main` on its own (until T3 ships, the light simply stays grey).

| Slice | Content | Commit |
|---|---|---|
| 1 | Health model: `health.rs` and the `stats` parser (500 lines, mostly tests) | `e50b8bc` |
| 2 | Server wiring: routing, per-target storage, engine status (~310 lines) | this commit |
| 3 | Desktop health column (T2) | pending |
| 4 | Android sequence tracker and stats report (T3) | pending |

Slice 1 stays above the budget after one honest slicing pass; it is about 380 lines of tests, so a
`size:exception` is recommended for it rather than splitting the tests from the code they verify.

## Progress

- Branch `feat/musician-link-health` created from `main` at `d99bbd9`.
- T1 done (route: delegated direct, one writer). Server side of link health:
  - `protocol.rs`: `StatsCommand` and `parse_stats_command`; `interval_ms` must be 1..=60000
    (`MAX_STATS_INTERVAL_MS`), unknown fields are ignored.
  - `control.rs`: `stats` has its own branch. It joins on the peer IP like `mix`, an unknown IP gets
    the shared `NO_TARGET_ERROR` and nothing is stored, a parse failure gets an error, and success
    stores the report with no reply. `register` and `mix` behaviour is unchanged.
  - `targets.rs`: `TargetEntry.link_report` (`Arc<Mutex<Option<LinkReport>>>`) and
    `TargetRegistry::link_report(ip)`; the slot survives re-registration like the counters.
  - `health.rs` (new): `LinkHealth`, `LinkReport`, `LinkStatus`, the pure `classify` and
    `link_status`, and the named threshold constants. Comparisons are exact integer (`u128`) math.
  - `engine.rs`: `MusicianStatus.link: LinkStatus`, classified in `status()` against one
    `Instant::now()`. `network.rs` is untouched and never reads the report.
  - `MusicianStatus` and `EngineStatus` are now `PartialEq` without `Eq`, because the rates are
    `f64`.
  - Compile fixes only: `link: Default::default()` in the `dto.rs` tests and in
    `examples/name_precedence_probe.rs` (that path was added to the edit surface with the user's
    approval).
- Slice 1 committed as `e50b8bc` and verified in isolation (detached worktree, shared target dir):
  `cargo fmt --all -- --check` exit 0, `cargo test -p personal-monitoring` 142 passed,
  `cargo check --workspace --all-targets` clean. It carries two temporary `#[allow(dead_code)]`
  attributes that slice 2 removes once the callers exist.
- Slice 2 (this commit) closes T1: `stats` routing in `control.rs`, per-target report storage in
  `targets.rs` and health in the engine status, plus the removal of the two temporary
  `#[allow(dead_code)]` attributes from slice 1 now that the callers exist.
- Native review: `.atl/` was excluded locally (`.git/info/exclude`, with the user's approval) so the
  review preflight has no untracked files to declare.

## Verification

- T1 RED (with `todo!()` skeletons): `cargo test -p personal-monitoring` gave 122 passed and 25
  failed: 14 `health::tests`, 6 `protocol::tests` stats tests, 2 `targets::tests::a_link_report_*`,
  `engine::tests::joins_targets_and_connected_peer_ips_into_one_row_per_musician` and 2 control
  stats tests. A routing RED followed once the lookup and parser existed:
  `a_stats_report_is_stored_on_the_peers_target_without_a_reply` failed with "the report should be
  stored", and `an_invalid_stats_report_gets_an_error_and_stores_nothing` failed with an error of
  `missing field volume_percent`, which showed `stats` falling into the mix branch.
  `a_stats_report_from_an_ip_with_no_target_gets_the_no_target_error` already passed in RED (the
  mix fallthrough gives the same error) and is kept as a regression guard.
- T1 GREEN:
  - `cargo fmt --all -- --check`: exit 0.
  - `cargo test -p personal-monitoring`: 147 passed, 0 failed (baseline was 122).
  - `cargo test -p mixlink-desktop`: 17 passed, 0 failed.
  - `cargo check --workspace --all-targets`: finished, no errors or warnings.
