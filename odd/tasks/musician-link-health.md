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
  - `unknown`: no report yet on the current control session (older phones never report; a new
    session starts with none), or the last one is older than 3 s and the musician's control channel
    has closed (the musician stopped or left).
  - `bad`: the last report is older than 3 s while the control channel is still connected (stats
    travel over the same Wi-Fi as the audio, so a phone that lost its link can never report it; no
    rates are shown), or, on a fresh report, any receive timeout, or underruns >= 2/s, or
    loss >= 2 %.
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
- [x] T2 (desktop): health and rates in `MusicianDto`; a health column with a dot and a tooltip in
  the musicians table.
- [ ] T3 (android): pure `SequenceTracker` (drop late/duplicate, count gaps once), counted receive
  timeouts, a pure stats accumulator and message builder, and the 1 s report sent through
  `ControlChannel`. JVM tests for each pure helper.

## Acceptance criteria

- A phone that streams cleanly shows green within ~2 s of connecting.
- Pulling the phone off Wi-Fi turns its dot red once its last report is older than 3 s, and grey
  once its control channel closes.
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
| 2 | Server wiring: routing, per-target storage, engine status (~310 lines), plus the lost-link fix (a stale report while connected is bad) | `71d02d7`, `4692544` |
| 3 | Desktop health column (T2) | `90f7a33` |
| 4 | New-session fix (a new control session starts with no link report; review 2 R3-001), then the Android sequence tracker and stats report (T3) | this commit, T3 pending |

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
- Slice 2 closes T1: `stats` routing in `control.rs`, per-target report storage in
  `targets.rs` and health in the engine status, plus the removal of the two temporary
  `#[allow(dead_code)]` attributes from slice 1 now that the callers exist.
- Native review: `.atl/` was excluded locally (`.git/info/exclude`, with the user's approval) so the
  review preflight has no untracked files to declare.
- Slice 1 native review approved and acknowledged (lineage `review-a3e66b32c636ca01`, reliability
  lens). Its advisory R3-001 is fixed by the lost-link fix commit below.
- Slice 2 committed as `71d02d7`; assessed medium and under budget, so its review is pending in the
  slice.
- Lost-link fix (`4692544`, part of slice 2; advisory R3-001): stats travel over the same Wi-Fi as
  the audio, so a phone that loses Wi-Fi can never send the report that would turn it red, and a
  stale report used to show grey. `classify` and `link_status` now take `control_connected`: no
  report ever is `unknown`; a fresh report keeps the thresholds; a stale report is `bad` (no rates)
  while the control channel is connected and `unknown` once it has closed. The engine passes the
  fact it already has, the connected control peer IPs, through a new pure `link_statuses` helper in
  `engine.rs`.
- T2 done (`90f7a33`, slice 3; route: delegated direct, one writer). Desktop side of link health:
  - `dto.rs`: `MusicianDto` gains `link_health` (`"unknown"`, `"good"`, `"warning"`, `"bad"`) and
    the three optional rates, passed through from `MusicianStatus.link`; JSON keys are
    `linkHealth`, `underrunsPerSecond`, `lossPercent`, `receiveTimeoutsPerSecond` (`null` when
    absent). `commands.rs` only gains the new fields in a test fixture.
  - `index.html` / `styles.css`: a "Link" column next to Control; column widths rebalanced to keep
    100 %. Dot colours reuse the window's tokens: grey (unknown), `--green`, `--accent` (amber),
    `--red`.
  - `main.js`: `describeLink` turns a row into a level, a short word and a tooltip. Fresh numbers
    show as `Underruns 0.0/s · Loss 0.00 % · Timeouts 0.0/s`; `bad` without numbers shows "lost"
    with `Reports stopped: link lost`; no numbers otherwise shows `No reports from this phone`. The
    dot is `role="img"` with an `aria-label` carrying the level and the tooltip text; the visible
    word is `aria-hidden`. The cell is updated in place on each poll like the others.
- Review 2 (lineage `review-8741f049c86db454`, range `49da28a..90f7a33`): approved and
  acknowledged. Advisory R3-001 is fixed by the new-session commit below. Advisory R3-002
  (`describeLink` in `main.js` has no automated test) stays a known gap: the repository has no JS
  test runner, and the T2 smoke check below is the only evidence for it.
- New-session fix (this commit, slice 4; route: delegated direct, one writer; review 2 R3-001): the
  report used to survive control close and re-registration, so a phone that came back on a new
  control session showed red (a stale report with a connected control channel) until its first new
  report, and an older build that never reports stayed red for good. A new control session now
  starts with no report:
  - `targets.rs`: `TargetRegistry::clear_link_report(ip)` forgets the stored report and keeps the
    slot, the mix state, the counters and the origin; an IP with no target is a no-op.
    `register` itself still keeps the report.
  - `control.rs`: the first successful `register` of a connection clears the report. A later
    `register` on the same socket is a rename (the phone re-announces its name while streaming) and
    keeps the current report, so typing a name does not blank the light.

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
- Lost-link fix RED (new `control_connected` parameter accepted but ignored):
  `cargo test -p personal-monitoring` gave 148 passed and 3 failed:
  `health::tests::a_report_that_goes_stale_while_the_control_channel_is_connected_is_a_lost_link`,
  `health::tests::a_lost_link_is_bad_with_no_rates_to_show` and
  `engine::tests::a_stale_report_is_judged_by_whether_the_musicians_control_channel_is_still_connected`
  (each got `Unknown` where `Bad` was expected).
- Lost-link fix GREEN:
  - `cargo fmt --all -- --check`: exit 0.
  - `cargo test -p personal-monitoring`: 151 passed, 0 failed.
  - `cargo test -p mixlink-desktop`: 17 passed, 0 failed.
  - `cargo check --workspace --all-targets`: finished, no errors or warnings.
- T2 RED (new `MusicianDto` fields filled with a fixed `"unknown"` and no rates):
  `cargo test -p mixlink-desktop` gave 17 passed and 4 failed:
  `each_link_health_level_reaches_the_window_as_its_lowercase_name`,
  `the_rates_the_health_was_judged_on_are_passed_through`,
  `a_musician_without_a_fresh_report_has_no_rates` and
  `the_link_fields_are_camel_case_json_with_absent_rates_as_null`.
- T2 GREEN:
  - `cargo fmt --all -- --check`: exit 0.
  - `cargo test -p personal-monitoring`: 151 passed, 0 failed.
  - `cargo test -p mixlink-desktop`: 21 passed, 0 failed.
  - `cargo check --workspace --all-targets`: finished, no errors or warnings.
- T2 UI: the window's JS, HTML and CSS have no test runner (no Node toolchain in the repo), so there
  is no RED for them. Smoke check instead: a scratch harness page outside the repository loaded the
  real `index.html` and `main.js` in headless Chrome with a stubbed Tauri global serving one musician
  per case. All five rows rendered 9 cells under a "Link" header with no script errors: `good`,
  `warning` and `bad` with their numbers, `bad` without numbers as "lost" / `Reports stopped: link
  lost`, and `unknown` / `No reports from this phone`; a screenshot at the window's default 1180 px
  width showed no clipped columns.
- New-session fix RED (`clear_link_report` as a no-op, wiring in place):
  `cargo test -p personal-monitoring` gave 152 passed and 2 failed:
  `control::tests::a_new_control_session_starts_with_no_link_report` (health `Bad` where `Unknown`
  was expected) and `targets::tests::clearing_a_link_report_forgets_only_the_report` ("the report
  is forgotten"). `control::tests::a_rename_on_the_same_session_keeps_the_link_report` already
  passed and is kept as a guard for the rename path.
- New-session fix GREEN:
  - `cargo fmt --all -- --check`: exit 0.
  - `cargo test -p personal-monitoring`: 154 passed, 0 failed.
  - `cargo test -p mixlink-desktop`: 21 passed, 0 failed.
  - `cargo check --workspace --all-targets`: finished, no errors or warnings.
