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

## Constraints

- Clean-room: never copy StageWave code, UI, name or assets.
- Old phones keep working; no PMON or control protocol change for processing.
- ASIO is never chosen by default and never enumerated implicitly.
- The engine thread and audio callbacks never allocate, lock or panic.
- **Build safety**: `CARGO_BUILD_JOBS=2`, gradle `--max-workers=2`, one build at a time, never
  `cargo clean`, stop and report on a compiler crash or a corrupt generated file.

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

- [ ] T1 (load): engine load measurement in the status, release benchmark example. Route:
  delegated writer.
- [ ] T2 (smoothing): gain ramp in MixPlan with the golden contract adjusted for moving gains.
  Route: delegated writer.
- [ ] T3 (dsp module): biquad EQ and HPF, gate, compressor, limiter; allocation-free, exact bypass,
  tests. Route: delegated writer.
- [ ] T4 (per-input stage): the stage in `Pipeline`, engine API, command ring, raw block to the
  recorder, post-processing meters and gain reduction. Route: delegated writer.
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

## Next step

T1 (load measurement).
