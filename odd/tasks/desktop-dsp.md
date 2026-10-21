# Desktop redesign, phase 5: processing (DSP)

## Objective

Give every input a console-style channel strip (high-pass, EQ, gate, compressor) shared by all
mixes, a limiter on every mix's master, shared reverb and delay with per-channel sends and a
return level per mix, strip presets, and know the engine's load and the latency it adds.

## Problem

- The mix only applies gain, pan, mute and solo, then hard-clamps to the ceiling
  (`apps/server/src/mix.rs:1128-1147`); there is no filter, dynamics or effect anywhere.
- Gains step at block boundaries with no ramp, so moving a fader can click (zipper noise).
- The single engine thread has no load measurement; overload only shows later as dropped blocks
  (`pipeline.rs:107-120`).
- Measured end to end latency is about 37 ms against a 25 ms target, 40 ms acceptable
  (`odd/tasks/latency-measurement.md:122-124`, `PRD.md:60`).

## Why

Phase 5 of the plan (`odd/tasks/desktop-console.md:45`). `PRD.md:18` kept advanced processing out
of v1; this phase brings it in and updates the PRD.

## Design decisions

- **Per-input processing, shared (user decision, 2026-10-19)**: one processing stage per input
  channel, before the mixes, like a mixing console. Every musician hears the processed channel.
  Edited only at the desk; the phone protocol and phone UI do not change. Cost scales with the
  number of inputs (up to 32), not with the number of musicians.
- **Where it runs**: a stage in `Pipeline` between the block pop and the sinks, writing a second
  preallocated buffer. The recorder keeps receiving the raw block, so input tracks stay "as
  captured"; the local mix track is the processed mix. Soundcheck and demo blocks go through the
  stage like live input.
- **Exact bypass**: a strip with neutral settings leaves samples bit-identical, so the golden
  float-mixer contract (`mix.rs:1288-1306`) and the phones' packets stay unchanged until something
  is turned on.
- **Real-time rules**: coefficients and state are computed and allocated off the engine thread and
  swapped through a command ring with a return ring (the `output.rs:373-458` pattern); the engine
  thread never allocates, locks or frees. Parameter changes are smoothed so they do not click.
- **Strip**: high-pass filter, 4-band parametric EQ (low shelf, two peaks, high shelf; RBJ
  biquads), gate (threshold, range, attack, hold, release), compressor (threshold, ratio, knee,
  attack, release, makeup; no lookahead). Order: HPF, gate, EQ, compressor. A stereo-linked pair
  shares one detector.
- **Master limiter**: per mix, replacing the hard clamp at the ceiling with a peak limiter with no
  lookahead (fast attack, release), so it adds no latency. Engineer-only, like `EngineerControls`.
- **Effects**: two shared buses (reverb, delay) fed once by per-channel send levels set at the
  desk, and a return level per mix (engineer-only). One instance of each effect, allocation-free
  delay lines.
- **Latency**: IIR filters, gate, compressor and the limiter add 0 ms; effects are wet returns,
  not path latency. Documented and re-measured live at the end.
- **Gain smoothing**: a per-block linear ramp when a gain changes; steady state stays
  bit-identical, so the golden contract only changes for blocks where a gain moved, on purpose.
  Rule (`MixRamp`, `mix.rs`): each mix keeps last block's per-channel weights (gain, 0 when not
  heard, and the pan weights); a channel whose weights differ moves linearly to the new ones,
  frame `i` of `n` at `(i + 1) / n` of the way, so the block ends on the new values; a channel
  going silent keeps its last pan, one coming in takes its new pan. Nothing changed: the block is
  mixed by `mix_into`, bit for bit. First block and a channel-count change: unramped. The ceiling
  clamp is not ramped. Each musician's state lives on their target entry; the local output and the
  recorder's local mix have their own (the recorder's resets per take).
- **Engine load**: each block timed on the engine thread with `Instant` from the pop (or the
  demo's or take's fill) to the return of the last sink, as a percentage of the block's period;
  published by atomics as `EngineStatus.load` (`EngineLoadStatus { percent, average_percent,
  peak_percent, late_blocks }`; desktop DTO `load { percent, averagePercent, peakPercent,
  lateBlocks }`): last block, an exponential average with a one-second audio-time constant, the
  peak of the last two to four seconds (max of the current and previous 2 s windows), and blocks
  over 100%. The desk shows "Engine load 8% (peak 15%)" (average and peak), warning above 70%.

## Constraints

- Clean-room: never copy StageWave code, UI, name or assets.
- Old phones keep working; no PMON or control protocol change for processing.
- ASIO is never chosen by default and never enumerated implicitly.
- The engine thread and audio callbacks never allocate, lock or panic.
- **Build safety**: `CARGO_BUILD_JOBS=2`, gradle `--max-workers=2`, one build at a time, never
  `cargo clean`, stop and report on a compiler crash or a corrupt generated file. The F: drive is
  nearly full: `CARGO_INCREMENTAL=0`, no release builds without the user's approval, stop on "no
  space" (os error 112).

## Scope

Load measurement, gain smoothing, the DSP module, the per-input stage, its desk side and editor,
strip presets, the master limiter, the effect buses, the PRD and docs, a latency check and a live
test.

## Acceptance criteria

- The status shows the engine's load (block processing time as a share of the block period, with
  a peak); a release benchmark gives the cost of 32 processed channels with 16 mixes.
- Neutral strips leave the phones' packets byte-identical; moving a fader does not click.
- Each input has HPF, EQ, gate and compressor editable from the desk, with a frequency-response
  view and gain-reduction meters; settings persist in the session and strip presets.
- Every mix has a master limiter that keeps peaks at the ceiling without clipping artefacts.
- Reverb and delay can be sent from any channel and returned per mix.
- No added latency from processing (documented), and the live test sounds right on the S23.

## Tasks

- [x] T1 (load): engine load measurement in the status, release benchmark example. Route:
  delegated writer (trigger: 2+ non-trivial files across the server, the desktop DTO and the UI).
  Commits d7ffef8 `feat(engine): measure how long each block takes` (+655/-8) and 0b27bf3
  `feat(desktop-ui): show the engine's load` (+66). RED: the six `load::tests` meter tests
  (injected durations) and `pipeline::tests::the_engine_thread_times_every_block_and_counts_the_late_ones`
  failed against a stubbed `LoadMeter::block` (5 of 6 meter tests and the pipeline test, all
  reading zeros), then passed. The UI view tests were written with the view, not observed red.
  No allocation with the measurement on: the existing engine-thread no-allocation tests
  (capture, demo, network, recorder) pass with it. Benchmark `engine_load` (32 inputs, 16
  musicians, 256-frame blocks, 10 s paced, loopback phones), debug build only, pessimistic:
  1876 blocks, mean 1973 µs (37.0% of 5333 µs), p99 3125 µs (58.6%), max 3956 µs (74.2%); the
  engine's own meter: average 36.9%, peak 74.1%, 0 late blocks; 30016 packets sent, 0 discarded.
  The release run is pending the user's approval.
- [x] T2 (smoothing): gain ramp in MixPlan with the golden contract adjusted for moving gains.
  Route: delegated writer (trigger: mixer, three sinks and the target table). Commit: this commit.
  RED: `a_fader_move_glides_across_the_next_block_then_holds`, `a_mute_fades_out_and_back_in_instead_of_clicking`,
  `a_solo_a_hidden_channel_and_the_master_glide_too` and `a_pan_move_glides_between_the_sides`
  failed against a `mix_ramped` that did not ramp, then passed; once the sinks ramped,
  `network::tests::phones_receive_the_same_packets_as_from_the_16_bit_mixer` failed (it moved the
  mix every block) and now compares each drawn mix on its second, steady block, after a gliding
  one. The steady golden (`a_steady_mix_is_the_unramped_mix_bit_for_bit`, 1000 drawn mixes over
  three blocks and after a move), the reset and layout test, `ramping_never_allocates` and
  `a_fader_move_reaches_the_phone_as_a_glide` pass; the 3000-mix float/16-bit golden is unchanged.
- Verification after T1 and T2: `cargo test -p personal-monitoring` 533 passed;
  `cargo test -p mixlink-desktop` 307 passed; `cargo check -p mixlink-desktop --all-targets`
  clean; UI `node --test apps/desktop/ui/tests` 542 passed. Builds ran with `CARGO_BUILD_JOBS=2`;
  the F: drive filled up (25 MB free), so `target/debug/incremental` (a regenerable cache, not a
  `cargo clean`) was removed and later builds used `CARGO_INCREMENTAL=0`.
- [x] T1/T2-fix (review-9d72c382f294a073's two SUGGESTIONs). Route: delegated writer (with T3).
  Commit 535aa1e `fix: settle the load warning on shown values and bound the bench` (+149/-28):
  `loadView` decides the warning on the rounded numbers it shows (70.4% reads "70%" and does not
  warn; 70.5% reads "71%" and does); RED: the new 70.4 case failed (`warning: true`), then passed.
  `LoadBenchOptions::validate` bounds the bench (inputs 1-32, musicians 0-64, seconds above 0 and
  at most 600, block 1-8,192 frames) and `run_engine_load` and the example's parser (whole numbers
  now, no `as usize` cast) refuse anything past them with a usage error; tests
  `the_bench_refuses_options_outside_its_bounds` and
  `a_run_out_of_bounds_is_an_error_before_it_allocates` (written with the fix). No port fallback
  on 127.0.0.1: the registry keeps one target per IP, so on a host that only answers on 127.0.0.1
  (macOS by default) a second musician is an error that says to run with `--targets 1` or add
  loopback aliases; documented in the example and `run_engine_load`.
- [x] T3 (dsp module): biquad EQ and HPF, gate, compressor, limiter; allocation-free, exact bypass,
  tests. Route: delegated writer (trigger: four new non-trivial files, `apps/server/src/dsp/`).
  Commits 6bbb70e `feat(engine): biquad filters and the channel equaliser` (+1077),
  7996fee `feat(engine): gate, compressor and limiter` (+1038/-2) and this commit (the strip).
  Pure module, not on the audio path yet (`#[allow(dead_code)] mod dsp`; T4 wires it).
  - API: settings are serde plain data (camelCase, missing fields defaulted): `StripSettings
    { eq: EqSettings { highPass, bands[4] }, gate, compressor }`, `LimiterSettings`; each has
    `clamped()`, `is_neutral()` and `design()` (off the engine thread: f64 RBJ coefficients,
    linear thresholds, time constants). Processors `Biquad`, `Eq`, `Gate`, `Compressor`,
    `Limiter`, `StripProcessor` are made in `new(channels)` (1, or 2 linked), take a design with
    `apply` (copies numbers) and `process(block, stride, first)` in place on interleaved frames,
    so a stage can process one input of a whole capture block. Gain reduction: positive dB f32s
    (`gain_reduction_db`, `gate_reduction_db`, `compressor_reduction_db`) for atomics as bits.
    `EqSettings::magnitude_db(freq)` gives the response curve.
  - Ranges (clamped; NaN takes the default): HPF 20-2,000 Hz, Q 0.5-2 (80 Hz, 0.707, off); bands
    20-20,000 Hz, ±18 dB, Q 0.3-10 (low shelf 100 Hz, peaks 400 and 2,500 Hz, high shelf 8 kHz);
    gate threshold -80-0 dBFS, range 0-80 dB, attack 0.05-50 ms, hold 0-2,000 ms, release
    5-4,000 ms; compressor threshold -60-0 dBFS, ratio 1-20, knee 0-24 dB, attack 0.1-200 ms,
    release 10-2,000 ms, makeup 0-24 dB; limiter ceiling -24-0 dBFS, release 1-1,000 ms.
  - Detectors: one per linked pair on `max(|L|,|R|)`. Compressor: feed-forward on the sample
    peak, soft knee in dB (Giannoulis/Massberg/Reiss), reduction smoothed in dB (attack up,
    release down). Gate: a peak envelope with a fixed 10 ms fall (no chatter on low notes), open
    at or above the threshold plus hold, closing by `range`. Limiter: instant attack to the
    ceiling, release on the attenuation (`1 - gain`, kept as the distance so f32 rounding cannot
    stall it short of 1), and a final clip at the ceiling for rounding. Peak, not RMS: no window,
    and transients are what a monitor mix must not let through.
  - Exact bypass: a neutral or disabled processor does not touch the samples (tests compare bits
    on a signal with ±0, 1e-30, `f32::MIN_POSITIVE`, -1.5, a tone and noise). Turned off while
    working, each glides out (filters to the identity, reductions to 0 dB over 20 ms, makeup to
    0 dB) and then stops touching them. Smoothing: coefficients and makeup glide linearly over
    480 frames (10 ms); a linear mix of two stable biquads is stable (the stability triangle is
    convex). Denormals: biquad state is zeroed when both values fall below 1e-20 together (zeroing
    one alone kept a resonant filter ringing at the floor, caught by the tail test), envelopes are
    flushed, reductions snap to their target within 0.001 dB.
  - RED: against stubs (`magnitude_db` returning 0, `flush_denormal` passing values through) the
    response, running-response, EQ curve and subnormal tests failed (5), then passed; against
    stubbed dynamics (no smoothing, a flat static curve, no limiter gain, no flush, glides that
    jump) 7 failed; with glides alone jumping, the biquad and makeup glide tests and the
    switch-off test failed (both glide tests first passed by switching on a zero crossing, so they
    now switch on a crest); with the strip's order reversed both order tests failed; then all 39
    `dsp` tests pass. The limiter's switch-off test found the f32 stall above.
  - No allocation: `applying_and_running_never_allocate` (Eq, dynamics) and
    `applying_and_running_a_strip_never_allocates` count 0 allocations and frees.
  - Micro benchmark of one strip on 32 channels: not added (optional); T4's `engine_load` run
    with strips on measures it in the real stage.
- [x] T3-fix (review-ca13706495195c9e's two SUGGESTIONs). Route: delegated writer (with T4).
  Commit 94b0a1f `fix(engine): retarget every coefficient together and survive non-finite
  samples` (+219/-4). `Biquad::set` restarts all five coefficients from where they are
  (`Glide::restart`) on any new target, so a change mid-glide moves them along one straight line
  and lands them on one frame. A working strip and the limiter take NaN and infinities as silence
  and count them (`non_finite_samples`), and a biquad whose state stops being finite starts again
  from silence and flushes that block's non-finite outputs. RED: the four new tests
  (`a_setting_changed_mid_glide_moves_every_coefficient_on_together`: `a1` landed on frame 239, not
  479; `a_filter_poisoned_by_a_non_finite_sample_recovers`; the limiter's and the strip's NaN/inf
  tests) failed against stubs (old `set`, counters returning 0, no sanitising), then passed; 43
  `dsp` tests, 578 server tests.
- [x] T4 (per-input stage): the stage in `Pipeline`, engine API, command ring, raw block to the
  recorder, post-processing meters and gain reduction. Route: delegated writer (trigger: a new
  module plus the pipeline, engine, recorder, bench and desktop DTO). Commit: this commit.
  - Stage (`apps/server/src/processing.rs`): `deliver` in `Pipeline` runs `ProcessingStage::process`
    between the pop (or the demo's or take's fill) and the sinks. `Block` now carries `samples`
    (processed; what the network, local output and the recorder's local mix read) and `raw` (as
    captured; the recorder's input tracks). While every strip is idle the stage returns `None` and
    does nothing, not even the copy: `samples` is `raw`. One strip per input for the first 32,
    a linked pair (`linked_to_next`, from the fitted channel details) one stereo strip on its first
    side's settings; a pair's sides keep equal settings.
  - Real time: designs go as plain numbers (`ProcessingCommand::Design`) and a new layout as a
    whole `StripSet` built off the engine thread (`ProcessingCommand::Strips`) through a 256-slot
    rtrb ring; the replaced set comes back through the return ring and is freed by the engine side.
    Strips on the same channels in both layouts carry their running state over. `EngineHandle`'s
    `publish` re-lays the strips after a device switch, a channel-count change or a pair (un)linked;
    the processed buffer is sized in `set_source`, like the block.
  - Engine API: `set_channel_processing(channel, StripSettings) -> StripSettings` (clamped),
    `channel_processing(channel) -> Option<StripSettings>`, `processing() -> Vec<StripSettings>`
    (32, CH 1 first) and `replace_processing(&[StripSettings])` (session load; channels past the
    list neutral). Settings are kept per channel index for the whole run. Exported:
    `StripSettings`, `EqSettings`, `EqBandSettings`, `HighPassSettings`, `GateSettings`,
    `CompressorSettings`, `LimiterSettings`.
  - Meters: `EngineMeters { processed_levels, gate_reduction_db, compressor_reduction_db }` (a
    channel whose strip is idle reads its input level); `EngineStatus.non_finite_samples`. Desktop
    `MetersDto { processed, gateReductionDb, compressorReductionDb }`.
  - RED: the 13 new tests (9 `processing::tests`, the recorder's raw/processed tracks, the
    network's neutral golden, the engine's device-switch and meters tests) all failed against stubs
    (`process` always `None`, the layout ignoring pairs, the recorder reading `samples`; the
    device-switch test saw a 16,384 peak against its bound of 500), then passed. Existing goldens
    unchanged: `phones_receive_the_same_packets_as_from_the_16_bit_mixer`,
    `a_steady_mix_is_the_unramped_mix_bit_for_bit`; new
    `neutral_channel_strips_leave_the_phones_packets_byte_for_byte` (200 packets through the stage
    from the capture callback, a pair laid out). No allocation:
    `the_engine_thread_never_allocates_with_every_strip_working_and_settings_changing` (32 working
    strips, three pairs, a design or a new layout between each of 200 blocks) counts 0 and 0.
  - Bench: `engine_load --strips` runs `bench::typical_strip()` (HPF 80 Hz, -3 dB at 400 Hz,
    +2 dB at 8 kHz, compressor -30 dBFS 3:1) on every input. Debug run once (32 inputs, 16
    musicians, 256-frame blocks, 10 s; pessimistic): mean 6,059 µs (113.6% of 5,333 µs), p99
    8,886 µs (166.6%), max 10,275 µs (192.7%); the meter: average 113.2%, peak 177.8%, 1,870 late
    blocks; 30,016 packets, 0 discarded. Against T1's 37.0% without strips, the 32 strips cost
    about 4.1 ms per block in debug (about 128 µs each); a release run is needed for a real figure.
  - Counts: 591 server tests, 307 desktop tests, `cargo check -p mixlink-desktop --all-targets`
    clean.
- [ ] T5 (desk): session, commands and strip presets for channel processing. Route: delegated
  writer.
- [ ] T6 (editor): the channel processing editor (EQ curve, dynamics, meters), opened from the
  Channels screen and the strip. Route: delegated writer.
- [ ] T7 (master limiter): per mix, engine, desk and master strip. Route: delegated writer.
- [ ] T8 (effects): reverb and delay buses, per-channel sends, per-mix returns, editor. Route:
  delegated writer.
- [ ] T9 (close): PRD update, docs, latency check, live test with the S23. Route: inline or writer.

## Delivery

Strategy `ask-on-risk` with chain strategy `stacked-to-main` (as in phases 1-4); one pull request
per reviewed slice, only with the user's approval.

## Progress

- Branch `feat/desktop-dsp` from main ea71cde (phase 4 merged as PRs #57-#68).
- Exploration done (read-only map with file evidence); the user chose per-input processing,
  shared by all mixes.
- Review review-9d72c382f294a073 (ea71cde..052c897, T1 + T2): approved and acknowledged; its two
  SUGGESTIONs (the load warning on unrounded values, the bench's unbounded options) fixed in
  535aa1e. Next review base: 052c897.
- Verification after T1/T2-fix and T3: `cargo test -p personal-monitoring` 574 passed; UI
  `node --test apps/desktop/ui/tests` 542 passed; `cargo check -p mixlink-desktop --all-targets`
  clean. Builds with `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, no release builds.
- Review review-ca13706495195c9e (052c897..06bdde6, T1/T2-fix + T3): approved and acknowledged;
  its two SUGGESTIONs (coefficients retargeted apart mid-glide, non-finite samples) fixed in
  94b0a1f. Next review base: 06bdde6.
- Verification after T3-fix and T4: `cargo test -p personal-monitoring` 591 passed;
  `cargo test -p mixlink-desktop` 307 passed; `cargo check -p mixlink-desktop --all-targets`
  clean; the debug `engine_load --strips` run above. UI tests not run (no UI change).
- Disk: the F: drive is nearly full (2.4 GB free before T3, 1.4 GB after; each test build of the
  server takes a few hundred MB). Keep builds to debug, one at a time, and stop on "no space".

## Next step

T5 (desk: session, commands and strip presets for channel processing, on the T4 engine API). The
release runs of `engine_load` (with and without `--strips`) still wait for the user's approval.
