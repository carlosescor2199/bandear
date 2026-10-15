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
    rates are shown), or, on a fresh report, nothing received over its interval whatever the other
    counts (the phone hears no audio at all; the received rate is exposed as 0 so the window can say
    so), any receive timeout, or underruns >= 2/s, or loss >= 2 %.
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
- [x] T3 (android): pure `SequenceTracker` (drop late/duplicate, count gaps once), counted receive
  timeouts, a pure stats accumulator and message builder, and the 1 s report sent through
  `ControlChannel`. JVM tests for each pure helper. Evidence: `6c93b9f`, refined by the slice 4
  commit that stops counting timeouts before the first packet (see Progress and Verification).

## Acceptance criteria

- A phone that streams cleanly shows green within ~2 s of connecting.
- Pulling the phone off Wi-Fi turns its dot red once its last report is older than 3 s, and grey
  once its control channel closes.
- A phone whose control channel works but that receives no audio at all shows red, with
  `No audio received` on hover; it is never green.
- A phone build without stats support shows grey and keeps working.
- A `stats` message from an IP with no target gets the existing no-target error and changes nothing.

## Acceptance checks

- `cargo fmt --all -- --check`
- `cargo test -p personal-monitoring`
- `cargo test -p mixlink-desktop`
- `cargo check --workspace --all-targets`
- `gradle -p apps/android :app:testDebugUnitTest`
- `gradle -p apps/android assembleDebug`

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
| 4 | New-session fix (a new control session starts with no link report; review 2 R3-001), then the Android sequence tracker and stats report (T3) | `cb1923c`, `6c93b9f`, `d46402c` (timeouts before the first packet), `7b7e194` (no audio received is bad), `9fbbd13` (reports held until the audio starts or a startup grace passes; review 3 R3-001) |

Slice 1 stays above the budget after one honest slicing pass; it is about 380 lines of tests, so a
`size:exception` is recommended for it rather than splitting the tests from the code they verify.

Slice 4 also lands above the budget: the new-session fix is about 170 authored lines (about 140 of
them tests) and T3 about 480 (about 220 production with doc comments, about 260 tests), plus about
50 for the timeouts-before-the-first-packet fix, about 100 for the no-audio fix (about 70 of
them tests) and about 145 for the startup-grace fix (about 90 of them tests), so about 950 in all,
not counting this document.
Its commits are separate work units, so it can be split into pull requests at those commit
boundaries if a smaller review is wanted.

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
- New-session fix (`cb1923c`, slice 4; route: delegated direct, one writer; review 2 R3-001): the
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
- T3 done (`6c93b9f`, slice 4; route: delegated direct, one writer). Android side of link health:
  - `SequenceTracker.kt` (new, pure): the first packet starts the stream; a newer packet is played
    and its gap counted once as lost; a duplicate or late packet is dropped (not written to the
    track) and rewinds nothing; a packet `STREAM_RESTART_PACKETS` (256, about 2.5 s) or more behind
    the last played one is a new stream (server restart), played without counting the jump.
  - `LinkStats.kt` (new, pure, time injected): `LinkStatsReport` and `LinkStatsAccumulator`, which
    counts played packets, lost packets and receive timeouts, turns the track's cumulative
    `underrunCount` into a per-report delta, and reports once at least 1000 ms have passed, with
    `interval_ms` the real elapsed time clamped to 1..60000; the interval counts then restart.
    `packets_received` counts played packets, so received plus lost is the stretch of the stream the
    interval covered.
  - `ControlChannel.kt`: `buildStatsMessage` next to `buildRegisterMessage` (JSONObject, integer
    values); `sendLinkStats` hands the report to the control executor, behind any queued
    registration or mix, and drops it when the channel is not open or has shut down.
  - `PmonPlayer.kt`: the receive loop uses the tracker (late and duplicate packets are no longer
    played), counts each `SocketTimeoutException`, and polls the accumulator at the top of every
    iteration (every packet and every 250 ms timeout), reading `underrunCount` only when a report is
    due; `Listener.onLinkStats` hands the report out. The cushion controller keeps its own underrun
    delta, unchanged. The local stats screen now shows played packets and the tracker's loss count.
    The `AudioAttributes` block is untouched.
  - `MainActivity.kt`: `onLinkStats` forwards the report to `ControlChannel.sendLinkStats`.
- Timeouts before the first packet (`d46402c`, slice 4; route: delegated direct, one writer): the
  receiver starts before the server knows where to send audio, so T3's first report of a session
  could carry a receive timeout from before the first packet and show a phone that connected
  cleanly as red for its first second. `LinkStatsAccumulator` now counts a receive timeout only
  once the stream has started (a packet was played); once started it stays started, so a wait
  while a restarted server comes back is still counted.
  - Decision (resolved by the no-audio fix below): a phone whose control channel works but which
    never receives audio (UDP blocked, wrong port) now reports zero packets, zero loss and zero
    timeouts every second, and the server judged such a report `good` (`judge` in `health.rs`;
    `loss_percent` is 0 when nothing was expected). Before `d46402c` it showed red. Candidates
    were: send no report until the stream starts (the light stays grey), or count timeouts before
    the first packet only after a startup grace of a second or two (it turns red).
- No-audio fix (`7b7e194`, slice 4; route: delegated direct, one writer; resolves the decision
  above). Rule: a fresh report whose `packets_received` is 0 over its interval is `bad`, whatever
  the other counts. Reason: the objective is whether the phone hears clean audio, and a phone
  hearing nothing must never be green; the server rule covers every cause (UDP blocked, a wrong
  port, the engine stopped) without changing the phone.
  - `health.rs`: `judge` checks `packets_received == 0` first. `LinkStatus` gains
    `packets_per_second` (received packets over the interval; `None` without a fresh report, like
    the other rates).
  - `dto.rs`: `MusicianDto.packets_per_second`, JSON `packetsPerSecond` (`null` when absent).
    `engine.rs` and `commands.rs` only gain the field in test fixtures.
  - `main.js`: `describeLink` shows the word "silent" and the tooltip `No audio received` for
    `bad` with `packetsPerSecond === 0`; every other case is unchanged.
- Review 3 (lineage `review-17d283b442597b70`, range `90f7a33..7b7e194`): approved and
  acknowledged. Advisory R3-001 (the phone reports from the start, so a first interval that ends
  before any audio arrives sends a 0-packet report and shows a healthy phone as bad) is fixed by
  the startup-grace commit below. Advisory R3-002 (no test that a `DROPPED` packet is not written
  to the track, or that reports are sent while no packets arrive) stays a known gap: both live in
  `PmonPlayer`'s receive loop, which owns the `DatagramSocket` and the `AudioTrack`, and no tiny
  seam reaches them; covering them needs that loop split from its Android I/O. The decisions
  themselves are covered by the pure tests: `SequenceTrackerTest` for `DROPPED`, and
  `LinkStatsTest` for reports with no packets after the startup grace.
- Startup-grace fix (`9fbbd13`, slice 4; route: delegated direct, one writer; review 3 R3-001):
  `LinkStatsAccumulator` holds reports back until the stream starts (the first packet is played) or
  `LINK_STATS_STARTUP_GRACE_MS` (3000 ms, the same span the server keeps a report fresh) has passed
  since the receiver started, whichever comes first; after that a report is due every 1000 ms as
  before. The timeouts-before-the-first-packet rule is unchanged.
  - Stream starts within the grace: the first interval starts at the first packet (`onPacket` now
    takes the packet's arrival time), so the first report comes one interval later, carries packets
    and describes the stream alone; the startup silence before it is not reported.
  - Grace passes with no packet: the first report covers everything since the receiver started
    (`interval_ms` is the real elapsed time, about 3000 ms or up to one 250 ms receive timeout more,
    clamped to 60000) with 0 packets, 0 timeouts and 0 underruns, so the server shows the phone as
    bad ("No audio received") about 3 s after it connects; reports then follow every second.
  - A stream that starts after the grace joins the interval already running, so the cadence stays.
  - `PmonPlayer.kt` passes the packet's arrival time to `onPacket`; the `Listener.onLinkStats`
    comment describes the hold-back. Until the first report the server has none for the new
    control session, so the musician shows grey, not red.
- Review 4 (lineage `review-55d96b6638201158`, range `7b7e194..9fbbd13`): approved and
  acknowledged. Its only advisory, R3-001 (a suggestion), asks for a boundary test where the first
  packet arrives exactly at `LINK_STATS_STARTUP_GRACE_MS` before any poll; the behaviour is the
  documented one, so it is recorded as a follow-up test, not a defect.
- A separate review of the whole unpushed range (`origin/main` `f231dad`..`90f7a33`, lineage
  `review-67dd43fed2154ac3`) was approved; its findings are all in earlier, unrelated commits (the
  audio panel, ASIO buffer defaults, the playback cushion) and are tracked outside this feature.

## Status and next steps

- Code complete: T1, T2 and T3 are done and every slice range has an approved native review.
- Final checks: `cargo test -p personal-monitoring` 156 passed, `cargo test -p mixlink-desktop` 21
  passed, Android JVM tests 81 passed with the debug APK assembled.
- Pending: the acceptance criteria have not been verified on a device. Live check with the S23 and
  the desktop: green within ~2 s on clean Wi-Fi; red ("silent") ~3 s after connecting with UDP
  blocked; red once reports stop while connected (Wi-Fi off); grey once the app closes its control
  channel; grey for an older build that never reports.
- Pending (user decision): push and the stacked-to-main pull requests, one per slice.
- Known gaps: no JS test runner for `describeLink` (review 2 R3-002); the `PmonPlayer` receive loop
  is only covered through its pure helpers (review 3 R3-002); the startup-grace boundary test
  (review 4 R3-001).

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
- T3 toolchain: no Gradle wrapper and no `gradle` or `java` on PATH; the installed Gradle 9.6.0
  distribution under `~/.gradle/wrapper/dists/gradle-9.6.0-bin/` was run with `JAVA_HOME` set to
  Android Studio's bundled JBR and `ANDROID_HOME` to the local Android SDK. Baseline:
  `:app:testDebugUnitTest` 59 tests, 0 failures.
- T3 RED (stub bodies: `accept` always 0, `isDue` false, `poll` null, `buildStatsMessage` `{}`):
  `gradle -p apps/android :app:testDebugUnitTest` gave 74 tests, 11 failed: both
  `ControlChannelTest` tests, 5 `LinkStatsTest` tests and 4 `SequenceTrackerTest` tests. The 4 that
  already passed (in-order play, the first packet starting anywhere, a server restart counting no
  loss, no report before the interval) are kept as guards.
- T3 GREEN:
  - `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 74 tests, 0
    failures (15 added: `SequenceTrackerTest` 7, `LinkStatsTest` 6, `ControlChannelTest` 2); the
    debug APK assembled with no Kotlin warnings.
  - `cargo fmt --all -- --check`: exit 0. `cargo test -p personal-monitoring`: 154 passed.
    `cargo test -p mixlink-desktop`: 21 passed. `cargo check --workspace --all-targets`: clean.
- T3 runtime check on a phone: not run here (no device in this session). The JVM tests cover the
  pure helpers; the wiring in `PmonPlayer` and `MainActivity` is covered only by the debug build.
- Timeouts-before-the-first-packet RED (three new `LinkStatsTest` tests, accumulator unchanged):
  `gradle -p apps/android :app:testDebugUnitTest` gave 77 tests, 2 failed:
  `aTimeoutBeforeTheFirstPacketIsNotCounted` and `aTimeoutAfterTheFirstPacketIsCounted` (the
  timeout from before the first packet was counted too). `theStreamStaysStartedAcrossReports`
  already passed and is kept as a guard.
- Timeouts-before-the-first-packet GREEN:
  - `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 77 tests, 0
    failures; the debug APK assembled with no Kotlin warnings.
  - `cargo fmt --all -- --check`: exit 0 (no Rust changes).
- No-audio fix RED (`LinkStatus.packets_per_second` and `MusicianDto.packets_per_second` added as
  always `None`, `judge` unchanged):
  - `cargo test -p personal-monitoring` gave 153 passed and 3 failed:
    `health::tests::nothing_received_is_bad_even_with_no_loss_and_no_timeouts` and
    `health::tests::nothing_received_is_bad_whatever_the_other_counts` (`Good` and `Warning` where
    `Bad` was expected), and `health::tests::link_status_reports_the_rates_normalised_by_the_interval`
    (`packets_per_second` `None` where `Some(495.0)` was expected).
    `a_single_packet_received_is_not_silence` already passed and is kept as the boundary guard.
  - `cargo test -p mixlink-desktop` gave 19 passed and 2 failed:
    `the_rates_the_health_was_judged_on_are_passed_through` and
    `the_link_fields_are_camel_case_json_with_absent_rates_as_null`.
- No-audio fix GREEN:
  - `cargo fmt --all -- --check`: exit 0.
  - `cargo test -p personal-monitoring`: 156 passed, 0 failed (baseline 154).
  - `cargo test -p mixlink-desktop`: 21 passed, 0 failed.
  - `cargo check --workspace --all-targets`: finished, no errors or warnings.
  - `main.js` has no test runner (known gap R3-002). The T2 scratch harness, rerun in headless Chrome
    with a sixth musician (`bad`, all rates 0), showed "silent" / `No audio received` for it and
    the five earlier cases unchanged, with no script errors.
- Startup-grace fix RED (`LINK_STATS_STARTUP_GRACE_MS` and the `onPacket` time argument added but
  unused; existing `LinkStatsTest` tests given packet times, and the ones that relied on reports
  with no packet before the grace now feed a packet or poll after the grace):
  `gradle -p apps/android :app:testDebugUnitTest` gave 81 tests, 3 failed:
  `noReportBeforeTheStreamStartsWithinTheStartupGrace`,
  `theFirstReportComesOneIntervalAfterTheStreamStartsAndCoversOnlyTheStream` and
  `withNoAudioTheFirstReportComesAfterTheGraceAndCoversAllOfIt` (each got a report where none was
  due yet). `afterTheGraceReportsFlowEveryIntervalAndAStreamThatStartsLateKeepsTheCadence` already
  passed and is kept as a guard for the cadence after the grace.
- Startup-grace fix GREEN:
  - `gradle -p apps/android :app:testDebugUnitTest assembleDebug` (Gradle 9.6.0 with Android
    Studio's JBR and the local SDK, as for T3): BUILD SUCCESSFUL, 81 tests, 0 failures; the debug
    APK assembled with no Kotlin warnings.
  - `cargo fmt --all -- --check`: exit 0 (no Rust changes).
