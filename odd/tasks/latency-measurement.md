# Playback Latency Measurement

## Objective

Make the delay a musician actually hears measurable on the device, and then reduce it. The client
must report an absolute playback latency in milliseconds, the server must expose the capture buffer
that sets the packet interval, and the client must stop letting its playback buffer saturate.

## Problem and why

The Android client already reported packets received, sequences lost, inter-arrival and jitter. None of
those is latency: jitter is the *variation* of the arrival interval, and loss is a count. A steady
stream with a 40 ms playback backlog looks perfect in every existing metric while the musician hears
obvious echo.

Three concrete causes were confirmed in the code, not assumed:

1. The client opened `AudioTrack` at `getMinBufferSize` and wrote with `WRITE_NON_BLOCKING` with no
   drain policy, so the buffer sits saturated and every sample that does not fit is discarded. Its
   occupancy is the client latency, and nothing exposed it.
2. The server never set a buffer size on the capture stream (`supported_config.config()` yields
   `BufferSize::Default`), so the device default period applied.
3. The server emits exactly one UDP packet per input callback, so the capture buffer is also the
   packet interval: an unconfigurable 10 ms device period is 10 ms of latency *and* a 10 ms packet.

## Scope

- T1 (done): report the client playback backlog in milliseconds, from `AudioTrack.getTimestamp` and
  the frames written. Single clock, no protocol change, no clock synchronization.
- T2 (done): make the server capture buffer configurable and small, with a safe fallback to the
  device default when a driver rejects an explicit size.
- T5 (next): harden the client playback path so the buffer cannot saturate silently.
- T3 (pending): server capture-to-send queue latency, measured on the server clock.
- T4 (pending): server capture timestamp on the wire plus a clock-offset estimate over the control
  channel, giving true one-way network latency and a real end-to-end total.

T3 and T4 remain out of scope. A server timestamp alone cannot produce a true number, because the
server and the phone are different machines with unrelated clocks; without an offset estimate the
subtraction is just clock skew.

## Constraints

- Do not touch unrelated work in the tree; commit only this feature's files.
- Keep technical artifacts in English.
- No wire-format change and no control-protocol change in this work.
- Existing defaults are preserved: with no explicit buffer, behaviour is byte-for-byte what it was.

## Tasks

- [x] T1: Add the playback backlog estimate and show it in the client stats.
- [x] T2: Add `--buffer` and a configurable capture buffer with a device-default fallback.
- [ ] T5: Bound the client playback path (no permanent saturation, no silent discard).
- [ ] T3: Measure the server capture-to-send queue latency.
- [ ] T4: Add the capture timestamp and the control-channel clock offset for true one-way latency.

## Acceptance checks

- `cargo fmt --all`
- `cargo check --workspace --all-targets`
- `cargo test -p personal-monitoring`
- `:app:testDebugUnitTest` (Gradle 9.6, Android SDK, JDK from Android Studio).

## Verification

- Baseline before any edit: `:app:testDebugUnitTest` BUILD SUCCESSFUL; server suite green on `main`.
- T1: `:app:testDebugUnitTest` BUILD SUCCESSFUL; `PlaybackLatencyTest` tests=5, failures=0, errors=0.
- T2: `cargo check --workspace --all-targets` exit 0 (examples and the desktop crate included);
  `cargo test -p personal-monitoring` exit 0, 116 passed, 0 failed. `cargo fmt --all` clean.
- First run of the server suite hit a transient MSVC linker crash (`LNK1000: Internal error during
  CImplib::EmitImportThunk`, access violation inside `link.exe`). The immediate retry linked and
  passed; the failure is in the toolchain, produces no code diagnostic, and is recorded here rather
  than hidden.
- T5: pending.
- Honest limits: nothing here was measured on a real phone. The backlog metric is the instrument; the
  buffer target is a reasoned default, not a measured optimum, and must be confirmed on device.

## Iteration log

- T1: added `PlaybackLatency` plus its test, the backlog measurement in `PmonPlayer`, the
  `playbackBacklogMs` stats field and the `Playback backlog: %.1f ms` line. Green on the second run
  after fixing a wrong `AudioTimestamp` import.
- T2: added `EngineConfig.buffer_frames`, the `--buffer` flag with a validated range, the pure
  `with_buffer_size` mapping and the `describe_buffer` formatter, and a fallback to the device default
  when an explicit size is rejected. `CaptureFormat.buffer_size` now reports what actually took effect.
- T5: pending, recorded when implemented.

## Route

- T1 direct inline: one non-trivial file (`PmonPlayer.kt`), one mechanical file (`MainActivity.kt`)
  and one small pure helper with its test.
- T2 direct inline: the change is understood end to end and already in context (`capture.rs` core,
  `cli.rs` parsing, `engine.rs` pass-through, two examples and one desktop line). Handing that context
  to a fresh worker would cost more than it saves, and the verify loop (`cargo check`/`cargo test`) is
  cheap and fast here.
- T4 will be delegated: it crosses Rust and Kotlin and adds protocol surface.
