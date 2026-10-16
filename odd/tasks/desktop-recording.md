# Desktop local mix and multitrack recording (redesign phase 3)

## Objective

Let the engineer hear a local mix on the computer's own output and record every input to its own
track while the show runs, like a DAW, then play a take back into the mixer in place of the live
inputs (virtual soundcheck).

## Problem and why

- The capture callback turns every sample format into 16-bit integers, keeping only the top 16
  bits of 24/32-bit input (`capture.rs:617-619`), so nothing downstream ever sees full resolution;
  96/192 kHz input is decimated by dropping frames with no anti-alias filter (`capture.rs:577-603`).
- The capture callback allocates on every block (`capture.rs:433`, `:472`, `:493`, `:595`) and can
  panic (`:563`); the send loop allocates twice per musician per packet (`mix.rs:1077`,
  `protocol.rs:336`) and does a blocking UDP send while holding the target registry's read guard
  (`network.rs:59-83`).
- During a device switch two capture streams briefly feed the same queue (`engine.rs:755`).
- There is no audio output, no ring buffer, no WAV code and no way to play recorded audio back.
- Known meter defects from phase 1 (`odd/tasks/desktop-console.md:493-501`): the 100 ms idle fall
  double-counts time for long or bursty blocks; the stall test sleeps a fixed 1 s; meter clears on
  switch and stop are not tested through `EngineHandle`.
- PRD.md:20 puts multitrack recording out of v1; the user has now asked for it, so the PRD is
  updated in this phase.

## Design (approved)

Design canvas "MixLink desktop redesign" (https://claude.ai/artifact/GcjxqS8ME4eBYVSzxjbQtB):
board 2 "Record" (transport with back, play, stop and record, position, take name, markers, disk
estimate; one track per input with arm, source, meter and waveform lane; a stereo local-mix
track; takes list with Play and Soundcheck; a virtual soundcheck card; settings for folder,
format and "also record the local mix"), the Mix board's "Local mix" tab and REC chip, and the
Network board's "Local mix out" select.

## Design decisions

- **Internal format**: interleaved f32 at 48 kHz from capture onward; conversion to 16-bit happens
  only when serializing packets for the phones (the wire protocol is unchanged). 24-bit input keeps
  its resolution.
- **Decimation**: 96/192 kHz input goes through an anti-alias low-pass before decimating.
- **Thread layout**: the capture callback converts into a preallocated lock-free SPSC ring
  (`rtrb`, approved by the user) and never allocates, locks or panics; one engine thread pulls
  fixed blocks and fans them out to sinks: the network (phones), the local output and the
  recorder, each through its own bounded ring. A full sink ring drops frames and counts them;
  audio is never blocked.
- **Network sink**: per-packet buffers are reused; the target list is snapshotted under the read
  guard and sent after releasing it.
- **Local mix**: an engine-owned mix (not a phone target) with the id `local` in the UI, edited
  like any mix and persisted in the session; played through a cpal output stream on the chosen
  output device (default: none). Under ASIO only the same driver's outputs are offered; under
  WASAPI any output device, with clock drift handled by dropping or padding frames (no resampler
  yet). ASIO is never chosen by default and never opened implicitly.
- **Recording**: our own WAV writer (no `hound`): 24-bit integer by default or 32-bit float; a
  JUNK chunk upgraded to `ds64` (RF64) on close when a file passes 4 GB; headers refreshed
  periodically so a crash leaves playable files. One folder per take, a mono file per input and a
  stereo file per linked pair, plus the local mix as a stereo file when chosen, and a `take.json`
  (tracks, start time, markers, dropped frames) written atomically. Tracks are written before any
  processing.
- **Device switch while recording** is refused (stop the take first); a take never mixes two
  devices.
- **Virtual soundcheck**: a playback source reads a take's files and feeds the engine in place of
  the capture device; its clock is the local output callback when there is one, otherwise a timer
  at the block rate; a take whose channel count differs from the current layout is refused.

## Scope

- The real-time-safe float engine with sinks, the meter fixes, local output, the WAV/RF64 writer,
  the recorder with takes and markers, the Record screen, the Local mix tab and output select,
  virtual soundcheck, and the PRD update.

## Out of scope

- DSP (phase 5), network history, redundancy and demo source (phase 4), export/import of
  sessions, editing or trimming takes, a resampler for local output.

## Constraints

- Clean-room: own names, look and copy.
- The phone wire protocol and every phase 1-2 behaviour stay as they are.
- ASIO is never chosen by default; the T7b start-request builder stays the only start path.
- Latency target stays within PRD.md:52 (≤25 ms, ≤40 ms acceptable); no dropouts over 3 hours
  (PRD.md:53).
- **Build safety (this PC bugchecked twice under heavy builds, BugCheck 0x1A)**: run builds one at
  a time with `CARGO_BUILD_JOBS=2` and gradle `--max-workers=2`; never `cargo clean` without
  asking the user; if rustc crashes or reports a corrupt cache, stop and report instead of
  retrying.
- Each commit stays under about 2,000 changed lines so it fits the reviewer's context budget.

## Acceptance criteria

- No allocation, lock or panic in the capture callback or on the per-block engine path after
  warm-up; phones hear exactly what they heard before (same mix, same packets).
- 24-bit input reaches the recorder at full resolution.
- The local mix plays on the chosen output and can be mixed from the Mix screen's Local mix tab.
- Armed inputs record to their own files while the show runs, with markers; takes are listed and
  playable; files stay readable after a crash.
- A take can be played back into the mixer as the live inputs.
- Meters fall at the same rate whatever the block length or delivery pattern.

## Checks

- Server: `cargo test -p personal-monitoring`
- Desktop: `cargo test -p mixlink-desktop`
- UI: `fnm exec --using=default node --test apps/desktop/ui/tests`
- Builds with `CARGO_BUILD_JOBS=2`.
- Live: the desktop app with the Volt (inputs and outputs) and the Galaxy S23.

## Delivery

Strategy `stacked-to-main`, branch `feat/desktop-recording` from main 0bb278b. Forecast about
15,000-20,000 authored changed lines; PRs per reviewed slice on the user's approval.

## Tasks

- [x] T1 (engine pipeline): f32 capture into an `rtrb` ring with an anti-alias decimator and no
  allocation in the callback; one engine thread with sinks; the network sink with reused buffers
  and no sends under the registry guard; a single producer across device switches; the meter
  fixes R3-001..R3-003 from phase 1. Route: delegated writer (mapping, writer and 2+ non-trivial
  files triggers).
  - Shape: `decimate.rs` (Kaiser-windowed sinc, cutoff 24 kHz, flat to 20 kHz, >= 80 dB from
    28 kHz; 63 taps at 96 kHz, 123 at 192 kHz, 0.32 ms delay); `pipeline.rs` (`CaptureSample`
    conversion keeping 24-bit, `CaptureWriter` per stream with its own `rtrb` ring of at least
    0.5 s or 4 blocks, `Pipeline` engine thread pulling blocks of one callback length (default
    256, at most 8192 frames), `Sink` trait with `process`, `silence` and `source_changed` for T2
    and T4 to add sinks); `network.rs` `NetworkSink` (destinations copied under the read guard,
    mixed and sent after it, reused stereo and packet buffers); `mix.rs` `MixPlan` (gains and pan
    weights once per block); `protocol.rs` `encode_audio_packet`.
  - RED: frame-dropping decimation folds 28 kHz back at -0.0 dB
    (`content_above_24_khz_is_attenuated_before_decimation`); the 16-bit adapter sink made
    (7000, 7000) allocations/frees per 1000 blocks (`the_capture_to_phone_path_never_allocates`)
    and held the table while sending (`a_target_can_be_removed_while_a_block_is_being_sent`); the
    fixed 100 ms fall read 0 instead of 30 for 150 ms blocks, 0 instead of 34 for bursts and 40
    instead of 34 for a stall (`long_blocks_are_not_counted_again_by_the_idle_fall`,
    `bursty_delivery_falls_by_the_audio_and_nothing_more`,
    `a_stalled_capture_falls_by_the_time_that_passes`); the EngineHandle clear tests fail with
    the clears removed (mutation: [50, 50] left on stop and switch).
  - GREEN: server unit tests 300 -> 315 -> 322 -> 327, all passing. Measured: stop band -83.8 dB
    or lower from 28 kHz, pass band within 0.002 dB to 19 kHz; packets byte-identical to the
    16-bit mixer over 400 drawn blocks on UDP and 3000 drawn mixes; zero allocations and frees
    over 1000 blocks from a 24-bit 96 kHz callback to three phones (thread-local counting
    allocator in the unit-test binary, so integration tests could not reach `pub(crate)`).
  - Latency: callback-to-sink hand-off 89 us median, 257 us p99 (debug build); otherwise at most
    one block when the driver's callback length differs from the block (WASAPI), plus 0.32 ms of
    filter delay at 96/192 kHz only. Documented in the `pipeline.rs` module comment.
  - Earlier commits: cdb5d84 (capture, ring, decimator, engine thread), ebadce2 (float network
    sink). Commit: f4ffdbe (meter fixes R3-001..R3-003).
  - [x] T1-fix (review findings): device switches driven through `EngineHandle` with the
    `CaptureWriter` seam; a capture whose format and frames disagree is now refused by the plan;
    the two timing-bound pipeline tests wait on conditions; tests that need several musician IPs
    skip with a message when 127.0.0.2/3 cannot be bound (macOS), and the allocation test runs
    with the phones that bind. RED (by mutation: the plan fed the format unchecked, and the commit
    made before the engine thread took the source):
    `a_refused_switch_leaves_the_running_capture_untouched` accepted the mismatched capture, and
    `a_switch_fails_without_committing_when_the_engine_thread_has_ended` committed
    ("Test input (1)", 1 channel) on a dead engine thread. GREEN: server unit tests 327 -> 329,
    passing on 3 runs. The timing and loopback changes have no RED: they remove dependence on the
    machine (timer granularity, loopback aliases) and pass the same here. Commit: 2b5ac1d.
- [x] T2a (local mix and output, engine): the engine-owned local mix and the cpal output sink with
  drift handling and the ASIO rules. Route: delegated writer (writer and 2+ non-trivial files
  triggers).
  - Local mix: one `Arc<MixState>` outside the target table, changed with
    `EngineHandle::set_local_mix(&MixUpdate)` on the desk's path, read with `local_mix()`,
    announced as `EngineEvent::LocalMixChanged` (no address), moved with every musician's mix on
    group edits, device switches and channel details, reported in `EngineStatus::local_mix`.
  - Local output (`output.rs`): a `LocalOutputSink` mixes the local mix with `MixPlan` each block,
    meters it (`EngineMeters::local_levels`) and pushes it into an `rtrb` ring read by the cpal
    output callback (`OutputPlayer`), which writes the pair into the chosen channels, silence
    elsewhere, in the device's sample format. Target fill one device callback + 2 blocks; under
    it the player pads with silence (counted as underrun frames) and primes again; 2 blocks above
    it frames are dropped back to the target (counted). Worst-case delay one callback + 4 blocks
    (31.3 ms at a 480-frame WASAPI period with 256-frame blocks, 26.7 ms at ASIO 256). Changing or
    removing the output is live (the sink takes the new feed between blocks); a device error marks
    only the output failed, with its reason. Off by default; under ASIO only the capture driver's
    own outputs are offered (from the device the capture holds, nothing listed) and a switch to
    another ASIO driver stops the output; under WASAPI every output device. 48 kHz only.
  - API for T2b: `output_devices()`, `set_local_output(Option<LocalOutputRequest>)`,
    `local_output()`, `EngineStatus::local_output` (`LocalOutputStatus { output, state:
    Off | Running | Failed { reason }, underrun_frames, dropped_frames }`).
  - RED: the local mix tests did not compile (no `local_mix`, `set_local_mix`,
    `LocalMixChanged`); 9 of 12 output tests failed against stubs (player that only played
    silence, sink that did nothing, rules that offered nothing and refused nothing); the engine
    output tests did not compile (26 errors: no `install_output`, `set_local_output`,
    `local_output`, `output_devices`, `local_levels`).
  - GREEN: server unit tests 329 -> 333 -> 350; the output callback makes zero allocations and
    frees over 2000 callbacks; desktop `cargo check --all-targets` clean (status and meters
    literals gained the new fields).
  - Commits: 1a0223f (local mix), f83180f (local output).
  - [x] T2a-fix (review findings): the local output sink's per-block path is allocation- and
    free-free. Its stereo buffer is sized for the longest block when the sink is built, feeds
    travel to it in a preallocated `rtrb` command ring (4 slots), and the feed a change lets go of
    goes back through a return ring to `OutputControl`, which frees it on the engine's side (on the
    next change, status read or stop), never on the engine thread. The sink also takes changes in
    its idle `silence` call, so a change lands within one idle wait without audio. The
    multi-musician network tests fail on Windows and Linux when 127.0.0.2/3 cannot be bound, and
    elsewhere report SKIPPED unless `MIXLINK_REQUIRE_LOOPBACK_ALIASES` is set; the allocation test
    keeps its centred mix on the second phone. RED: `a_live_output_change_frees_nothing_on_the_engine_thread`
    counted (0, 3) allocations/frees on the engine thread with the channel design. GREEN: server
    unit tests 350 -> 353 (the live-change test, a change taken without audio, bounded changes);
    the full engine path with the network and local sinks makes (0, 0) over 1000 blocks with no
    output and with an output playing; desktop `cargo check --all-targets` clean.
    Commit: this commit.
- [x] T2b (local mix and output, desktop): session persistence of the local mix and the output
  choice, Tauri commands over the T2a API, the Local mix tab and the "Local mix out" select.
  Route: delegated writer (writer and 2+ non-trivial files triggers).
  - Session: a `local` section `{ mix, output }` (version 1): `mix` is a `SessionMix` (inputs in
    channel order, group levels by group id, master volume and mute, never a cap), absent until
    the local mix was changed; `output` is `{ device, firstChannel }` (0-based pair) or null. It
    loads on its own (a damaged one is set aside); named sessions carry it.
  - Commands (`commands/local.rs`): `set_local_mix(mix) -> MixDto` (as `set_mix`, `maxLevelPercent`
    refused) applies under the engine lock, then saves by group id (a save failure is logged);
    `output_devices()` needs a running engine; `set_local_output(output | null)` answers
    `{ output, localOutput, applied, saved, saveError }` and with no engine keeps it for the next
    start. Status gains `localMix` and `localOutput { device, firstChannel, state, reason,
    underrunFrames, droppedFrames, notice }`; meters gain `local`; a local change goes out as
    `mix-changed { run, id: "local", origin: "desktop", mix }` with no address.
  - Start and load: the kept mix goes through the desk's path by group id, then the output opens
    only if `output_devices()` offers that exact device and pair (under ASIO only the capture
    driver's own); otherwise it stays off with `localOutput.notice` and never another device. A
    snapshot saved while running keeps the engine's local mix; one without a local section leaves
    the desk's own.
  - UI: a "Local mix" tab last ("This computer · <output or no output>"), the same strips with no
    lock, cap, limit or tools, through pending edits and the coalescer (`set_local_mix`); "Local
    mix out" in the audio setup ("None (lowest load)" and each output pair), state and counters in
    words, a stop and an unopened kept output as alerts; outputs listed again for every run. The
    fake mirrors the rules (outputs, ASIO's own outputs only, refusals, start/load notice, regroup
    and new pairs moving the local mix, meters, sessions).
  - Blocked once on a corrupt tauri-build artifact (`out/capabilities.json` all NUL, from the
    October bugcheck); the parent deleted it and touched the capabilities, with no `cargo clean`.
  - RED: desktop by 8 mutations (session dropping `local` on read, no save after a local edit,
    opening an output not offered, the notice not cleared, snapshots without the engine's local
    mix, loads ignoring the local section, no local `mix-changed`, meters without `local`): 13 of
    196 failed. UI: the module missing, then 16 of 23 new tests failing. The master-controls null
    test was written after its change (no RED).
  - GREEN: desktop tests 170 -> 176 (84ce3be) -> 196, `cargo check --all-targets` and `cargo build`
    clean; UI tests 350 -> 375. Preview `?fake#/mix` and `#/network`: no console errors; tab, edit,
    output choice and the tab's word checked.
  - Follow-ups: save the local mix with a debounce (done in T2b-fix); live with the Volt: WASAPI
    OUTPUT 1/2 counters over a long run, ASIO listing the Volt driver's own outputs and reopening
    the kept output at start, and a WASAPI-kept output showing the notice and staying off under
    ASIO.
  - Commits: 84ce3be (session), b70131d (commands, DTOs, events), 25530e6 (UI).
  - [x] T2b-fix (review findings of review-dab3acc47185a219 and review-8a82f3af04262a69): the
    local-mix save leaves the engine lock (part 1 R3-002): an edit applies under the lock and only
    marks the mix; a saver thread writes the latest at most every 400 ms, and stop, quit and drop
    write what waits; a failed write is kept to retry and logged once until it changes; a start and
    a load pause the saver to read or replace the session with the mix still waiting (a load
    keeps it with the desk's own local mix, drops it for a loaded one). A load's output notice is
    written inside the engine lock (part 1 R3-001), and the status is built by `status_dto`, which
    the test drives with the notice (a running `EngineHandle` needs hardware, so the test drives
    `load_session_with`, the whole load). UI: an output listing answered for an ended run is not
    taken (part 2 R3-001); output choices go one at a time, an overtaken one never goes out and
    only the last choice's words are said (R3-002); the listing per run, the select's choice, the
    audio setup's lines and the master meter's local peak are DOM-free functions with tests
    (R3-003); the fake reads `firstChannel` as the desktop does, a required whole number 0-65535,
    any pair within the device (R3-004: the desktop and the engine take odd pairs too, so the fake
    does not require even ones).
    RED: desktop by 4 mutations (no in-lock notice write, a write on every edit under the lock,
    no throttle, a failure logged every time): 4 of 203 failed, one per mutation. UI: 7 of 31 new
    tests failing before the change. GREEN: desktop 196 -> 203 (3 runs), `cargo build` clean; UI
    375 -> 382; preview with no console errors (two quick choices end on the last one).
    Commits: 1decd1d (backend), 9ce6711 (UI).
  - [x] T2b-fix2 (review-a198f89aefeca3bf findings): the output-choice lane never stays rejected
    (R3-001): every choice settles with an answer, the lane catches, a refusal, an answer that
    cannot be read and a status that cannot be read are said in the message, and a later choice
    still goes out. Seam tests (R3-002): a start right after a fader edit plays the edit through
    `kept_local` (`with_pending`) and the edit is still written later; the exit hook is
    `on_run_event`, which writes the waiting local mix on `RunEvent::Exit` only.
    RED: UI, the 2 new lane tests failing (the status failure unsaid; an unreadable answer said as
    "not changed"); desktop, by 2 mutations (the start ignoring the waiting edit, exit not
    flushing): 2 of 205 failed. GREEN: desktop 203 -> 205 (3 runs), `cargo build` clean; UI
    382 -> 384. Commit: this commit.
- [x] T3 (WAV/RF64 writer): pure, heavily tested writer for 24-bit and 32-bit float, mono and
  stereo, RF64 upgrade, crash-safe headers; and a reader for playback. Route: delegated writer
  (preparation trigger: the format research and the module are one write).
  - API (`wav.rs`, re-exported from the crate): `WavSpec { channels, sample_rate, format }` with
    `bytes_per_frame()` and `bytes_per_second()`; `SampleFormat::{Int16, Int24, Int32, Float32}`
    (the writer records `Int24` and `Float32` and refuses the others, the reader reads all four);
    `WavWriter` with `create(path, spec)` (never overwrites a file), `new(target, spec)`,
    `write_frames(&[f32])` (whole interleaved frames), `refresh()`, `sync()`, `finish() -> target`
    and `frames()`, over any `WavTarget` (`File`, whose `sync` is `sync_data`, or
    `Cursor<Vec<u8>>`); `WavReader` with `open(path)`, `new(source)`, `spec()`, `frames()`,
    `position()`, `read_frames(&mut [f32]) -> frames` and `seek(frame)`;
    `WavError::{Io, Malformed, Unsupported, InvalidInput, Stopped}`. No panics, no new crates.
  - Header: always 104 bytes: `RIFF`/`WAVE`, a 28-byte `JUNK` at offset 12 (where `ds64` goes),
    a 40-byte WAVE_FORMAT_EXTENSIBLE `fmt ` (24-bit PCM with 24 valid bits, or IEEE float; channel
    mask front centre for mono, front left and right for stereo, none otherwise), `data` at 96; no
    `fact` chunk. Int24 scales by 2^23 like capture, so 24-bit capture samples are written back bit
    for bit; +1.0 and above clamp to 8,388,607, -1.0 is -8,388,608, NaN is 0; float is written as
    is. A pad byte follows odd-sized audio (written by `finish`).
  - Crash safety: `refresh` hands the audio to the file before rewriting the RIFF and data sizes
    in place and puts the write position back; `sync` also calls `sync_data`; a failed write stops
    the writer (`Stopped` from then on) and the header is never touched again. The reader takes
    the whole frames present when the data size is 0, missing (RF64 without `ds64`) or past the
    end of the file, and honours a smaller one (later bytes may be other chunks), so a crash loses
    at most what came after the last refresh.
  - RF64: when the RIFF size passes 0xFFFFFFFE (injectable with `with_size_limit` for tests),
    `refresh` and `finish` write `RF64` with 0xFFFFFFFF, turn `JUNK` into `ds64` (64-bit RIFF size
    with the pad byte, data size, frames, an empty table) and set the data size to 0xFFFFFFFF;
    nothing moves.
  - RED: 25 of the 26 new tests failed against stubs that returned an error from every call; the
    unsupported-encoding test passed against the stubs' blanket `Unsupported`, so it was checked
    by mutation (any PCM bit depth read as 16-bit): it failed on 8-bit PCM with `Malformed`.
  - GREEN: server unit tests 353 -> 379, all passing; clippy reports nothing in `wav.rs` (its 7
    warnings are older, in capture, engine, mix and control). Tests run in memory, plus one 4 KB
    temporary file; RF64 is reached with a 120-160 byte limit, never by writing 4 GiB.
  - Commit: b9e1073.
  - [x] T3-fix (review-c02182433a1b692a findings): a data size of 0 is recovered as an unfinished
    file only when the file is laid out as `WavWriter` lays it out (`JUNK`/`ds64` of 28 bytes at
    12, a 40-byte `fmt ` at 48, audio at 104) or when the bytes after the data chunk's header
    cannot begin a chunk (four printable ASCII characters and a size that fits); another writer's
    empty data chunk followed by `LIST` stays empty (R3-002). The desktop exit-hook test no longer
    races the saver's interval: a test-only guard (`manual_savers()`, thread-local) makes savers
    started on the test's thread write only when flushed, so the test proves that only
    `RunEvent::Exit` writes the waiting mix, deterministically (R3-001).
    RED: server, `an_empty_data_chunk_of_another_writer_stays_empty` read the LIST chunk as 4
    frames of noise; the layout guard checked by mutation (no `ours` check:
    `an_unfinished_file_laid_out_as_ours_is_recovered_even_if_its_audio_reads_like_a_chunk`
    failed). Desktop, a 450 ms pause after `mark(20)` (a descheduled test thread) made the old
    test fail with `Some(20)` on `Ready`; the new test passed with the same pauses.
    GREEN: server unit tests 379 -> 381; desktop 205 -> 206 on 3 runs (the saver guard test
    added, the exit test replaced); clippy reports nothing in `wav.rs`. Commit: this commit.
- [ ] T4 (recorder and takes): the recorder sink and disk thread, arming, take folders and
  `take.json`, markers, dropped-frame accounting, refusing device switches while recording, disk
  estimate, Tauri commands. Route: delegated writer.
- [ ] T5 (Record screen): the Record section per the design, the REC chip, takes table and
  settings. Route: delegated writer.
- [ ] T6 (virtual soundcheck): the playback source in place of capture and its UI. Route:
  delegated writer.
- [ ] T7 (close): PRD update, docs, live test. Route: inline or writer.

## Progress

- Branch `feat/desktop-recording` from main 0bb278b (phases 1 and 2 merged as PRs #17-#38).
- The user approved adding the `rtrb` crate (a download from crates.io); `rtrb` 0.3.5 is in.
- T1 done in three work units: cdb5d84, ebadce2 and f4ffdbe. T2 plugs the local output in as a
  `Sink` (fed by `process`, sized in `source_changed`) and T4 the recorder the same way; neither
  touches the capture path.
- T1 was reviewed in two parts because of the lens context budget, both approved and
  acknowledged: review-bd3440d2b6f6bfc9 (0bb278b..cdb5d84) and review-4f74b5710abf87fc
  (cdb5d84..f4ffdbe). Part 1's R3-001 (the network sink still allocating) was resolved by
  ebadce2; the rest (part 1 R3-002 and R3-003, part 2 R3-001) by T1-fix. Next review base: f4ffdbe.
- T2 split into T2a (engine, done in 1a0223f and f83180f) and T2b (desktop).
- Review lineage review-6ec7e1a9fbf7a242 over f4ffdbe..f83180f (T1-fix and T2a) approved and
  acknowledged; its findings (R3-001 local sink on the engine thread, R3-002 silent loopback
  skips) are fixed by T2a-fix. Next review base: f83180f.
  Live checks still owed for T7: an ASIO output on the Volt next to its capture (cpal opens both
  directions on one driver), and drift over a long WASAPI run.
- T2b done in three work units: 84ce3be, b70131d and 25530e6.
- T2a-fix and T2b were reviewed in two parts (over the reviewer's budget), both approved and
  acknowledged: review-dab3acc47185a219 (f83180f..b70131d) and review-8a82f3af04262a69
  (b70131d..25530e6). Their findings (part 1 R3-001 and R3-002, part 2 R3-001 to R3-004) are fixed
  by T2b-fix. Next review base: 25530e6.
- Review lineage review-a198f89aefeca3bf over 25530e6..9ce6711 (T2b-fix) approved and
  acknowledged; both findings (R3-001 the output lane, R3-002 the start and exit seams) are fixed
  by T2b-fix2. Next review base: 9ce6711.
- T2b-fix2 (d8f2ab1) is pending review in the slice from 9ce6711.
- Review lineage review-c02182433a1b692a over 9ce6711..b9e1073 (T2b-fix2 and T3) approved and
  acknowledged; both findings (R3-001 the exit test racing the saver's interval, R3-002 an empty
  data chunk read as audio) are fixed by T3-fix. Next review base: b9e1073.

## Next step

T4.
