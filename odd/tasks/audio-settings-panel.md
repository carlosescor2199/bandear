# Audio Settings Panel

## Objective

Give the desktop window a DAW-style audio panel: pick the driver, the device and the buffer from the
window instead of the command line, so the latency the engine was tuned for is reachable where the
engineer actually works.

## Problem and why

The engine gained an ASIO backend and a configurable buffer, but the window could reach neither. It
always used the automatic host and the engine's default buffer, and it listed whatever the automatic
host offered - which on this machine is the Realtek ASIO driver first, a driver that reports a
1024-frame period and is worse than WASAPI. The engineer could not choose the Volt, which is the
interface that actually honours a 256-frame period.

## Scope

- Driver: `auto`, `asio` or `wasapi`, as an `AudioHost` on `EngineConfig` and a `--host` flag.
- Device: already switchable at runtime; unchanged.
- Buffer: exposed as a selector; the engine already had `buffer_frames`.
- Sample rate: shown, not selectable. It is the wire format the phones receive, not a free choice.

## Constraints

- Changing the driver or the buffer restarts the engine, the way a DAW does when the driver changes.
  The phones reconnect on their own through the discovery beacon.
- Changing the device still does not restart anything.
- ASIO can only hold one driver at a time, so the host stays created once and shared.
- Keep technical artifacts in English.

## Tasks

- [x] T1: Add `AudioHost` and a per-backend host/device listing on the server.
- [x] T2: Thread the backend through `EngineConfig`, the CLI (`--host`) and the engine handle.
- [x] T3: Expose the driver and buffer through the desktop DTO, commands and window panel.
- [ ] T4: Fix the rough edges found in the first live use (see below).

## Acceptance checks

- `cargo fmt --all`
- `cargo test -p personal-monitoring`
- `cargo test -p mixlink-desktop`
- `cargo build -p mixlink-desktop` (needs `LIBCLANG_PATH` for the ASIO bindings).

## Verification

- Server: 120 tests passed. Desktop: 17 tests passed.
- `cargo build -p mixlink-desktop` succeeded and the panel was verified on screen: Driver, Input
  device and Buffer selectors render, with the sample rate shown as read-only.
- The ASIO device path was verified from the CLI, which is the same start path the window now uses
  for a device change under ASIO: `--host asio --device Volt --buffer 256` selected
  `Universal Audio Volt`, printed `buffer=Fixed(256)`, and the wire measured 256-frame (5.33 ms)
  packets.

## Known rough edges (T4)

- A driver that rejects an explicit buffer (the Realtek ASIO driver) prints one
  "requested input buffer ... was rejected ... falling back" line per start. It is honest but noisy,
  and the clamp in `capture.rs` already narrows the request first.
- "No input device is available" was observed transiently after switching the driver back and forth,
  which points at the shared ASIO host losing its driver across rapid backend changes.
- The state pill can read "the engine is not running" while a device name and counters from the
  previous session are still on screen, because those readouts only update on a successful snapshot.

## ASIO and device changes

ASIO can only hold one driver at a time. The live `switch_device` builds the replacement stream before
dropping the current one, so under ASIO the second driver can never load and the switch fails with
`input device "..." is no longer available`. The window therefore restarts the engine with the chosen
device when the backend is ASIO, and keeps the cheap live switch only for WASAPI.

## Route

- Direct inline. The change is one coherent thread through the engine, the DTO and the window, and
  the verify loop (`cargo test`, a rebuild and one screenshot) is fast enough to run inline.
