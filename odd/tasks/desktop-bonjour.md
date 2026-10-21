# Desktop Bonjour (mDNS) advertisement

## Objective

The desk also announces itself over Bonjour/mDNS as `_bandear._tcp`, so an iPhone can find it with
`NWBrowser` without Apple's multicast entitlement (which needs a paid developer account).

## Problem and why

Discovery today is a UDP broadcast beacon (`MLNK`, `255.255.255.255:50002`, every second; see
`apps/server/src/discovery.rs`). iOS only lets an app receive broadcast or multicast with the
`com.apple.developer.networking.multicast` entitlement, granted on request to paid accounts. Bonjour
browsing needs only the local-network permission. The user has no paid account yet but wants to
build the iOS app now.

## Scope

- Additive: the broadcast beacon, the Android app, the wire protocol, audio and control are untouched.
- The engine registers one mDNS service while it runs and unregisters it when it stops.
- A failure to start mDNS is logged and never stops the engine or the beacon.

## Constraints

- Crate `mdns-sd` pinned to `=0.21.5` (released 2026-10-05; 0.21.6 was two days old). User approved
  the download and one debug test build on 2026-10-21.
- Builds: `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, debug only, one build at a time, stop if F: has
  less than 5 GB free or rustc crashes. No `--release`, no `cargo clean`.

## Tasks

- [x] T1 — mDNS advertiser in the engine (route: delegated writer; trigger: 2+ non-trivial files —
  Cargo.toml, a new module, engine wiring). Service `_bandear._tcp.local.`, port = control port,
  TXT `v=1` (discovery version) and `sr=<sample rate>`, registered next to the beacon thread and
  unregistered on shutdown. Unit tests for the service description; failure path logged.
- [x] T2 — Docs: root README "How it works" mentions Bonjour next to the beacon.

## Acceptance criteria

- `cargo test -p personal-monitoring` passes (existing discovery tests unchanged).
- The service description is `_bandear._tcp.local.` with the control port and the TXT keys.
- Engine start and stop register and unregister the service; an mDNS error does not fail the start.

## Checks

- `cargo test -p personal-monitoring` (debug, limited jobs).
- Manual, later on the Mac mini: `dns-sd -B _bandear._tcp` lists the desk.

## Progress

- Branch `feat/desktop-bonjour` from main c8c5f27.
- T1 + T2 (delegated writer): `mdns-sd =0.21.5` with `default-features = false` (compiles; adds
  flume 0.12.0, if-addrs 0.15.0, socket-pktinfo 0.4.1, spin 0.9.9). New `apps/server/src/mdns.rs`:
  pure `describe_service` (type `_bandear._tcp.local.`, instance `BandEar on <host>` cut to 63
  bytes on a character boundary, fallback `BandEar`, own address-record host
  `bandear-<host>.local.` so it never collides with the OS responder's `<host>.local.`, port = the
  control port, TXT `v=1` from `DISCOVERY_VERSION` and `sr=<rate>`). `start_bonjour` registers
  with automatic addresses and the loopback interfaces disabled; `BonjourAdvertiser::stop` (also
  run on drop) unregisters and shuts the daemon down, each step bounded to 1 s. Every mDNS error is
  a `log_warn!("bonjour", ...)` and the engine and beacon keep running. Control port 0 (OS chosen)
  is not announced, which also keeps the demo-start test off the network.
- Test-first exception: RED was not run as a separate build because the approved budget was one
  debug test build; the 9 new unit tests were observed passing in that build.
- Checks: `cargo check -p personal-monitoring` clean, no warnings; `cargo test -p
  personal-monitoring`: lib 657 passed, 0 failed, 0 ignored; main.rs 0; doc-tests 0. Existing
  discovery tests unchanged.
- Pending: manual `dns-sd -B _bandear._tcp` on the Mac mini.
- Work-unit commit: `8b37cfd feat(server): announce the desk over Bonjour for iOS` (T1 + T2).
- Native review review-264d0aa2cc03dd30 (medium, one lens): approved and acknowledged. Its WARNING
  (dots inside a host name split the instance label) fixed on request: `display_host` keeps only
  the first label; new test `a_full_domain_name_keeps_only_its_first_label`. Test-first exception:
  one build only (no separate RED) to limit build load. `cargo test -p personal-monitoring`: 658
  passed, 0 failed. Suggestions left open: all-non-ASCII host names share the `bandear.local.`
  fallback; start/stop proved only by the manual check.
