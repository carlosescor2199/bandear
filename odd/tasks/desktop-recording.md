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
    sink). Commit: this commit (meter fixes R3-001..R3-003).
- [ ] T2 (local mix and output): the engine-owned local mix, the cpal output sink with drift
  handling and the ASIO rules, session persistence, Tauri commands, the Local mix tab and the
  "Local mix out" select. Route: delegated writer.
- [ ] T3 (WAV/RF64 writer): pure, heavily tested writer for 24-bit and 32-bit float, mono and
  stereo, RF64 upgrade, crash-safe headers; and a reader for playback. Route: delegated writer.
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
- T1 done in three work units: cdb5d84, ebadce2 and the meter-fix commit. T2 plugs the local
  output in as a `Sink` (fed by `process`, sized in `source_changed`) and T4 the recorder the same
  way; neither touches the capture path.

## Next step

T2.
