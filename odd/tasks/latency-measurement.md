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
- T5 (done): harden the client playback path so the buffer cannot saturate silently.
- T6 (done): make the playback cushion adaptive, driven by the track's own underrun count.
- T7 (done): capture through ASIO on Windows, the only backend where the capture period can shrink,
  with a WASAPI fallback.
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
- [x] T5: Bound the client playback path (no permanent saturation, no silent discard).
- [x] T6: Make the playback cushion adaptive from the track's underrun count.
- [x] T7: Capture through ASIO on Windows (the only backend where the period can shrink).
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
- T5/T6: `:app:testDebugUnitTest` exit 0, BUILD SUCCESSFUL; `PlaybackLatencyTest` tests=12,
  failures=0, errors=0 (the backlog math, the buffer target and the adaptive cushion controller).

## On-device measurement

Measured on a Galaxy S23 (SM-S916B, Android, 48 kHz stereo) against a Windows server, with the
server's default input device and `--buffer 240`:

- Server: cpal accepted the request and printed `buffer=Fixed(240)`, but the effective packet
  interval did not change. The client reported `Inter-arrival: 9.9 ms` and 97.7-99.5 packets per
  second, which is 480 frames per callback, not 240. On Windows WASAPI **shared** mode the engine
  period governs the callback size, so `BufferSize::Fixed` cannot go below it; that needs exclusive
  mode or ASIO. Confirmed on a second device (the Volt 4, capturing at 192 kHz: `buffer=Fixed(240)`
  still produced 1920-frame callbacks) and in cpal's own source, which states it plainly: "The
  callback period is always GetDevicePeriod() regardless of what is requested here; the value only
  affects ring-buffer latency." Recorded as a finding, not a fix.
- Client: the track lands on the **fast path** (`AudioStreamOut` with `AUDIO_OUTPUT_FLAG_FAST`,
  `FastMixer`, HAL buffer 192 frames = 4 ms), so `PERFORMANCE_MODE_LOW_LATENCY` did its job.
- Playback backlog was linear in the cushion, and the framework's own track latency agreed:

  | Cushion | Buffer | Playback backlog | Track latency | Sequences lost | Samples discarded |
  | --- | --- | --- | --- | --- | --- |
  | 3 packets | 1440 frames | 53.9 ms | 51.00 | 5 | 0 |
  | 2 packets | 960 frames | 43.9 ms | (not dumped) | 0 | 0 |
  | 1 packet | 480 frames | 33.9 ms | 31.00 | 0 | 0 |

  The fixed device pipeline is ~24 ms (backlog minus buffer) under every setting; the buffer adds its
  own duration on top. One packet left the fast track reporting empty mix cycles (`Empty 47`), which
  is the audible-gap risk, so the committed default is two packets.
- Adaptive cushion (T6), verified live on the same S23: the controller walked the cushion from its
  2-packet start down to **1 packet** and held it for over a minute (8618 packets, 0 lost, 0
  discarded). The track buffer stayed at 480 frames, the framework's own track latency read 31.00,
  and the playback backlog settled at 33.9-34.1 ms instead of the fixed 2-packet 43.9 ms. Android's
  underrun count, which the controller watches, stayed at zero.
- ASIO (T7), measured on the same machine with the Volt 4:
  - `--buffer 240` is rejected by the ASIO driver (it wants a power of two); the fallback lands on the
    driver default of 128 frames, which is **2.67 ms packets at ~347 per second**. That cut the
    playback backlog to 29.0 ms but made the phone drop **1031 of 14927 packets (~7%)**: the client's
    single receive loop cannot keep up at that rate.
  - `--buffer 256` is accepted: **5.33 ms packets at ~187 per second**, playback backlog 29.0 ms,
    **0 sequences lost**, 0 samples discarded. The backlog matches the 128 case because the adaptive
    cushion grew to two small packets there, so 256 is strictly better: same latency, no loss.
  - The default capture buffer is therefore 256 frames: the ASIO driver default (128) loses packets,
    and WASAPI shared mode ignores the request anyway.
- Remaining budget: ~5 ms capture + ~3 ms network + ~29 ms playback is ~37 ms, down from ~59 ms when
  the work started. The two levers left are the client's own receive loop (it caps the packet rate, so
  it caps how small the packet can be) and the ~24 ms device pipeline, which is not ours to change.

## Honest limits

- Only one device and one server machine were measured. The cushion default is a safe value, not a
  proven optimum for every phone.
- The DAC underrun that a too-small buffer would cause is not visible to the app: the only signal was
  the framework's fast-track `Empty` count, and the audible verdict needs a human listening.

## Iteration log

- T1: added `PlaybackLatency` plus its test, the backlog measurement in `PmonPlayer`, the
  `playbackBacklogMs` stats field and the `Playback backlog: %.1f ms` line. Green on the second run
  after fixing a wrong `AudioTimestamp` import.
- T2: added `EngineConfig.buffer_frames`, the `--buffer` flag with a validated range, the pure
  `with_buffer_size` mapping and the `describe_buffer` formatter, and a fallback to the device default
  when an explicit size is rejected. `CaptureFormat.buffer_size` now reports what actually took effect.
- T5: switched the playback write to `WRITE_BLOCKING` so the writer is paced by the render position
  and samples are no longer silently discarded, and added `applyPlaybackBufferTarget`, which asks
  `setBufferSizeInFrames` for two packet intervals once the first packet reveals the packet size.
  The platform clamps the request up to its hardware minimum; a refused call leaves the buffer as it
  was. The cushion was then tuned on device (see the measurement section): three packets was the first
  guess, and the measurement showed it was pure added latency.
- T6: replaced the fixed cushion with an adaptive controller. `PlaybackLatency.nextCushion` walks the
  cushion down after three clean one-second intervals and jumps it back up on any new underrun, and
  `PmonPlayer` drives it from `AudioTrack.underrunCount`, so the client settles on the smallest buffer
  this device and network actually sustain instead of a fixed guess.
- T7: the engine now captures through the ASIO host when it is available, falling back to the default
  host. ASIO is the only Windows backend where `BufferSize::Fixed` shrinks the callback period, so the
  `--buffer` flag finally does what it says. ASIO reports 32-bit integers, so the capture path gained
  an `I32` conversion. The build needs LLVM/Clang (cpal's build script runs bindgen over the ASIO SDK
  and downloads the SDK itself), so `asio` is a default feature that can be dropped with
  `--no-default-features`. ASIO can only hold one driver at a time, so the host is created once and
  shared, and a requested buffer is clamped into the range the device reports. A Realtek ASIO driver
  reports `1024..=1024` and then rejects even that, so it falls back to its default; the Volt honours
  `256`.

## Route

- T1 direct inline: one non-trivial file (`PmonPlayer.kt`), one mechanical file (`MainActivity.kt`)
  and one small pure helper with its test.
- T2 direct inline: the change is understood end to end and already in context (`capture.rs` core,
  `cli.rs` parsing, `engine.rs` pass-through, two examples and one desktop line). Handing that context
  to a fresh worker would cost more than it saves, and the verify loop (`cargo check`/`cargo test`) is
  cheap and fast here.
- T4 will be delegated: it crosses Rust and Kotlin and adds protocol surface.
