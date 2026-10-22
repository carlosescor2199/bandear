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
- **No staleness timer for Bonjour (T4).** Android drops a desk it has not heard for 5 s
  (`ServerBeacon.STALE_TIMEOUT_MS`) because a UDP beacon has no way to say it stopped. Bonjour
  has one: the desk sends a goodbye when it unregisters, and the OS expires records it stops
  hearing. A present desk raises no new events, it stays in the browser's set, so porting the
  timer would remove live desks. `NWBrowser` hands over its whole current set on every change, so
  the directory is one pure function from that set to the list. The original T4 line said
  "staleness handling"; this is what it turned into.
- **Discovery is its own module (T4).** `BandEarDiscovery` sits beside `BandEarProtocol` and
  depends on nothing in it, so the package names its features rather than its layers.
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
- [x] T2c — Read `config` as leniently as Android (route: inline). Found while porting T5b: the
  T2b decoder required `sample_rate`, a group `id` and all four channel fields, so a desk older
  than any of them (the desk added the channel details after the groups, and a group's `id` after
  its name and channels) would cost the musician the whole configuration. Commit `68cb553`.
- [x] T3 — Reorder hold and cushion policy (route: inline). Ported from `ReorderHold.kt` and
  `CushionController.kt`, including server-restart detection.
- [x] T4 — Bonjour discovery (route: inline). `NWBrowser` over `_bandear._tcp`, TXT keys `v` and
  `sr` (`apps/server/src/mdns.rs:63-66`), and the manual host/port fallback. Staleness is the
  browser's, not a timer of ours: see the design decision below.
- [ ] T5 — Mix domain and reducer, in a `BandEarMix` module (route: inline per unit). 32 channels,
  16 groups, gains, pans, mutes, solos, group levels, engineer limit, locked channels, following.
  Mapped by one read-only explorer first (the Android reducer holds the whole app's state, about
  2 500 lines with its tests, beyond an inline read); its reconciliation claim spot-checked against
  `MonitorReducer.kt:123-125,428-437` and `MonitorUiState.kt:262-278`. Sliced into units of
  behaviour, each with its Android parity suite:
  - [x] T5a — Channel layout: strips, stereo pairs, balance and pans, labels, the musician's
    channel choices (`ChannelLayout.kt`, `ChannelLayoutTest.kt`, the layout tests of
    `MonitorReducerTest.kt`).
  - [x] T5b — Edits and config: applying `config`, strip edits through pairs, locks and following,
    group levels, master, mute and cap, clamping.
  - [x] T5c — Reconciliation with the desk: `MixSyncState` numbering, `mix_state` and `mix_ack`
    rules, the engineer's controls, the echo check, session start.
  - [ ] T5d — More of me: derived at send time and never stored, its notices and turn-off paths,
    the musician's own levels put back after a link.
  - [ ] T5e — The outgoing mix and when one is sent.
- [ ] T9 — Mix banks and recall (route: to be decided). Device-local saved mixes
  (`MixBankStore.kt`) and recalling one into the mix. Split from T5 because it needs device
  storage, which the pure mix module does not have.
- [ ] T6 — App shell and project file (route: to be decided). SwiftUI app, `Info.plist` with
  `NSLocalNetworkUsageDescription` and `NSBonjourServices`, composition root over `BandEarKit`.
- [ ] T7 — UDP receive path and AVAudioEngine output (route: to be decided).
- [ ] T8 — Screens: Connect, Mix, Mixes, Settings (route: to be decided).

## Follow-ups

- **A comment of T2a overstates.** `MixCommand`'s doc says sending a whole mix to move one fader
  "would fight every other socket of the same musician". That was never checked, and the Android
  client sends all five lists on every `mix` (`ControlChannel.kt:470-504`). The wire rule it rests
  on is right (an absent list is unchanged); the rationale is not. Correct it in T5e, which builds
  the outgoing mix.
- **A crashed desk lingers in the list.** The desk announces with mdns-sd's defaults:
  `DNS_OTHER_TTL` 4500 s for PTR and TXT, `DNS_HOST_TTL` 120 s for SRV and addresses
  (`mdns-sd` 0.21.5, `src/service_info.rs`). A desk that stops cleanly sends a goodbye and leaves
  the list at once (proved by the round-trip test). One that crashes sends none: the phone can keep
  listing it for up to 75 minutes, while connecting to it fails after about 2 minutes, once its
  SRV and address records expire. Android drops it in 5 s. Two ways out, neither in this feature's
  scope: the desk could announce PTR and TXT with a short TTL (`ServiceInfo::set_other_ttl`, a
  server change), and T6 must make a failed connect to a listed desk say so plainly.
- **T6 inputs from T5c.** `MixEvent(reportedBy:)` maps `config` to nil, so the session layer
  sends `.configReceived` to the reducer itself. Two decoding divergences left on purpose, both
  about what T6 shows: a `mix_ack` with no number whose mix cannot be read still throws, where
  Android only counts it as acknowledged (unreachable while every `mix` is numbered, which T5e
  keeps; if the pill needs it, give `unreadableMixAck` an optional sequence); and an `error` whose
  `seq` is negative or not a number fails as a whole message, where Android shows the error and
  answers nothing (make its `seq` as lenient as the ack's). The "acknowledged" counting and
  "nothing sent back" assertions of `ControlChannelTest.kt` belong to T6's transport.
- **`BandEarProtocol` holds more than the protocol.** `ReorderHold` and `CushionController` are
  playback timing. When T7 adds the player that depends on them, move them to a
  `BandEarPlayback` module so every module names its feature.
- **T6 inputs from T4.** `Info.plist` needs `NSBonjourServices` = `_bandear._tcp` and an
  `NSLocalNetworkUsageDescription`, or the browser waits on `kDNSServiceErr_PolicyDenied`
  forever. The browser spells a found desk's endpoint `type="_bandear._tcp"` (no trailing dot)
  and `domain="local."` (with one), observed in the round-trip test.
- **T8 inputs from T4.** The connect screen owns the form's words and must use Android's:
  `missingHost` "Enter the server IP address or host name", `invalidAudioPort` "Enter a valid UDP
  port between 1 and 65535", `invalidControlPort` "Enter a valid WebSocket control port between 1
  and 65535". `DiscoveryStatus.localNetworkDenied` needs its own message: without it the screen
  says "searching" for a desk that can never appear.

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
- `swift build --build-tests` on a **clean copy** of the package (`rsync` without `.build`) with no
  warnings. A plain `swift build` does not compile the test targets, and an incremental build
  after `swift test` recompiles nothing and so prints no warnings at all: both report clean
  whatever the code says.
- `grep -rE 'import (SwiftUI|UIKit|AVFoundation)' apps/ios/BandEarKit/Sources` returns nothing.
- `BANDEAR_BONJOUR_IT=1 swift test --package-path apps/ios/BandEarKit --filter roundTrip`: the
  Bonjour round trip. Opt-in, because it announces a service on the local network; the
  adapter has no other coverage, so run it whenever `BonjourDeskBrowser` changes.
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
- T4 done in two work units (route: inline), in a new `BandEarDiscovery` module.
  - Reference first, this time from the start: `ServerDiscovery.kt` and `ServerDiscoveryTest.kt`
    (the beacon parse and the 5 s expiry), the desk's `mdns.rs`, and the Android connection form
    in `MonitorViewModel.startSession` with its test `aManualAddressIsCheckedBeforeAnythingStarts`.
    The Network APIs were checked against the iOS 27 SDK's `Network.swiftinterface` and
    `dns_sd.h` before use, not recalled.
  - T4a `Sources/BandEarDiscovery/DeskDirectory.swift`: `DeskDirectory.desks(from:)`, a pure
    function from the browser's current set to the list. A desk is listed once its TXT record
    has arrived and only at discovery version 1, as Android ignores a beacon of another version.
    `sr` is informational: anything but a positive whole number is no rate. A service reported
    twice is listed once, and when its copies disagree on the rate the rate is dropped, so the
    answer never depends on the set's iteration order. The order is by name, ignoring case and
    reading digits as numbers, then domain and type, so it is total and stable between snapshots.
    The service type is not filtered on: the browser was asked for that type only, and its
    spelling in a result was not yet known. `Sources/BandEarDiscovery/ConnectionForm.swift`: the
    Android checks in Android's order (host, then this phone's UDP port, then the desk's control
    port), ports 1 through 65535 trimmed of blanks, defaults 50001 and 50000; a desk found over
    Bonjour only needs this phone's port. The checks return cases; the words belong to T8 (see
    Follow-ups). `ConnectionFormParityTests` ports Android's inputs. Commit `b27458c`.
  - T4b `Sources/BandEarDiscovery/DiscoveryStatus.swift`: the browser state read as what the
    screen says, with `localNetworkDenied` for `kDNSServiceErr_PolicyDenied` (-65570, verified
    in the SDK's `dns_sd.h:801`) whether the browser waits or fails on it.
    `Sources/BandEarDiscovery/BonjourDeskBrowser.swift`: the thin `NWBrowser` adapter. It clears
    a browser's handlers before cancelling it, so an old browser's `cancelled` cannot land after a
    new one reported browsing and make a live screen read idle. Its state is only touched on its
    own serial queue. Commit `9ece87a`.
  - The adapter cannot be unit tested; its input is a network. `BonjourDeskBrowserIntegrationTests`
    announces a desk from the test process with `NWListener` (TXT `v=1`, `sr=48000`), finds it,
    cancels the listener and waits for it to leave the list. RED against a stub that never
    reported: failed after 10 s with the listener `ready`, so a later pass is the browser's doing.
    GREEN: passed in 1.0 s; the desk was found with its rate and left the list on its own once the
    listener stopped, which is the evidence behind having no staleness timer. It announced
    `BandEar IT <8 hex>` on the local network for about a second.
- Test-first: RED before each implementation. T4a: 24 new tests, 37 failed expectations. T4b: 5
  new tests, 8 failed expectations, plus the round trip's RED above.
- Checks: `swift test --package-path apps/ios/BandEarKit`: 156 passed, 0 failed (127 in
  `BandEarProtocolTests`, 29 in `BandEarDiscoveryTests`; the round trip is skipped unless opted
  in). `BANDEAR_BONJOUR_IT=1 … --filter roundTrip`: passed. `swift build --build-tests`: no
  warnings. No SwiftUI, UIKit or AVFoundation import in `Sources`. The only earlier file touched
  is `Package.swift`, which gained the new target; the earlier 127 tests pass unchanged.
- Risk: none. Additive module nothing depends on yet. The adapter's only coverage is the opt-in
  round trip, recorded under Checks so it is run whenever the adapter changes.
- Native review assessment of the T4 slice against base fcff93e: `medium`, `review_due: false`,
  reason `under_budget` (10 paths, 717 lines). The slice stays open.
- T5 started. One read-only explorer mapped the mix domain inside Android's whole-app reducer
  (about 2 500 lines with its tests, beyond an inline read) and returned the state shape, events,
  rules, the tests by area and two corrections to the brief: there is no "mix ownership" in the
  reducer (in-flight edits are reconciled by `MixSyncState(latest, answered)`), and Android sends
  all five lists on every `mix`. Its reconciliation claim was spot-checked against
  `MonitorReducer.kt:123-125,428-437` and `MonitorUiState.kt:262-278` and held exactly.
  - Module decision: `BandEarMix` depends on `BandEarProtocol` and takes the desk's values at its
    edge (`DeskConfiguration`, `MixReport`, `MixCommand`); the protocol types are the vocabulary
    the desk publishes, and a parallel set would be mapping for its own sake. The dependency only
    runs one way.
  - T5a `Sources/BandEarMix/ChannelLayout.swift` and `MixState.swift`: strips, stereo pairs, the
    balance rule, labels, group faders and their members, the musician's channel choices, locked
    sides and following, all derived from plain index-aligned lists. `ChannelLayoutParityTests`
    ports `ChannelLayoutTest.kt` (9); `MixReducerParityTests` ports the four layout tests of
    `MonitorReducerTest.kt`. Commit `878b35b`.
    - Kotlin's `roundToInt` and Swift's `rounded()` disagree on negative halves; both are then held
      to 0-100, which makes them agree everywhere, and the round trip over 0...100 pins it.
    - The first RED crashed the test process (`Index out of range` on an empty stub result) rather
      than failing. Indexing is now behind `try #require(count == …)`, so a short result fails one
      test instead of hiding every other result.
    - Two implicit dependencies made explicit: the test target imported `BandEarProtocol` only
      through `BandEarMix`, and `ChannelLayout.swift` used `trimmingCharacters` without importing
      Foundation. Both compiled; both would break the day a transitive import changed.
  - T2c (above): the `config` decoder now reads as `ControlChannel.kt` does. `sample_rate` is
    optional (Android never reads it, and its own tests send configs without one); groups skip an
    entry that is not an object or has no name, take `id` as optional and drop member channels that
    are not whole numbers or are negative; `channels` absent or not a list is nil, otherwise exactly
    `source_channels` entries with defaults for anything unreadable. Where Android's `optString`
    and `optInt` coerce garbage (a name of `7`, a channel of `2.5`) this drops it instead, documented
    at the decoder; the desk never sends either. `ConfigParityTests` ports
    `aConfigHandsOnTheChannelDetailsInChannelOrder`, `aConfigFromAnOlderServerHandsOnNoDetails`,
    `channelDetailsAreReadLeniently` and `everyKindIsReadByItsWireName` with Android's own JSON.
    RED in two steps: first a compile error from the API change, then, with only the types made
    optional, exactly the ten new leniency tests failing and every earlier test passing. Scope
    checked on the diff: no line of the routing or of the other four decoders changed.
  - T5b `Sources/BandEarMix/MixSession.swift`: the reducer for `config` and the musician's edits.
    A `config` outside 1...32 channels is ignored (Android drops it in its parser); lists resize
    keeping existing channels, new ones neutral with the default pans, group levels kept by
    position. Strip edits go through pairs, refuse locked strips and anything while following;
    master, mute and cap never are. Android writes by index unguarded and would throw on a list
    shorter than the levels; this writes only what a list has room for. The T5a ports now reach
    their states through the reducer, and `firstConfigBridge` proves the reducer builds exactly
    the hand-built states the layout tests started from. 13 more ports from `MonitorReducerTest.kt`;
    `aGroupLevelFillsTheOtherGroupsAsTheyRead` sets the short group list directly where Android
    reaches it through a recall (T9). Commit `ce9ef1f`.
  - Test-first: RED before each unit. T5a: 24 tests, 156 issues (after the crash fix). T5b: 44
    tests, 54 issues, the T5a ports included because they now go through the stub reducer.
- A correction to every assessment recorded so far. The rule takes `--base-ref <last reviewed
  boundary>`, and `under_budget` does not advance the boundary, so with nothing reviewed it is
  still the branch point. T1 was assessed against it, but T2, T3 and T4 were each assessed against
  the previous docs commit, which reset the slice every time. Re-assessed correctly after T5b:
  base 07a8711, 33 paths, 5 427 lines, `medium`, `review_due: false`, `under_budget`. The
  outcome did not change, but the bases were wrong. Assessments from here on use 07a8711 until
  something is reviewed.
- Checks: `swift test --package-path apps/ios/BandEarKit`: 210 passed, 0 failed (137 protocol,
  29 discovery, 44 mix). `swift build --build-tests`: no warnings. No SwiftUI, UIKit or
  AVFoundation import in `Sources`.
- Risk: none for T5a and T5b (additive module, nothing depends on it yet). T2c changes public
  types of `BandEarProtocol` (`DeskConfiguration.sampleRate` and `.channels`, `ChannelGroup.id`
  became optional); their only consumers are this package's tests and `BandEarMix`, so no
  contract anyone already consumes changed.
- T5c done (route: delegated writer; trigger: the long-session backstop, the parent context at
  about 130k tokens). The parent fixed the API shape in the handoff (the `unreadableMixAck` case,
  `MixSync`, the four new events, `reducedSending`, the `ServerMessage` to `MixEvent` mapping) and
  the writer implemented it within eleven named edit surfaces. What landed:
  - `ServerMessage.swift`: `mix_ack` and `mix_state` read as `parseMixReport` and
    `parseEngineerControls` read them. The masters, the mute and all five lists are required now,
    so a report missing a list is unreadable (T2b had read it as empty); the limit is any number cut
    to its whole part and held to 0-100, else 100; `locked` counts only `true` entries; `following`
    is only a string; `seq` is only a non-negative whole number. A numbered ack whose mix cannot be
    read becomes `unreadableMixAck(sequence:)` and still answers its number. No other message type
    changed, and no existing test did.
  - `MixSync.swift`, `MixSession.swift`: `MixSync(latest, answered)` with `isSettled`, `changed()`,
    `answeredUpTo(_:)`; `sync` and `isSessionRunning` on the session; `.sessionStarted`,
    `.sessionEnded`, `.mixPushed`, `.mixAnswered`. A push applies its mix only when settled, an ack
    only when it answers the newest change; engineer controls in a readable report apply at once; a
    refusal only answers; reports outside a session change nothing (the answered number still moves,
    as Android's does); session start resets the engineer controls, never the numbers. The echo
    check compares against `mixAsPlayed`, the stored mix until T5d lifts it.
  - `DeskReports.swift`: `MixReport.engineerControls` and `MixEvent(reportedBy:)`.
    `EngineerControls.isLimited` added because two parity tests assert Android's `limited`.
  - Ports: 26 from `MonitorReducerTest.kt` (795, 807 and 1166 partly, their More of me parts left
    to T5d; 914's snapshot check to T5e; 776 sets directly the levels Android reaches by a recall)
    and 12 from `ControlChannelTest.kt`. Skipped for later units: four recall tests (T9), four More
    of me tests (T5d), two outgoing-mix tests (T5e). 20 Swift-only tests.
  - RED against the writer's stub: 46 of 239 tests failed with 89 issues, no crash. GREEN: 268.
    The writer also ran four mutations in a scratch copy (a push ignoring settled, any answer shown,
    engineer controls changing outside a session, a mix applied outside one); each was caught.
  - About 1 250 authored lines, over the ~400 heuristic: some 320 are source, the rest the parity
    suites, every port reaching its state through events.
- Parent checks of the delegated unit: every changed file inside the eleven surfaces; `swift test`
  rerun by the parent: 148, 91 and 29 passed; scratch paths the writer reported removed after
  inspection.
- The warning check had been vacuous. The writer noticed that an incremental `swift build
  --build-tests` after `swift test` recompiles nothing and so prints nothing, which is the order
  every check since T1 ran in (with `swift test`'s own output filtered down to its result lines).
  That explains why T3's `unused result` warning only surfaced in a fresh scratch copy. A full
  build of a clean copy after T5c: 0 warnings in sources and tests, so the tree is clean as of
  `9982f3b`. The documented check now builds a clean copy.
- Risk: none. The writer named item 3 because `ServerMessage` gains a public case; as with T2c,
  its only consumers are this package's own targets, which compile and pass, so no contract anyone
  already consumes changed, and no independent verifier ran.
- Work-unit commit: `9982f3b feat(ios): reconcile the desk's mix reports with the edits in flight`.
- Native review assessment against base 07a8711: `medium`, `review_due: false`, `under_budget`
  (37 paths, 6 706 lines).
- Pending: T5d, T5e. The `.xcodeproj` decision is still open and belongs to T6.
