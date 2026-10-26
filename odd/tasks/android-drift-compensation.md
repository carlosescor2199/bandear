# Android clock-drift compensation

Objective: keep the Android client's heard latency from growing over a long session when the desk's
audio clock runs faster than the phone's DAC. Branch `feat/android-drift-compensation` from main
aa12e08. Delivery: single PR. Tests: JVM unit tests (`gradle -p apps/android :app:testDebugUnitTest`).

## Specs

- S1 — Field report (L1): "al principio se escuchaba muy bien y sincronizado con poca latencia pero
  al pasar el tiempo la latencia fue aumentando". Android phone, desk on the Windows PC, capture from
  the Behringer X32's X-USB card. After a long session the heard latency must stay near the cushion
  target instead of growing without bound.
- S2 — Root cause (code evidence, 2026-10-26): the desk sends one packet per captured block, so at
  the X32's clock (`apps/server/src/pipeline.rs:243-270, 449-476`). The phone writes to `AudioTrack`
  with `WRITE_BLOCKING` (`PmonPlayer.kt:274`) into a track sized to the cushion (`:462-471`), so
  when the desk is faster the surplus waits unseen in the kernel UDP socket buffer. Nothing in the
  client drops, resamples or catches up (no drift handling anywhere in `apps/android`).
- S3 — The client must measure the whole queue a packet waits in (anything queued before the
  `AudioTrack` plus the track's own backlog), and when that queue stays above the cushion target
  for a sustained period (not on a single burst), it removes the surplus a little at a time so it
  is inaudible: short splices with a crossfade, never a hard cut, never more than a small fraction
  of the audio per second.
- S4 — The existing behaviour stays: cushion growth on underruns and shrink after clean seconds
  (`PlaybackLatency.kt`), `ReorderHold` reordering and gap handling, duplicate/late drops, the
  256-packets-behind restart, mute/volume processing, and the wire protocol.
- S5 — The `backlog_ms` the phone already reports to the desk must include the queue that S3
  measures, so the desk sees the real latency. The stats message shape does not change.
- S6 — If the phone's clock is the faster one, nothing new is needed: underruns already grow the
  cushion (S4).

## Tasks

- T1 — S1-S6: drift compensation in the Android playback path, pure policy with unit tests, honest
  backlog. Route: delegated writer (reason: long-session context backstop in the parent). Commit: 1111baf

## Log

- L1 (2026-10-26, user): "ayer hice una prueba en la iglesia con un amigo, me dijo que al principio
  se escuchaba muy bien y sincronizado con poca latencia pero al pasar el tiempo la latencia fue
  aumentando, esto puede deberse a que no hay perdida de paquetes?" Then: "Android y el servidor
  estaba en este computador", "estaba usando x usb que viene en la mixer x32 de behringer", and
  "si" to starting with Android.
- L2: read-only mapping confirmed no drift compensation on either client; iOS accumulates in its
  500 ms ring (separate follow-up), the desk UI shows only `cushionMs`, never `backlogMs`
  (follow-up).
- L3 (T1 evidence): RED first: `DriftTrimTest` + the queued-frames backlog test failed 13 of 42 against
  stubs; GREEN: full `:app:testDebugUnitTest` 570 tests, 0 failures. Design: the receive loop drains
  every waiting datagram through a non-blocking `DatagramChannel` into `PlayoutQueue` (measurable,
  bounded at 24 000 frames), `DriftTrimmer` (pure; 2 s persistence window on the minimum surplus, 8 frames
  per splice, 500 ppm budget, never below target) decides, `PcmSplice` removes the frames with a 64-frame
  crossfade. `backlog_ms` now adds the queued frames (`reportExtras(queuedFrames)`). Not verified on a device.
