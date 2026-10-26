# iOS client

## Objective

A native iOS app that finds the desk over Bonjour, receives its personal mix over UDP, plays it
through AVAudioEngine, and gives the musician the same control surface the Android client already
has.

## Problem and why

The desk already serves any number of clients (`apps/server/src/network.rs`), but only Android can
listen. The PRD commits to native apps on both platforms (`PRD.md:7`, `PRD.md:69`) and the server
grew a Bonjour advertisement specifically so an iPhone could find it without the paid-account
multicast entitlement (`odd/tasks/desktop-bonjour.md`). Nothing on the iOS side exists yet:
`apps/` holds `android`, `desktop` and `server` only.

## Design decisions

- **The core is a Swift package, not an app target (T1).** `apps/ios/BandEarKit` holds the wire
  codecs, the jitter policy and the mix domain as plain Swift with no SwiftUI, UIKit or AVFoundation
  import. It runs under `swift test` from the command line: no `.xcodeproj`, no simulator, no
  Homebrew tooling. The app shell composes it later (T6) and stays thin.
- **No generated project file yet.** XcodeGen and Tuist are both absent from this machine and a
  hand-written `.xcodeproj` is an unmergeable blob. The decision is deferred to T6, where the shell
  is the first thing that actually needs one.
- **Bonjour only for discovery (T4).** The `MLNK` broadcast beacon the Android client uses
  (`apps/server/src/discovery.rs:9-26`) is unreachable from iOS without
  `com.apple.developer.networking.multicast`. `NWBrowser` over `_bandear._tcp` needs only the
  local-network permission. A typed host/port form is the fallback, as on Android.
- **The Android jitter strategy is ported, not reinvented (T3).** Cushion counted in packets, 1 to 4,
  starting at 2, shrinking only after 3 clean intervals (`PlaybackLatency.kt:15-21`,
  `CushionController.kt:34-101`), and a reorder hold of up to 4 packets that reads a sequence 256
  behind as a server restart (`ReorderHold.kt:11-21`). Both clients then behave the same under the
  same WiFi, which is what makes a bug report from either one worth anything.
- **No clock synchronisation.** Latency comes from the audio engine's own render timestamps, the way
  Android reads it from `AudioTrack.getTimestamp` (`PlaybackLatency.kt:5-10`).
- **Mix banks stay on the device.** They are not on the wire on Android (`MixBankStore.kt`) and they
  will not be here.
- **Protocol types are hand-written `Codable` with explicit `CodingKeys`.** The server names every
  field in snake_case through `#[serde(rename = "type")]` with no `rename_all`, so a global
  key-encoding strategy would be a lie that holds right up until one field disagrees.

## Scope

- `apps/ios/BandEarKit`: PMON datagram decoding, control-message codecs both ways, reorder and
  cushion policy, mix state and its reducer.
- `apps/ios/BandEar`: SwiftUI shell, Bonjour browser, UDP socket, AVAudioEngine output, and the
  Connect / Mix / Mixes / Settings screens the Android app has (`ui/screens/AppScreen.kt:6-15`).
- Feature parity target: per-channel fader, pan, mute and solo, channel detail, group chips, "More
  of me", saved mix banks, link-health indicator, diagnostics.

## Out of scope

- Any change to the server, the wire protocol, the desktop app or the Android app.
- App Store distribution, provisioning profiles, paid-account entitlements.
- Recording, presets and the installer (M4 in `PRD.md:91`).

## Constraints

- iOS 16+ (`PRD.md:69`). Xcode 27.0, Swift 6.4, iOS 27.0 SDK on this machine.
- Audio output is AVAudioEngine (`PRD.md:81`).
- The audio render callback never allocates, locks or blocks.
- No third-party Swift dependencies; Foundation and Network only.

## Tasks

- [x] T1 — `BandEarKit` package skeleton and the PMON datagram decoder (route: inline; one unit of
  behaviour). 20-byte little-endian header (`PMON`, version 1, channels, sample rate, u64 sequence,
  u16 sample count) then interleaved stereo PCM16, per `apps/server/src/protocol.rs:476-498`.
  Rejects a wrong magic, an unknown version, a truncated header, a payload that disagrees with
  `sample_count`, and an odd sample count against a stereo stream. Golden bytes from the Rust test
  at `protocol.rs:665-672`.
- [x] T2 — Control-message codecs both directions (route: inline). `register`, `mix`, `stats` out;
  `config`, `mix_ack`, `mix_state`, `playback`, `redundancy`, `error` in. Field-for-field against
  `apps/server/src/protocol.rs`, with the `register` name truncation at 64 characters and the
  `client_id` character rule.
- [x] T3 — Reorder hold and cushion policy (route: inline). Ported from `ReorderHold.kt` and
  `CushionController.kt`, including server-restart detection.
- [ ] T4 — Bonjour discovery (route: inline). `NWBrowser` over `_bandear._tcp`, TXT keys `v` and
  `sr` (`apps/server/src/mdns.rs:63-66`), staleness handling, and the manual host/port fallback.
- [ ] T5 — Mix domain and reducer (route: inline). 32 channels, 16 groups, gains, pans, mutes,
  solos, group levels, engineer limit, locked channels, following.
- [ ] T6 — App shell and project file (route: to be decided). SwiftUI app, `Info.plist` with
  `NSLocalNetworkUsageDescription` and `NSBonjourServices`, composition root over `BandEarKit`.
- [ ] T7 — UDP receive path and AVAudioEngine output (route: to be decided).
- [ ] T8 — Screens: Connect, Mix, Mixes, Settings (route: to be decided).

## Acceptance criteria

1. `swift test` passes in `apps/ios/BandEarKit` with no Xcode project and no simulator.
2. The decoder accepts the golden packet from the server's own Rust test byte for byte, and rejects
   each malformed case with a distinguishable error.
3. Every control message round-trips to the exact JSON the server's serde attributes produce,
   including snake_case keys and the `type` tag.
4. The cushion and reorder logic reproduce the Android behaviour on the same inputs.
5. The app finds a running desk over Bonjour and plays its mix on a device.
6. `BandEarKit` imports no SwiftUI, UIKit or AVFoundation.

## Checks

- `swift test --package-path apps/ios/BandEarKit`.
- `swift build --build-tests --package-path apps/ios/BandEarKit` with no warnings. A plain
  `swift build` does not compile the test targets, so it reports their warnings as clean.
- `grep -rE 'import (SwiftUI|UIKit|AVFoundation)' apps/ios/BandEarKit/Sources` returns nothing.
- Manual, later: `dns-sd -B _bandear._tcp` lists the desk, then the app connects and audio plays.

## Route

- T1: inline, one unit of behaviour (package manifest plus one decoder with its tests).
- T2, T3, T5: inline, one unit each.
- T4: inline, with the browser isolated behind a protocol so the policy stays testable.
- T6, T7, T8: route decided when reached; T7 and T8 are candidates for parallel writers once the
  shell exists and their edit surfaces are disjoint.

## Delivery forecast

Well over the ~400 authored-line work-unit heuristic for the feature as a whole, so it ships as a
chain rather than one pull request. Provisional slices: T1-T3 (wire and timing core), T4-T5
(discovery and domain), T6-T7 (shell and audio), T8 (screens). Strategy: `ask-on-risk`.

## Progress

- Branch `feat/ios-client` from main 07a8711.
- T1 done (route: inline, one unit of behaviour). What landed:
  - `apps/ios/BandEarKit/Package.swift`: swift-tools-version 6.0, platforms iOS 16 and macOS 13,
    one `BandEarProtocol` library target plus its test target. No dependencies, so `swift test`
    needs nothing but the toolchain already on the machine.
  - `Sources/BandEarProtocol/AudioDatagram.swift`: `AudioDatagramHeader` parses the 20 documented
    bytes and `AudioDatagram` adds the interleaved PCM16 payload. A private `ByteReader` reads
    little-endian fields by offset from `startIndex`, not by absolute index, so a `Data` slice
    decodes the same as an array. `AudioDatagramDecodingError` names each refusal:
    `headerTooShort`, `unknownMagic`, `unsupportedVersion`, `zeroChannels`,
    `payloadLengthMismatch`, `sampleCountNotDivisibleByChannels`. Validation order is magic,
    version, channels, then sample count, so the magic check speaks first when something else is
    on the port. `frameCount` divides by `channels` safely because a zero-channel header never
    decodes. The header is split from the payload deliberately: T7's render path reads the header
    and copies PCM straight into its own buffer instead of allocating a `[Int16]` per packet.
  - `Tests/BandEarProtocolTests/AudioDatagramTests.swift`: 16 tests. The golden packet is the
    server's own, copied byte for byte from `apps/server/src/protocol.rs:656-671`, so the two
    implementations are pinned to the same 24 bytes.
- Test-first: RED observed before implementation against a stub holding the same API with no
  behaviour — 15 tests, 22 failed expectations, the only pass being the `byteCount == 20`
  constant. GREEN after implementing: 15 passed. A 16th test was then added because a code comment
  claimed `Data` slices with a non-zero `startIndex` decode correctly and nothing proved it;
  `decodesFromOffsetSlice` proves it. Final: 16 passed, 0 failed.
- `.gitignore`: SwiftPM and Xcode build output, every pattern scoped under `apps/ios/**`. Caught
  because `apps/ios/BandEarKit/.build` had already grown to 65 MB and `apps/ios/` was untracked as
  a whole. Verified with `git add -An apps/ios` (exactly 3 source files staged) and by piping
  `git ls-files` through `git check-ignore` (no tracked file became ignored).
- Checks: `swift test --package-path apps/ios/BandEarKit`: 16 passed, 0 failed.
  `swift build --package-path apps/ios/BandEarKit`: clean, no warnings.
  `grep -rE 'import (SwiftUI|UIKit|AVFoundation)' apps/ios/BandEarKit/Sources`: no matches.
- This work unit is roughly 490 authored lines, over the ~400 heuristic, because the feature
  document ships with the first task rather than as a commit of its own. Noted, not split.
- Risk: none. A new isolated package nothing depends on yet; the `.gitignore` additions are all
  scoped to the new `apps/ios/**` tree.
- Work-unit commit: `e519a53 feat(ios): decode the PMON audio datagram in a new BandEarKit
  package` (T1).
- Native review assessment of e519a53 against base 07a8711: `medium`, `review_due: false`,
  reason `under_budget` (5 paths, 503 lines). No review for this commit; the slice stays open and
  accumulates until a later commit reaches the budget, then the slice is assessed as a whole. The
  first assess attempt returned `unassessable` with `changed_paths: 0` because the pre-existing
  untracked `.atl/` directory needs an explicit declaration; rerun with
  `--untracked-scope=exclude` and the expected inventory hash, which is what produced the verdict
  above. At `medium` with `Risk: none`, writer self-verification stands and no independent
  verifier runs.
- T2 done in two work units (route: inline). Delegation was considered and declined: the two
  directions are independent units with disjoint files, which does fire the parallel-writer
  trigger on shape, but they share the conventions that matter (snake_case `CodingKeys`, omit
  rather than send null, the `type` tag, the lenient-default policy) and a writer cannot see its
  sibling. Explaining those invariants twice in two prompts costs more than writing them once,
  and the whole of `protocol.rs` was already read for T1.
  - T2a `Sources/BandEarProtocol/ClientMessage.swift` (+ tests): `ClientMessage` with
    `register`, `mix` and `stats`. A `ControlPayload` protocol keeps each `type` string in exactly
    one place. `Registration` normalises the way the desk does, so what the phone shows is what
    the desk stores: the name trimmed and capped, a client id the desk would discard dropped here
    rather than sent, capabilities deduplicated in order, and a zero UDP port refused outright.
    `MixCommand` leaves an unset list out of the JSON instead of sending null, because the desk
    reads an absent list as unchanged and sending a whole mix to move one fader would fight every
    other socket of the same musician. `LinkStats` validates the interval and sanitises the
    optional measurements on the way in.
  - T2b `Sources/BandEarProtocol/ServerMessage.swift` (+ tests): `ServerMessage` decoding
    `config`, `mix_ack`, `mix_state`, `playback`, `redundancy` and `error`. An unknown `type`
    becomes `unrecognized(type:)` rather than an error, which is the desk's own repeated contract,
    and keeping the name lets a diagnostics screen say what arrived. Fields added after a message
    shipped are read with the documented fallback (`engineer_limit_percent` 100, `locked` all
    false at the channel count, `following` nil) so an older desk still drives this client. One
    `translatingMissingFields(in:)` turns a missing required key into `missingField` for every
    message instead of each one catching for itself.
- Two decisions worth naming:
  - The name is cut to 64 **Unicode scalars**, not 64 Swift `Character`s, because the desk cuts
    with Rust's `chars().take(64)` and `char` is a scalar. Counting graphemes would keep more
    scalars than the desk stores and the phone would display a name the desk does not have.
    Pinned by `truncatesNameByScalar` with a five-scalar ZWJ emoji.
  - `LinkStats` drops a non-finite measurement. `JSONEncoder` *throws* on a non-finite double, so
    a NaN jitter reading would lose the whole report mid-send over a value the desk was going to
    discard anyway. Pinned by `dropsNonFiniteMeasurement`.
- Test-first: RED observed before each implementation. T2a: 39 tests, 37 failed expectations
  against a stub holding the API with no behaviour, then 39 passed. T2b: 63 tests, 37 failed
  expectations, then 63 passed.
- One self-correction during T2b: the first routing attempt passed a closure into the
  missing-field translator so the string `config` could live in "one place"
  (`case payloadType(DeskConfiguration.self):`). That is cleverness that costs readability for
  nothing; the literal now sits in the switch where a reader expects it. A separate compile error
  came from a `...` range operator opening a continuation line, where Swift reads it as a prefix
  operator; the bounds are named locals now.
- Checks: `swift test --package-path apps/ios/BandEarKit`: 63 passed, 0 failed.
  `swift build`: clean, no warnings. No SwiftUI, UIKit or AVFoundation import in `Sources`.
  Neither work unit touched a file from an earlier task, so no existing behaviour is in reach.
- Risk: none. Additive files in a package nothing depends on yet.
- Work-unit commits: `7345c11 feat(ios): encode the register, mix and stats control messages`
  (T2a) and `fe64a44 feat(ios): decode the desk's config, mix, playback and error messages`
  (T2b).
- Native review assessment of the T2 slice against base 9b0fd19: `medium`, `review_due: false`,
  reason `under_budget` (4 paths, 1282 lines). No review for this slice yet; it stays open.
- T3 done in two work units plus a parity suite (route: inline).
  - T3a `Sources/BandEarProtocol/ReorderHold.swift`: a line-for-line port of `ReorderHold.kt`.
    A `final class` rather than a struct, so an accidental copy can never duplicate its arrays on
    the receive path. One deliberate difference: Android reads the u64 sequence as a signed `Long`
    and Kotlin's arithmetic wraps, while Swift's traps, so a corrupt packet with the top bit set
    would crash the receive loop. `offer` refuses a sequence above `Int64.max` before it reaches
    any arithmetic; no stream reaches 2^63 blocks, so every real sequence behaves as on Android.
    Commit `89d7fd4`.
  - The suite first written for T3a was traced by hand through the Kotlin, case by case. Then
    `ReorderHoldTest.kt` turned up: 19 tests, including scenarios the hand-traced suite did not
    cover (the full redundancy stream with `dropped == 8`, the hold-4 burst that loses originals 6
    and 7, a hold shortened mid-stream). They are ported verbatim as
    `ReorderHoldParityTests.swift` and all 19 passed on the first run. Separate commit `dead274`,
    so the history shows parity arrived after the port. Lesson applied to T3b: look for the
    reference implementation's tests before writing any.
  - T3b `Sources/BandEarProtocol/CushionController.swift`: `CushionPolicy` (`next`,
    `fixedCushionPackets`, `adjustIntervalFrames`, `capacityFrames`) and `CushionController`,
    ported from `PlaybackLatency.kt` and `CushionController.kt`, with `CushionParityTests.swift`
    porting all 11 tests of `CushionControllerTest.kt` and the 5 `nextCushion` tests of
    `PlaybackLatencyTest.kt` from the start. Two translations, both documented in the suite:
    Android's `fixedMs` (an `Int`, 0 for Auto) becomes the `PlaybackBufferSetting` the
    `playback` message already decodes to; and `onPacket` takes frames only, where Android takes
    frames and samples and divides samples by a fixed `CHANNELS = 2`. The header's `frameCount`
    already carries the real channel count, so counting samples as frames cannot be written, and
    the two Android tests that exist to catch that mistake are ported by what they assert about
    time. `capacityFrames` is ported now because the iOS player must be sized by it too.
    Commit `2e52cf5`.
  - Deferred to T7: what Android measures off `AudioTrack` (`pendingFrames`, `backlogMs`,
    `targetBufferFrames`, `cushionMs`, `reportExtras`, `keepsLowLatency`). Its output path
    measures differently from AVAudioEngine's, so porting it now would be guessing.
- The property test, and what it took to give it teeth. `ReorderHoldTests` adds a seeded
  SplitMix64 run over 50 seeds of 500 operations each (reordering, duplicates, loss, far jumps,
  hold changes, releases). Order and uniqueness are largely structural, since `head` never moves
  back, so its real job is catching a hang on the receive thread. Verified by mutation in a
  `mktemp -d` copy outside the worktree, changing `newest - holdPackets` to
  `newest - holdPackets - 1` so `head` can stop advancing:
  - With Swift Testing's `.timeLimit(.minutes(1))`: the run never ended and had to be killed by
    an outer alarm at 201 s (exit 142), leaving an orphaned `swiftpm-testing-helper` spinning.
    Swift Testing cancels cooperatively, and a synchronous loop never yields. The comment that
    said the time limit would catch this was wrong, and was removed.
  - With a watchdog (the run on its own `Thread`, waited on with a `DispatchSemaphore` timeout):
    exit 1 after 23 s, `next() never returned, first in the run for seed 0`, no orphans.
  - The mutation was confirmed absent from the real source and the scratch copy removed.
- The mutation run also surfaced an `unused result` warning in `ReorderHoldTests.swift` that the
  documented check had missed, because a plain `swift build` does not compile test targets. The
  check now uses `--build-tests`; it reports no other warning in any test file from T1 onward.
- Test-first: RED before each implementation. T3a: 87 tests, 45 failed expectations. T3b: 127
  tests, 57 failed expectations. GREEN after each.
- Checks: `swift test --package-path apps/ios/BandEarKit`: 127 passed, 0 failed, across 7
  suites. `swift build --build-tests`: no warnings. No SwiftUI, UIKit or AVFoundation import in
  `Sources`. No file from an earlier task touched.
- Risk: none. Additive files in a package nothing depends on yet.
- Native review assessment of the T3 slice against base f09c8bc: `medium`, `review_due: false`,
  reason `under_budget` (6 paths, 1446 lines). The slice stays open.
- Pending: T4 onward. The `.xcodeproj` decision is still open and belongs to T6.
