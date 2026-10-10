# Playback Latency Measurement

## Objective

Make the delay a musician actually hears measurable on the device, instead of inferred from packet
jitter. The client must report an absolute playback latency in milliseconds, so buffer tuning stops
being guesswork.

## Problem and why

The Android client already reports packets received, sequences lost, inter-arrival and jitter. None of
those is latency: jitter is the *variation* of the arrival interval, and loss is a count. A steady
stream with a 40 ms playback backlog looks perfect in every existing metric while the musician hears
obvious echo.

The dominant term is the client playback backlog: the `AudioTrack` buffer (jitter cushion plus OS
output buffer) that the receive loop fills with `WRITE_NON_BLOCKING` and never drains. Nothing in the
app exposes how far behind the device's render position the writer is.

## Scope

- T1: report the client playback backlog in milliseconds, derived from `AudioTrack.getTimestamp` and
  the frames written, as a single-clock measurement. No protocol change, no clock synchronization.
- T2 (later): server capture-to-send queue latency, measured on the server clock.
- T3 (later): server capture timestamp on the wire plus a clock-offset estimate over the control
  channel, giving true one-way network latency and a real end-to-end total.

T2 and T3 are out of scope for this work unit. A server timestamp alone cannot produce a true number,
because the server and the phone are different machines with unrelated clocks; without an offset
estimate the subtraction is just clock skew.

## Constraints

- Do not touch the uncommitted desktop/Tauri work in the working tree.
- Keep technical artifacts in English.
- T1 makes no wire-format change and no control-protocol change.
- The stats display stays additive: existing lines keep their meaning and order.

## Tasks

- [x] T1: Add the playback backlog estimate and show it in the client stats.
- [ ] T2: Measure the server capture-to-send queue latency.
- [ ] T3: Add the capture timestamp and the control-channel clock offset for true one-way latency.

## Acceptance checks

- `:app:testDebugUnitTest` (Gradle 9.6, Android SDK, JDK from Android Studio).

## Verification

- Baseline before any edit: `:app:testDebugUnitTest` BUILD SUCCESSFUL (22 tasks up-to-date).
- After T1: `:app:testDebugUnitTest` BUILD SUCCESSFUL, 5 executed. `PlaybackLatencyTest`:
  tests=5, failures=0, errors=0. The whole debug unit-test suite compiles, so the widened
  `PmonPlayer.Listener.onStats` signature and the `MainActivity` override agree.
- First observed failure was real: `Unresolved reference 'AudioTimestamp'`. `AudioTimestamp` is a
  top-level `android.media` class, not nested in `AudioTrack`; a dedicated import fixed it.
- Test-first note: the pure helper and its test were authored together, because the helper is a
  mechanical extraction with no behavioural ambiguity. Strict RED was therefore not observed for that
  unit; the compiler and the suite provided the only meaningful signal. Recorded honestly rather than
  claimed.

## Iteration log

- T1: added `PlaybackLatency` plus its test, the backlog measurement in `PmonPlayer`, the
  `playbackBacklogMs` stats field and the `Playback backlog: %.1f ms` line. Green on the second run.

## Route

- T1 direct inline: one non-trivial file (`PmonPlayer.kt`), one mechanical file (`MainActivity.kt`)
  and one small pure helper with its test. Reading that prepared the edit was already in context, so
  delegating a ~60-line Android change would cost more than it saves.
- T2 and T3 will be re-routed when they start; T3 crosses both languages and will be delegated.
