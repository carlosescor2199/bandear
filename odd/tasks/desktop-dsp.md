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
- [x] TA (ASIO safety port): the still-relevant server fixes from the two unmerged local branches
  `fix/audio-panel-review-findings` (e67d8ec) and `fix/asio-switch-waits-for-pick` (d0d1fc7),
  hand-ported onto the rewritten `engine.rs` and `capture.rs`. Route: delegated writer (trigger:
  a vendored crate plus capture, engine, output and CLI changes).
  - Commits: e1f36b8 `build: vendor cpal 0.18.2 with ASIO open-by-name` (third-party, 80 files,
    +29,932 vendored lines plus the workspace wiring), 08d632d `fix(server): do not panic on an
    inverted driver buffer range` (+16/-2, from a8c2483), a3c7657 `fix(server): refuse ASIO without
    a picked driver and never fall back silently` (+152/-32, from aae3ca3 + 26574e7) and this commit
    `fix(server): open an ASIO driver by its registry name` (from 545df12 + c63246f + 4daf017 +
    ee613c7).
  - Ported: `clamp_buffer_frames` leaves the request unchanged on an inverted driver range
    (`min > max`) instead of panicking in `u32::clamp` (start, switch and the ASIO local output).
    `require_asio_device` refuses a start or a live switch under ASIO with a missing or blank driver,
    first thing in `start` and right after the demo check in `switch_device`, so `--host asio`
    without `--device` no longer opens the first installed driver. `input_host` returns a `Result`:
    a failed ASIO host is the error `ASIO is unavailable (...)` (kept for the session in a
    `OnceLock<Result<..>>`, `resolve_asio_host`), with no fallback to `cpal::default_host()`, and a
    build without ASIO says so. Under ASIO the engine never calls `input_devices()`:
    `device_lookup(host)` (`AsioByName`, `Enumerate`, `NoDevice` for the demo) is the one decision
    `start` and `switch_device` share; `open_asio_device` resolves the filter against the registry
    (`resolve_asio_driver_name`: an exact case-insensitive name wins, otherwise exactly one
    substring match, an ambiguous or unknown filter is an error naming the candidates) and opens that
    one driver through cpal's `device_by_id` (`open_named_asio_driver`, testable with a fake
    opener). The ASIO device list is the registry only: the fallback to cpal's enumeration when the
    registry read gave no names is gone. Unchanged: ASIO is never the default or enumerated
    implicitly, the desktop's T7b start-request builder (`apps/desktop/ui/app/lib/engine-config.js`)
    stays the only UI start path, and the demo source path is untouched (its own early returns run
    before any of this).
  - Dropped: 15ae80d (one `planned_buffer_frames` so the startup log shows the clamped buffer;
    cosmetic, and the current engine already defaults a missing request to 256 in `start`),
    fe9a72a, edcec27, 2bdefaa and a01dfc4 (the old window's audio selects and driver pick, replaced
    by the T7b builder). The branches' `vendor/README.md` and
    `.gitattributes` (`vendor/cpal/** -text`) were not ported (outside this task's edit surface); the
    vendored blobs were copied from the branch as-is, so they are byte-identical to it.
  - Vendored cpal: `vendor/cpal` is the crates.io package cpal 0.18.2 (the version in `Cargo.lock`)
    plus a patch of +71/-40 lines in `src/host/asio/device.rs` and `src/host/asio/mod.rs`, each
    change marked `MixLink patch`: the per-driver description moved unchanged into
    `Device::from_driver`, a new `device_by_name` loads and describes one driver, and the ASIO
    `HostTrait` overrides `device_by_id` to call it instead of the trait default that walks
    `devices()`. Why: cpal 0.18.2 lists ASIO devices by loading every installed driver in registry
    order, and the first one here, Realtek ASIO, has bugchecked this PC. Wired in with
    `[patch.crates-io] cpal = { path = "vendor/cpal" }` and `exclude = ["vendor/cpal"]` in the root
    `Cargo.toml`; the lock entry lost its registry source and checksum. Verify the copy with
    `diff -r ~/.cargo/registry/src/index.crates.io-*/cpal-0.18.2 vendor/cpal` (only `.cargo-ok`
    missing plus the two patched files). Drop it, and bump `cpal`, once a cpal release opens one
    ASIO driver by name; offering the `device_by_id` override upstream is an open follow-up. As a
    path dependency its lints are no longer capped: a build without the `asio` feature shows one
    upstream `frames_to_duration` dead-code warning.
  - RED: `an_inverted_driver_range_passes_the_request_on_instead_of_panicking` panicked
    (`min > max. min = 512, max = 128`), then passed. The ASIO guard tests
    (`an_asio_session_must_name_its_driver`, `other_hosts_may_open_their_first_device`,
    `an_asio_host_that_cannot_load_is_reported_instead_of_replaced`,
    `the_platform_host_is_always_available`) failed to compile (9 errors: no `require_asio_device`,
    no `resolve_asio_host`, `input_host` not a `Result`), then passed;
    `an_asio_start_without_a_driver_is_refused_before_anything_opens` failed with the start guard
    removed (`invalid --target`), then passed. `under_asio_a_switch_without_a_driver_is_refused_and_the_capture_kept`
    was not run red on purpose: without the guard that switch would enumerate the real ASIO
    drivers. The open-by-name tests (`asio_is_opened_by_name_and_never_enumerated` for ASIO, Auto,
    WASAPI and Demo; the five `resolve_asio_driver_name` cases: exact, single substring, ambiguous,
    unknown or none installed, blank; `only_the_resolved_asio_driver_is_opened`,
    `an_unresolved_asio_filter_opens_no_driver_at_all` and
    `an_asio_driver_that_does_not_open_is_named_in_the_error` with a fake opener) failed to compile
    (18 errors), then passed. Every test is a pure helper or uses the device-less test engine; no
    test touches an ASIO host.
  - Checks: `cargo test -p personal-monitoring` 608 passed; `cargo test -p mixlink-desktop` 307
    passed; `cargo check -p mixlink-desktop --all-targets` clean; `cargo check -p
    personal-monitoring --no-default-features` builds (only the pre-existing
    `parse_asio_driver_names` and the vendored `frames_to_duration` dead-code warnings).
  - Open follow-ups: `R3-asio-load-error-discarded` (`device_by_name` turns every load failure into
    `None`, so "failed to load", "no channels" and "another driver is loaded" read the same; telling
    them apart needs a wider cpal patch); `R3-registry-failure-misreported` (a failed `reg query` and
    a machine with no ASIO drivers both read as "no ASIO drivers are installed"); offering the cpal
    patch upstream; `R3-zero-channel-devices-selectable` (devices listed with 0 channels, every ASIO
    registry entry included, can still be picked).
  - Not exercised on real hardware yet: needs a live test with the Volt under ASIO, with the user's
    go-ahead.
- [x] T5 (desk): session, commands and strip presets for channel processing. Route: delegated
  writer (trigger: session, sessions, a new command module, the saver, lib.rs and the console's
  store, actions and fake bridge). Commits dac564b `feat(desktop): keep each channel's processing
  in the session` (+1553/-72), ae49ddc `feat(desktop): strip presets and the equaliser's response`
  (+662/-9) and this commit `feat(desktop-ui): channel processing in the console`.
  - Session (`session.rs`): `processing` is each input's strip (`StripSettings`, camelCase) by its
    1-based channel number as a string key (`{"1": {...}}`), only strips that do something; absent
    while every strip is neutral; read on its own like every other section (one that cannot be read
    is set aside in `session.json.processing-damaged-<ms>`); a strip may leave out any field. Version
    unchanged (added within version 1). `stripPresets` (`[{ name, settings }]`) is the desk's own,
    like `recording`: a named session never carries it and `SessionStore::replace` keeps the desk's.
  - Named sessions carry the processing (it is part of a show): a save takes the engine's strips
    (`DeskView.processing`) when one runs, else the session's; a load replaces every strip through
    `replace_processing` right after the channel layout (engine lock first, session inside), and a
    show saved without processing loads with every strip neutral. A start hands the new engine the
    session's processing (a change not written yet included) after `hand_session_layout`.
  - Saving: a change is marked on a `ProcessingSaver` and written off the engine lock at most every
    400 ms (`local::saver::Saver<T: Kept>`, now generic; the local mix saver is `Saver<SessionMix>`).
    Stop and quit flush it; a start reads it paused; a load pauses it and drops the change waiting.
  - Commands (`commands/processing.rs`, channels 1-based): `processing` (all 32 `{ channel,
    settings, active }`), `channel_processing(channel)`, `set_channel_processing(channel, settings)`,
    `reset_channel_processing(channel)`, `copy_channel_processing(from, to[])`, `strip_presets`,
    `save_strip_preset(name, settings)`, `delete_strip_preset(name)`, `apply_strip_preset(name,
    channel)` and `eq_response(settings: EqSettings, points?)` (`[{ freqHz, db }]`, 128 points by
    default, held to 2-1,024, log grid 20 Hz-20 kHz, `EqSettings::magnitude_db` on the command's
    thread). Changes answer `{ outcome: set | invalid | notFound, channels, message }`, `channels`
    every strip that changed (a linked pair's other side included); presets `{ outcome: saved |
    replaced | deleted | invalid | factory | notFound, preset, presets, message }`. Changes need a
    running engine (an error otherwise); reads fall back to the session. Stand-in trait
    `ProcessingEngine`.
  - Presets: built-in "Vocal", "Kick", "Bass DI", "Acoustic guitar", "Gentle compression"
    (clean-room values of our own, all in range and not neutral), listed first, never written, never
    deleted. Decision: a user preset can never take a built-in name (any case; `invalid`), so a
    built-in name always means the built-in strip; saving under one of the engineer's own names
    replaces it in place (`replaced`). Names 1-40 characters, trimmed, matched in any case.
  - Console (`ui/app/lib`): `processing.js` (defaults, ranges and `clampStrip`, `isNeutralStrip`,
    `eqMagnitudeDb`/`eqResponseOf` with the same RBJ biquads, `formatProcessingSet`,
    `formatStripPresetsSet`); store slices `processing`, `processingPending`, `processingError`,
    `stripPresets`, `eqCurves` and the selector `channelStrip(state, channel)`; console actions
    `loadProcessing`, `editProcessing(channel, settings)` (a per-channel coalescer, the latest edit
    always sent, resolves `{ ok, message }`), `resetChannelProcessing`, `copyChannelProcessing`,
    `applyStripPreset` (in turn with desk edits and loads), `loadStripPresets`, `saveStripPreset`,
    `deleteStripPreset`, `loadEqResponse(channel, eq, points)`; a load drops the strip edits waiting
    and reads the strips again. The fake bridge mirrors every command (pairs from its channel
    table, the built-in presets as `factoryStripPresets()`, the processing in its saved sessions)
    and its meters carry `processed`, `gateReductionDb` and `compressorReductionDb`.
  - RED: against stubs (the store's processing and preset methods doing nothing, `read_session` not
    taking the sections, `replace` not keeping the presets, `session_processing` empty,
    `processing_list` neutral, `eq_response` at 0 dB) 16 of the 18 new Rust tests failed (the
    out-of-range channel and built-in preset data tests passed), then all passed. UI: against stubs
    (`eqMagnitudeDb` at 0, the fake's pairs ignored, the console never sending a strip edit) 3 of the
    11 new tests failed, then passed; the other UI tests were written with the code.
  - Counts: `cargo test -p mixlink-desktop` 325 passed (307 before); `cargo check -p
    mixlink-desktop --all-targets` clean; UI `node --test apps/desktop/ui/tests` 553 passed (542
    before). Server untouched. Commit: this commit.
- [x] T5-fix (review-62e9809767b1790e's and review-ae983bec860aca54's findings: five WARNINGs, two
  SUGGESTIONs). Route: delegated writer (with T6; trigger: Rust commands plus the console's store and
  actions). Commits cf238a4 `fix(desktop): copy a strip atomically and clamp the response request`
  (+273/-25) and 7b25b2d `fix(desktop-ui): keep strip edits honest around loads and pairs`
  (+234/-34). A previous writer's run was cut off by a machine crash; its uncommitted Rust half was
  checked (no NUL bytes), formatted, tested and committed as cf238a4.
  - `copy_channel_processing` reads its source and writes its targets under one hold of the engine
    lock (`apply_on`); `eq_response` holds the settings in range (`EqSettings::clamped`) before
    `magnitude_db`, so a zero, negative or NaN frequency or Q never reaches the window as `null`; a
    load whose strips the engine refuses marks the strips it still plays (`processing_kept`) for the
    session instead of dropping the change waiting.
  - Console: a strip edit held back by a session load answers once the load is over (dropped and
    said so: "CH 1's processing was not changed: a session was loaded meanwhile.", or sent when the
    load was refused), and edits made after the load landed go out; an answer before the strips were
    ever read keeps `processing` null and reads them all (no 32 made-up neutral strips, the
    connect-time read not blocked); a load whose strips cannot be read sets `processingError`
    (`channel: null`), leaves `processing` unknown and says so in the load's answer; reset, copy and
    preset wait for a linked partner's edit too (`stripUnitChannels`).
  - RED: the five new UI tests failed against the old console (the edit reported `ok` with no send,
    32 made-up strips and no read, the load's answer silent, the reset sent before CH 4's edit), then
    passed. The three Rust tests (`a_change_to_the_source_sent_during_a_copy_lands_after_the_copy`,
    forced interleaving with the stand-in's read park over 8 rounds;
    `the_equaliser_response_of_settings_out_of_range_is_finite`;
    `a_load_whose_strips_the_engine_refuses_keeps_the_strips_it_plays`) were written with the fix
    and not run red, to keep compiles to a minimum on this machine.
- [x] T6 (editor): the channel processing editor (EQ curve, dynamics, meters), opened from the
  Channels screen and the strip. Route: delegated writer (trigger: new pure modules, two organisms, a
  screen and its sheet, the router, the shell and both entry points). Commits 21b9f98
  `feat(desktop-ui): the processing editor's logic` (+1,247) and this commit
  `feat(desktop-ui): the channel processing editor`.
  - Route `#/processing` ("Processing" in the sidebar, between Channels and groups and Network and
    system, as the design's board), `#/processing/5` opens CH 5 (`routeChannel`, `processingHash`,
    `canonicalHash`; the shell keeps the channel in the address). Opened from each row of the
    Channels and groups table (a "Processing" link with "On"/"Off" in words) and from every Mix
    channel strip ("Processing" / "Processing on"); a pair opens on its first side.
  - The design's processing board was followed for layout (one bar of actions, the equaliser across
    the width with its band panels, the gate and compressor side by side); its per-musician
    selector, effect sends, reverb and delay are not built (per-input processing was the user's
    decision; effects are T8).
  - Editor: channel picker with previous and next (a stereo pair once, "CH 3–4 · Keys"), state in
    words ("Processing on", "No processing", "Bypassed"); Bypass (A/B) keeps the strip in the editor,
    sends a neutral one and puts it back (also on leaving the screen; a strip changed elsewhere
    while bypassed ends the bypass and is not overwritten); Reset; Copy to… (a list of the other
    strips); Presets (apply, save as, delete; built-in ones marked "(built-in)" and not deletable).
    Input and processed meters. Equaliser: `<ml-eq-graph>` (log 20 Hz-20 kHz, ±18 dB, grid with
    labels) with a handle per part: drag sets frequency and gain (the high-pass only frequency) and
    turns it on, wheel or Page Up/Down change Q, arrows move it (Shift for octaves and 3 dB), Home
    sets 0 dB, Enter turns it on or off; the curve is worked out at once in the window and replaced
    by the desk's `eq_response` once the edits pause 120 ms (`curveSource` keeps the desk's curve
    only for the equaliser it was asked for). Band panels with On, frequency, gain and Q sliders
    (log scales for frequency, Q and times). Gate and compressor with On, every setting as a
    slider with its value in words and `<ml-reduction-meter>` (gate 40 dB, compressor 20 dB full
    scale, filled from the right); the compressor's static curve (soft knee, makeup) in a small
    square. Text alternatives: every handle is named in words, the graph has the whole equaliser in
    words, the transfer curve its settings.
  - Pure logic in `lib/processing-view.js` (scales, handles, keys, words, slider scales, transfer
    curve, reduction, choices, presets, A/B) and `lib/processing-editor.js` (the editor's commands
    over the console app), tested in node; the screen and organisms only draw and forward.
  - RED: `processing-view.test.js` (17 tests) all failed against a stub module, then passed (an 18th,
    the slider definitions, was written with them); `processing-editor.test.js` (7) all failed
    against a stub, then passed; the router test failed to import (no `routeChannel`), then passed.
  - Browser check (`desktop-ui`, `?fake`): opened from the Channels table (CH 2) and from the Mix
    strip (Lead vox, CH 5); dragged the low peak to +12.3 dB at 868 Hz, wheel raised Q to 1.12;
    gate and compressor on with the gain-reduction meters moving (−40.0 dB, −9.3 dB); applied
    "Vocal" (built-in), saved and deleted "Snare crack", "Delete" disabled on built-in presets;
    copied CH 2 to CH 5; A/B bypassed (controls disabled, kept settings shown, reduction 0) and
    restored; two quick keys on a handle both landed after building gestures on the current strip
    (found here, fixed). Band panels reflowed to five across at 1,100 px. No console errors.
  - Counts: UI `node --test apps/desktop/ui/tests` 584 passed (553 before T5-fix); `cargo test -p
    mixlink-desktop` 328 passed (325 before); `cargo check -p mixlink-desktop --all-targets` clean.
    Commit: this commit.
- [x] T6-fix: the findings of review-90dc4abdb8816f9f and review-93fc890ef1905355. Route: delegated
  writer (with T7). Commits 3a044b2 `fix(desktop-ui): keep the bypassed strip until it is restored
  and guard unknown processing` and c7b03e2 `test(desktop): make the copy race deterministic`.
  - Bypass off keeps the kept strip, still bypassed, until the restore is answered; a refused
    restore leaves it bypassed with its settings kept.
  - The strip opened last is remembered per console app, not in module state.
  - The processing screen draws through `frameScheduler` (`lib/scheduling.js`), stopped for good on
    unmount, so a late answer neither draws on a removed screen nor rewrites the address.
  - The Mix strips and the Channels table say neither On nor Off while the strips are not read
    (`processingActive` null, `processingLink` "not read yet"); a pair opens on its first side
    (tests in `processing-editor.test.js`).
  - The copy race test waits for the competing change to reach the engine lock (no fixed sleep).
  - `<ml-reduction-meter>` announces the value it settles on (`trailingThrottle`).
- [x] T7 (master limiter): per mix, engine, desk and master strip. Route: delegated writer
  (trigger: the engine, the desk's commands and session, then the console's store, app, fake bridge,
  a pure module, an organism and the screen). Commits 992e418 `feat(engine): a limiter on every
  mix's master` (+1,055/-37), aa58567 `feat(desktop): keep each mix's limiter` (+766/-26) and this
  commit `feat(desktop-ui): limiter on the master strip`.
  - Design: each musician's mix, the local output and the recorder's local mix go through a peak
    limiter after the sum and before the clamp at the mix's ceiling, which stays as the last safety
    net. Its ceiling is the lower of its own (−24 to 0 dBFS, 0 by default) and the mix's (the lower
    of the phone's cap and the engineer's limit); release 1-1,000 ms (50 by default); no lookahead,
    no added latency. Settings live on the mix as atomics, the design worked out off the engine
    thread; the running state lives with each mixer's ramp, so two mixes never share it. Off by
    default and bit for bit as before. Engineer-only (`set_musician_limiter`, `set_local_limiter`),
    kept per musician by client id or address (the default kept as nothing) and in the session's
    local section, so named sessions carry them; the status reports each row's `limiter` and
    `localLimiter`, the meters each mix's `limiterReductionDb`.
  - Master strip (each musician's tab and the Local mix tab): a "Limiter" switch with On/Off in
    words, Ceiling and Release sliders with their values in words (`aria-valuetext`), a "Limiter
    gain reduction" meter (`<ml-reduction-meter>`, 12 dB full scale, 0 while off) and a line saying
    which ceiling wins (the mix's cap/limit, or full scale for the local mix). Changes show at once
    and go through a per-mix coalescer (`editMixLimiter`); a refusal is said above the strips.
  - Pure logic in `lib/limiter.js` (ranges, `masterLimiter`, `limiterChange`, where an arrow key
    that rounds back to the same value moves one unit, 0.5 dB or 1 ms; `limiterReduction`, words).
    The fake bridge keeps them as the desk does and limits a hot mix in its meters.
  - RED: `limiter.test.js` (12) against the inherited uncommitted module with stub `limiterChange`
    and `masterLimiter`: 6 failed (the view's positions, the control change, the fake's commands and
    session, the console's command and refusal; the fake also threw on an undefined default), 6
    passed (ranges, words, reduction, outcome, bridge, fake meters, written before by the previous
    writer); then all 12 passed. The local mix meters test now expects `limiterReductionDb: 0`.
  - Browser check (`desktop-ui`, `?fake`): Lead vocal's limiter on, ceiling to −18 then an arrow key
    to −17.5 dBFS, release 251 ms, gain reduction −8.0 dB with the note naming the 90% cap; the Local
    mix tab's own limiter on at −21.5 dBFS, 4 ms, reduction about −21 dB; back on Lead vocal its own
    settings kept. Processing editor: "Vocal" applied to CH 5, Bypass on ("Bypassed", settings kept)
    and off ("Processing on"); an edit and a bypass then leaving for the Mix screen at once left the
    address on `#/mix`. No console errors.
  - Counts: UI `node --test apps/desktop/ui/tests` 604 passed (592 before this commit);
    `cargo test -p mixlink-desktop` 333 passed; `cargo test -p personal-monitoring` 617 passed
    (`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, one at a time, once at the end). Commit: this commit.
- [x] T7-fix (review-186d441c6b486688's and review-8f20640b741f6075's findings: three WARNINGs, one
  SUGGESTION). Route: delegated writer (with T8). Commits b6bdff7 `test(desktop): prove the copy
  holds the engine lock` (+101/-86), 36f2173 `fix(desktop): hand every engine the limiters the
  session keeps` (+176/-47) and ff50994 `fix(desktop-ui): nudge the limiter only on keys and keep a
  set that read back badly` (+118/-24).
  - The copy race test parks the copy between its read and its write (a `#[cfg(test)]` hold on the
    copying thread, `between_read_and_write`), sends a change to the source and checks that the
    engine lock is still held (`try_lock` is `WouldBlock`) and the change does not land within
    50 ms; released, the copy writes the strip it read and the change lands after it (writes CH 5,
    CH 6, then CH 1). RED by mutation: with the copy reading and writing under two holds of the
    lock the test failed ("the change landed while the copy was between its read and its write"),
    then passed with the single hold restored.
  - Limiter keys: `limiterChange(settings, field, value, { key })` moves one unit past a rounding
    only for a key step; `<ml-master-strip>` marks an `input` that followed an arrow, Page or
    Home/End key (`limiter-edit { field, value, key }`). RED: a slow drag of 51 positions flipped
    between -12 and -11.5 dBFS ("flipped back at step 2"), then passed.
  - `sendLimiter` reads the status apart from the command: a set the desk answered is said as set,
    and when the status read after it fails the strip keeps the answered limiter
    (`limiterSettled(address, sent, kept)` keeps it on the row or `localLimiter`) instead of going
    back to the stale status. RED: the strip showed the old status's limiter, then passed. (The
    status read never threw, so the reported refusal itself could not happen; the stale strip
    could.)
  - One rule for the local limiter (`limiter::hand_kept_limiters`, used by a start and a load): the
    engine takes the musicians' limiters whole and the local mix the one the session keeps then. A
    session saved without a local section keeps the desk's own local section, limiter included, so
    the engine and the session always agree (before, the engine kept whatever it had). Tests: a
    load with a local section (its limiter, or off when it keeps none) and without one (the
    desk's), and what a start hands a new engine. RED by mutation (the old rule): the new load test
    and the load-order test failed, then passed; the start test was written with the helper.
- [x] T8 (effects): reverb and delay buses, per-channel sends, per-mix returns, editor. Route:
  delegated writer (trigger: two DSP modules, a new engine module and the stage, mixer, targets and
  engine; the desk's session, commands and DTOs; the console's lib, store, app, fake bridge, two
  screens and the master strip). Commits 5584583 `feat(engine): reverb and delay` (+1,185/-1),
  c99a50c `feat(engine): effect buses with sends and returns` (+1,527/-42), 9016db4
  `feat(desktop): keep sends, effects and returns` (+1,297/-18) and this commit
  `feat(desktop-ui): effect sends, the effects panel and returns`.
  - Reverb (`dsp/reverb.rs`, clean-room from the public Schroeder-Moorer/Freeverb description): a
    pre-delay line, eight damped feedback combs and four series all-passes a side (the published
    44.1 kHz lengths scaled to 48 kHz, the right side spread by 23 samples). `sizePercent` 0-100
    (comb feedback 0.70-0.98, 50 by default), `dampingPercent` 0-100 (40), `preDelayMs` 0-200 (10),
    `widthPercent` 0-100 (100). About 37,000 floats (150 KB).
  - Delay (`dsp/delay.rs`): two lines of 96,001 frames (2 s, 750 KB); `timeMs` 1-2,000 (375), or
    `tempoBpm` 30-300 (0 for none) with `note` (half, quarter, dottedEighth, quarterTriplet, eighth,
    eighthTriplet, sixteenth), `feedbackPercent` 0-90 (35), `highCutHz` 1-20 kHz on the repeats
    (6,000), `pingPong` (input to the left, each side feeding the other).
  - Both: allocated once at full size in `new` (off the engine thread, when the stage is made);
    feedback, damping, width and the high cut glide over 10 ms, a new pre-delay or delay time
    crossfades over 20 ms; values under 1e-10 (-200 dBFS) are written as zero, so a dead tail is
    exact silence and the effect goes idle once nothing else was written for longer than its lines.
  - Buses (`effects.rs`): each input's sends (`StripSettings.sends { reverb, delay }`, 0-100%) are
    taken after its strip and before every fader, a pair sharing them; the stage sums them into each
    effect's mono bus once per block (a changed send glides across the block) and runs each effect
    once (`ProcessingStage::run` gives the block and its `Wet`). Each mix (every musician's, the
    local output and the recorder's local mix) adds each effect at its return (0-100%, on its
    `MixState`, kept per musician like the limiter) times its master volume (nothing under the
    master mute), after the sum and before the limiter and clamp; a changed return glides. An effect
    with no send and an idle tail does not run (no `Wet`), and a mix that returns nothing is the
    ramped mix bit for bit: the goldens are untouched. No added latency: the effects are wet
    returns. `engine_load --fx` added (debug only, not run).
  - Engine API: `set_channel_send(channel, effect, level) -> StripSettings`, `set_reverb`,
    `set_delay`, `replace_effects`, `effects`, `set_musician_fx_return(key, effect, level) ->
    FxReturnsSetting`, `musician_fx_returns`, `replace_musician_fx_returns`, `set_local_fx_return`,
    `replace_local_fx_returns`, `local_fx_returns`; status `MusicianStatus.fx_returns`,
    `EngineStatus.local_fx_returns` and `.effects`. Exported `Effect`, `EffectLevels`,
    `EffectSettings`, `ReverbSettings`, `DelaySettings`, `DelayNote`.
  - Desk: sends persist with the strip (a strip that only sends is kept: `is_untouched`), the
    effects in a new `effects` section, the returns as `fxReturns` on each musician's entry and in
    the local section; named sessions carry them; a start and a load hand them over
    (`effects::hand_kept_effects`, the local returns by the same rule as the limiter). Commands
    `set_channel_send` (answers as a strip change), `set_effect { effect, settings }` (`EffectSetDto
    { outcome: set | invalid, effect, effects, saved, saveError, message }`),
    `set_musician_fx_return` and `set_local_fx_return` (`FxReturnSetDto { musicianKey, outcome,
    effect, returns, saved, saveError, message }`). A strip change, reset, copy and preset keep each
    target's sends; a preset keeps none. DTOs: `MusicianDto.fxReturns`,
    `EngineStatusDto.localFxReturns` and `.effects`.
  - Console: `lib/effects.js` (ranges, clamps, the delay's time at a tempo, the tempo field, views
    and words); store slices `sendsPending`, `effectsPending`, `returnsPending`; actions
    `editChannelSend`, `editEffect`, `editMixReturn` (a coalescer each); bridge `setChannelSend`,
    `setEffect`, `setMusicianFxReturn`, `setLocalFxReturn`; the fake bridge mirrors them and the
    session. Effect sends (Reverb, Delay) in the processing editor with an "Effects…" link; an
    Effects screen (`#/effects`, "Effects" in the sidebar after Processing) with the reverb's four
    sliders and the delay's time, tempo field, note, feedback, high cut and ping-pong; Reverb and
    Delay return sliders on every master strip (musician tabs and Local mix).
  - RED: the 14 reverb and delay tests against stubs (no output, `apply` ignored, no tempo): 8
    failed; the reverb's click test, the delay's silence test (since tightened to check it repeated
    first) and the JSON and allocation tests passed. Engine layer, against mutations (no return
    added, no send command, `set_send` ignored, returns not stored, the replace inverted): the five
    new tests (`a_mix_returns_…`, `a_send_runs_its_effect_…`, `a_pair_shares_its_sends`,
    `effect_returns_are_kept_…`, `sends_effects_and_returns_reach_…`) failed, then passed; the six
    `effects::tests` were written with the stage. Desk, against mutations (no handing, no saving,
    sends not kept, the old filter, presets keeping sends): all six new tests failed, then passed.
    UI: `effects.test.js` (9) failed to import against the old modules, then 8 of 9 failed against
    a stub `effects.js` without the fake's and console's commands, then passed.
  - No allocation: `running_the_effects_never_allocates_…`, the effects' own tests and
    `the_engine_thread_never_allocates_with_every_strip_working_and_settings_changing` (now with
    every input sending and sends, reverb and delay changing between blocks) count 0 and 0.
  - Browser check (`desktop-ui`, `?fake`, pane hidden: renders forced by switching the hash): CH 5
    Lead vox reverb send 35%; from "Effects…" the Effects screen, reverb size 80% ("The reverb is
    set: size 80%, …"), delay tempo 120 bpm on a dotted 1/8 ("Repeats every 375 ms (Dotted 1/8 at
    120 bpm)", the time slider disabled); Lead vocal's reverb return 60% and the Local mix's delay
    return 45%, each kept on its tab; the limiter ceiling dragged slowly over 60 positions ended on
    −1.5 dBFS, the value at its release. No console errors.
  - Counts: `cargo test -p personal-monitoring` 643 passed (617 before); `cargo test -p
    mixlink-desktop` 341 passed (333); `cargo check -p mixlink-desktop --all-targets` clean; UI
    `node --test apps/desktop/ui/tests` 615 passed (604). Commit: this commit.
- [x] T8-fix (review-24afb76ed941a1de's, review-5d6be859d233a714's and review-6f8659ef97e6cff0's
  findings: six WARNINGs, five SUGGESTIONs). Route: delegated writer (with T9's docs; trigger: the
  engine's stage, two effects, the mixer and targets, then the console's app, store, a pure module,
  the master strip, a screen and the fake bridge). Commits 01df9c5 `fix(engine): keep a channel's
  sends when its strip is replaced` (+61/-5), 3759ecc `fix(engine): retarget delay and pre-delay
  without a click` (+178/-67), 714dbd5 `fix(engine): change one effect return without losing the
  other's` (+128/-16), 590d746 `refactor(engine): drop the dsp module's dead-code allowance`
  (+7/-26), 993342f `fix(desktop-ui): settle every limiter edit and test the slider's keys`
  (+103/-22) and 702b47a `fix(desktop-ui): keep each effect's own answer and the latest failure`
  (+137/-24).
  - Sends: `Processing::set_channel` (so `set_channel_processing`) keeps the channel's own sends and
    ignores `settings.sends`; only `set_send` changes them, and `replace` (a session load) replaces
    them with the strips it loads. The desk already kept each target's sends; now no caller can
    zero them by accident.
  - Delay time and pre-delay: one `MovingTap` (`dsp/reverb.rs`) for both; a length that comes
    during the 20 ms crossfade waits for it to end and then crossfades on from where the output is
    (the last length wins), so the output only moves between two taps along one fade.
  - Returns: both of a mix's returns in one `AtomicU16`; `MixState::set_fx_return` changes one with
    a single compare-and-swap (musicians and the local mix), and `set_fx_return` in the targets
    holds the book from the change through the keeping.
  - Dead code: the blanket `#[allow(dead_code)]` on `mod dsp` and its stale note are gone; four
    unused helpers removed (`Glide::jump`, `BiquadCoefficients::IDENTITY`, `Biquad::reset`,
    `StripProcessor::channels`), test-only accessors under `#[cfg(test)]`. The intra-doc links in
    `dsp/mod.rs` point at modules that exist; `cargo doc` not run.
  - Console: `sendLimiter` settles in `finally` on every path (an answer that cannot be read is
    said as not set); the limiter sliders' key tracking is the pure `createSliderKeys`
    (`lib/limiter.js`); the store keeps only the answered effect's settings and the answered
    return (`effectsKept`, `returnsKept`); the processing editor's failure line is
    `createFailureLine` (`lib/processing-view.js`): the latest failure wins and a refused send is
    forgotten on another channel; the fake bridge refuses an unknown effect in
    `set_musician_fx_return` and `set_local_fx_return`. Suggestion 11 (the effects screen's DOM
    handlers into pure helpers) not done: not cheap.
  - RED: against mutations (`set_channel` writing the sends it was given; `MovingTap::set` with
    the old jump-to-the-louder-tap rule) `a_strip_written_over_a_channel_keeps_its_sends` (sends 0,
    not 40), `times_that_come_during_a_crossfade_never_click` (a step of 0.235 against 0.0144) and
    `a_tap_moved_during_a_crossfade_never_jumps` (1,673 against 7 a frame) failed, then passed.
    `changes_to_both_returns_at_once_never_undo_each_other` (two threads, 50 rounds) and
    `the_effect_returns_are_limited_with_the_dry_mix` were written with the fix and passed against
    the old code (a race that rarely shows, and behaviour that was already right). UI: against the
    committed modules (stub `createSliderKeys` and `createFailureLine` with the old rules) all five
    new or extended tests failed (the unreadable limiter answer, the slider keys, the out-of-order
    effect and return answers, the failure line, the fake's unknown effect), then passed.
  - Counts: `cargo test -p personal-monitoring` 648 passed (643 before); `cargo test -p
    mixlink-desktop` 341 passed; `cargo check -p mixlink-desktop --all-targets` clean, and `cargo
    check -p personal-monitoring --lib` now without warnings; UI `node --test apps/desktop/ui/tests`
    619 passed (615). `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, one at a time.
- [x] T9 (close): PRD update, docs, latency check, live test with the S23. Route: delegated writer
  for the docs (with T8-fix); the live test with the user.
  - Docs: `PRD.md` version 0.5: processing in scope (RF21 per-input strip, RF22 strip presets,
    RF23 master limiter per mix, RF24 reverb and delay with sends and returns, RF25 engine load),
    "procesamiento avanzado" moved out of "fuera de alcance" (per-musician processing and
    processing from the phone stay out), latency (processing adds 0 ms: IIR, no lookahead, effects
    are wet returns), the release benchmark (32 strips, 16 musicians: 17.6% mean, 25.9% p99, 0
    late blocks) and a processing decision. `apps/desktop/README.md`: the screens list, Channel
    processing, Master limiter, Effects, Engine load and ASIO safety. Commit: this commit.
  - Remaining: live test.
    - [x] Neutral strips sound unchanged.
    - [x] An HPF, EQ and compressor on a vocal channel work, and the meters move.
    - [x] A factory preset keeps the channel's sends.
    - [x] The limiter on a musician keeps peaks at the ceiling.
    - [x] Reverb and delay sends and returns (and the tempo) sound right on the S23.
    - [x] Engine load stays low with everything on.
    - [x] ASIO open-by-name with the Volt (only with the user's explicit go-ahead).
  - Live test (2026-10-20, desktop debug build at c357ff6, Galaxy S23 with the phase 4 APK, which
    phase 5 does not change): the app log shows the S23's reverb and delay returns being moved,
    then a switch to ASIO that opened "Universal Audio Volt" by name (4 channels at 48 kHz,
    buffer 256) with the phone registered, and the local output on the Volt's outputs 1/2 under
    ASIO; the app exited cleanly and the PC stayed up. The user's verdict: "funciona excelente".
    The ASIO run was the user's own choice during the test. Individual items beyond what the log
    shows rest on the user's word.

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
- TA (ASIO safety port) done between T4 and T5: the vendored cpal and three server fixes from
  the two unmerged ASIO branches (see the task). Not reviewed yet; next review base stays 06bdde6.
- Disk: the F: drive is nearly full (2.4 GB free before T3, 1.4 GB after; each test build of the
  server takes a few hundred MB). Keep builds to debug, one at a time, and stop on "no space".
- Review review-1e6e24e2a5907a8b (06bdde6..814e164, T3-fix + T4): correction_required, corrected
  in 89eccbb (a stale strip layout; the claimed panic was not real, `channel_frames` already
  guards), validated and acknowledged.
- Release benchmark (user-approved once): `engine_load --strips` with 32 strips and 16 musicians,
  17.6% mean / 25.9% p99 / 47.2% max of the block period, 0 late blocks. The PC bugchecked
  (0x20001) about 3.5 minutes after that release run, so every release build needs a fresh yes.
- TA reviews: review-6e3fc7b9772bc0ad (the cpal patch) and review-bb3f74843543721d
  (e1f36b8..d4fe007) approved; the two old ASIO branches deleted with the user's OK. Next review
  base: d4fe007.
- T5 done (dac564b, ae49ddc and the console commit): processing in the session and named
  sessions, the commands, strip presets and the equaliser's response; free disk after its builds
  about 13 GB.

- Reviews review-62e9809767b1790e (d4fe007..ae49ddc, T5's desk half) and review-ae983bec860aca54
  (ae49ddc..9bdd0e9, T5's console): approved and acknowledged; their findings fixed in T5-fix
  (cf238a4, 7b25b2d). Next review base: 9bdd0e9.
- Machine: 13 unexpected shutdowns in 30 days with mixed bugchecks on a Windows Insider build with
  HVCI on, several under compile load; builds kept minimal (one focused desktop test build, one full
  run and one check for T5-fix and T6, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, no release builds).
- T5-fix and T6 done (cf238a4, 7b25b2d, 21b9f98 and the editor commit).
- Reviews review-90dc4abdb8816f9f (9bdd0e9..21b9f98, T5-fix and T6's logic) and
  review-93fc890ef1905355 (21b9f98..0069f51, T6's editor): approved and acknowledged; their findings
  fixed in T6-fix (3a044b2, c7b03e2). Next review base: 0069f51.
- T6-fix and T7 done (3a044b2, c7b03e2, 992e418, aa58567 and the master strip commit).
- Reviews review-186d441c6b486688 (0069f51..992e418, T6-fix and T7's engine) and
  review-8f20640b741f6075 (992e418..62f6926, T7's desk and master strip): approved and
  acknowledged; their findings fixed in T7-fix (b6bdff7, 36f2173, ff50994). Next review base:
  62f6926.
- T7-fix and T8 done (b6bdff7, 36f2173, ff50994, 5584583, c99a50c, 9016db4 and the console commit).
- Reviews review-24afb76ed941a1de, review-5d6be859d233a714 and review-6f8659ef97e6cff0 (62f6926..
  b924113, T7-fix and T8): approved and acknowledged; their findings fixed in T8-fix (01df9c5,
  3759ecc, 714dbd5, 590d746, 993342f, 702b47a). Next review base: b924113.
- T8-fix and T9's docs done (the six fix commits and the docs commit).

## Next step

Push and the pull requests, with the user's approval. Review review-48067a52e39f2671
(b924113..c357ff6, T8-fix and docs) approved and acknowledged; next review base c357ff6. T9's
live test passed (see T9); phase 5 is complete. The user continues on a Mac
afterwards: the ASIO items (the Volt under ASIO, the vendored cpal patch) and the build-safety
constraints (`CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, no release builds without a yes) are
specific to this Windows PC.
