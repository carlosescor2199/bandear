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
- **A committed `.xcodeproj` with synchronized folders (T6, user's decision on 2026-10-21).** T1
  deferred the choice because a hand-written project file used to list every source file and was
  an unmergeable blob. Xcode 16 and later reference a folder instead
  (`PBXFileSystemSynchronizedRootGroup`), and this Xcode's own prototype for a new app project
  (`IDEFoundation.framework/…/UntitledAppProjectPrototype/Project.xcodeproj/project.pbxproj`) is
  334 lines with no source file listed, so adding Swift files never touches the project. Chosen
  over XcodeGen (an extra tool for every contributor and CI) and Tuist (more than one app target
  and one local package need). The project is based on that prototype rather than invented.
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
- [x] T5 — Mix domain and reducer, in a `BandEarMix` module (route: inline per unit). 32 channels,
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
  - [x] T5d — More of me: derived at send time and never stored, its notices and turn-off paths,
    the musician's own levels put back after a link.
  - [x] T5e — The outgoing mix and when one is sent.
- [ ] T9 — Mix banks and recall (route: to be decided). Device-local saved mixes
  (`MixBankStore.kt`) and recalling one into the mix. Split from T5 because it needs device
  storage, which the pure mix module does not have.
- [ ] T6 — App shell, project file and control session. Grown since first written: the follow-ups
  of T4 and T5c-T5e handed it the control channel, which no task held. Sliced:
  - [x] T6a — The project and a shell that builds (route: delegated writer; trigger: the
    long-session backstop). `apps/ios/BandEar/BandEar.xcodeproj` from Xcode's app prototype with a
    synchronized `BandEar/` folder; target and product `BandEar`, bundle id `com.bandear.ios`, iOS
    16 deployment target; the local package `../BandEarKit` linked (`BandEarProtocol`,
    `BandEarDiscovery`, `BandEarMix`); `NSBonjourServices` = `_bandear._tcp` and an
    `NSLocalNetworkUsageDescription` in the built app's `Info.plist`; a SwiftUI shell (iOS 16, so
    `ObservableObject`, not `@Observable`) that lists the desks `BonjourDeskBrowser` finds and
    says when local network access is denied. Proof of life: built for the simulator, launched on
    one, a desk announced with `dns-sd -R` appears.
  - [x] T6b — The control session, in a `BandEarSession` module of `BandEarKit` (route: delegated
    writer; trigger: the long-session backstop). A `ControlTransport` port with a fake for tests and
    a `WebSocketTransport` adapter (`NWConnection` with `NWProtocolWebSocket`, which connects to a
    Bonjour service endpoint itself and answers the desk's pings); a `ControlSession` core that
    registers on open (no capabilities until T7 implements them, so the desk sends nothing this
    build cannot use), then sends the opening mix, routes every desk message (`config` to the
    reducer directly, reports through `MixEvent(reportedBy:)`), runs every event through
    `reducedNumberingSends` and sends `outgoingMix()` whenever `sync.latest` grew, keeps the desk's
    playback and redundancy settings for T7, and sends `stats` only while open. No app changes, no
    reconnect.
  - [x] T6c — Session robustness, in `BandEarSession` (route: delegated writer; trigger: the
    long-session backstop). `ReconnectPolicy.kt` (0.5, 1, 2, 4 s, then 5 s; reset on the first
    message, not on open) and its tests; a connect timeout (a listed Bonjour desk that is gone, or
    a host dropping SYNs, otherwise leaves the session `connecting` for good); the phone's own pings
    so a dead link is noticed (Android pings every 5 s, `ControlTransport.kt:19,77`); `connection`
    set before `.sessionStarted` and `.sessionEnded` are reduced; tests for the transport's failure
    paths and for its release. See Follow-ups, "T6c inputs from T6b".
  - [ ] T6d — Identity, persistence and the app (route: delegated writer; trigger: the long-session
    backstop). A stable client id
    (`ClientIdentity.kt`); the musician's name and channel kept across launches; the Android tests
    T5c-T5e left to T6 that need storage; connecting from the desk list to a session the screens
    observe. Ends with the end-to-end check against the user's own desk machine.
- [ ] T7 — UDP receive path and AVAudioEngine output (route: to be decided).
- [ ] T8 — Screens: Connect, Mix, Mixes, Settings (route: to be decided).

## Follow-ups

- **T6 inputs from T5c.** `MixEvent(reportedBy:)` maps `config` to nil, so the session layer
  sends `.configReceived` to the reducer itself. Two decoding divergences left on purpose, both
  about what T6 shows: a `mix_ack` with no number whose mix cannot be read still throws, where
  Android only counts it as acknowledged (unreachable while every `mix` is numbered, which T5e
  keeps; if the pill needs it, give `unreadableMixAck` an optional sequence); and an `error` whose
  `seq` is negative or not a number fails as a whole message, where Android shows the error and
  answers nothing (make its `seq` as lenient as the ack's). The "acknowledged" counting and
  "nothing sent back" assertions of `ControlChannelTest.kt` belong to T6's transport.
- **T6, T8 and T9 inputs from T5d.** Storing the musician's channel across launches is T6's
  (Android's `startingRestoresTheName…`, `aChannelTheServerDidNotAnnounceIsNotStored`,
  `aConfigWithoutTheRememberedChannelForgetsIt` in `MonitorViewModelTest.kt`). The amber strip
  while More of me lifts it is T8's (`MixScreenTextTest.theMusiciansOwnStripIsAmberOnlyWhileMoreOfMeLiftsIt`);
  so are the seven notices' words. The musician's channel surviving a recall is T9's (the More of me
  assertion of `recallingAMixAppliesItsValuesAndKeepsWhatTheServerAnnounced`).
- **T6 inputs from T5e.** The session layer routes every event through
  `MixSession.reducedNumberingSends(by:)` and sends `outgoingMix()` whenever `sync.latest` grew,
  which also covers the restore after a link, numbering itself. Every mix is numbered, the opening
  one too (seq 0, all five lists as empty arrays before the first `config`, which the desk's
  `merge_levels` reads as changing nothing, `mix.rs:1003-1027`). Left for T6 by T5e, needing a
  socket, a queue or storage: Android's `aConfigArrivingBeforeTheOpeningMixIsSent…`,
  `anAnswerArrivingBeforeAQueuedChangeIsSentNeitherUndoesNorLosesIt`,
  `aReconnectStillSendsThePhonesOwnMix`,
  `aReconnectSendsTheMixAgainUnderItsNewestNumberAndItsAnswerSettlesIt`,
  `ControlChannelReconnectTest.everyOpenRegistersFirstAndThenSendsTheMix`,
  `anAckWithoutANumberOnlySaysTheServerHoldsTheMix`, and the storage tests listed under T5d.
- **T8 inputs from T6a's review** (lineage `review-94c7ed93bf2370fa`, approved; all four
  findings non-blocking and, in the review's own words, separate later work):
  `R4-no-recovery-after-terminal-status` (after `localNetworkDenied` or `unavailable`, coming back
  from Settings fires no `onAppear`, so the list stays stuck: restart on `scenePhase` and offer a
  retry); `R4-unbounded-searching-state` ("Searching for desks…" can last forever with no hint);
  `R3-repeated-start` and `R3-untested-run-guard` (`DeskDiscoveryModel.swift`: the run guard has
  no test in the repository; moving it into `BandEarKit`, where `swift test` reaches it, would close
  that).
- **T8 inputs from T6a.** Discovery is never restarted when the app comes back from the
  background or after the browser fails for good, because the root view's appear and disappear
  rarely fire: restart it on `scenePhase` (the verifier's smallest fix; not reproducible without a
  device). A sample rate shows as "48.000 Hz" under a Spanish locale, correct grouping but easy to
  misread as 48: say "48 kHz". The status lines are provisional. "Designed for iPad/iPhone" on Mac
  and visionOS is still on by default; whether to offer it there is a distribution choice for M4.
- **T6c inputs from T6b** (its verifier and its review, lineage `review-2d1cf4a399f54d0d`,
  approved, every finding non-blocking):
  - `R3-restart-sends-on-old-connection` and `R3-stop-reduces-after-close-while-connected`
    (`ControlSession.swift:88-89,105-108`): `start` reduces `.sessionStarted` while `connection` is
    still `.connected` to the previous desk, and `stop` reduces `.sessionEnded` after closing but
    while still `.connected`. Nothing is sent today, because neither event numbers a send, but the
    guard rests on that and on the transport's own ordering. Set `connection` to `.connecting` or
    `.idle` before reducing, and pin with tests that nothing reaches the old connection on a replace
    and nothing is sent on a stop from connected.
  - `R4-no-connect-timeout` (`WebSocketTransport.swift:99-105`), also the verifier's: an
    `NWConnection` to an unannounced Bonjour service stays preparing with no timeout (it completes
    on its own once the desk is announced again), and a typed host dropping SYNs waits for the
    system's TCP timeout or longer.
  - No dead-link detection: the phone only answers the desk's pings, so a Wi-Fi drop with no FIN
    leaves the session `connected`. Android pings every 5 s.
  - `R3-transport-failure-paths-unproved` (`WebSocketTransport.swift:115-125`): `.waiting` to
    `.failed`, a receive error, a stream ending with no opcode and the `hostPort` fallback have no
    test in any run. The verifier saw a refused port and an unknown host report `.failed` at once.
  - The verifier's mutant M7 (the connection handler holding the transport strongly) was caught
    only by its own probe; no test in this repository catches that retain cycle.
  - A Close frame right behind another message is reported as `.failed` ("Socket is not
    connected") rather than `.closed`. The desk never sends a Close frame (`control.rs:952` reads
    one, then drops the stream); mapping that receive error to `.closed` is the smallest fix if it
    matters for reconnecting.
  - `close()` cancels without waiting for the desk's close reply, so the desk saw a reset in 4 of
    10 rounds. Harmless; noted.
- **T8 input from T6b's verifier.** A typed `[::1]` becomes `[[::1]]` and fails; Android accepts the
  brackets. Strip them in the connection form.
- **From T6c's verifier and review** (lineage `review-4e409e696a4429d2`, approved, both findings
  non-blocking):
  - `R3-001`: the transport's connect deadline and ping clock (`WebSocketTransport.swift:197-256`)
    are exercised only by the opt-in round trip, so a default `swift test` would pass a regression
    in cancelling the deadline on ready or in the pong bookkeeping. Three independent looks (writer,
    verifier, review) named it. The fix is an injected clock on the transport, so the clocks run
    under a manual clock in the default suite; worth a small unit of its own after T6d.
  - `R3-002` (`ControlSession.swift:211-219`): a session that ends on its own through
    `.sessionEnded` (the path T7's audio receiver will take) goes idle but keeps the outage's
    `connectionError`. Clear it when the session ends. Done in T6d.
  - For T8: `connectionError` stays set while `connected` until the desk's first message (the "one
    outage" rule); do not show it as a current error.
- **T7 inputs from T6b: align the `playback` and `redundancy` decoders with Android.** The T2b
  decoder reads an unusable message (an unknown mode, `fixed` with no `ms`, `ms` outside 5-500, a
  delay outside 0-4) as Auto or clamps it; Android ignores such a message and keeps the setting it
  has. That reading was a guess made in T2b, the same kind T2c corrected for `config`. Parent's
  decision: ignore them as Android does, done in T7 before it announces `playback.buffer` and
  `redundancy.hold`, since only then can the desk send them. Six parity cases in
  `ControlSessionParityTests` are marked `withKnownIssue`, which fails the suite the day the
  decoder agrees, so the markers come off with the fix. Also for T7: `LinkStats` does not round the
  extras to a hundredth (Android does; one more known issue), and leaves `recovered` out when nil,
  where Android always sends it.
- **T8 input from T6b.** An unreadable desk message is ignored; Android shows "Invalid control
  response" (an `error` with no message, an unnumbered ack that cannot be read).
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
    reads an absent list as unchanged (Corrected in T5e: this note first claimed that sending a whole mix to move one fader "would
    fight every other socket of the same musician"; that was never checked, and the Android client
    sends all five lists on every `mix`, as this client now does.) `LinkStats` validates the interval and sanitises the
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
- T5d done (route: delegated writer; trigger: the long-session backstop). The parent fixed the
  shape in the handoff (`MoreOfMeNotice` with Android's seven cases, `MoreOfMeState`,
  `MoreOfMe.liftedLevels`, `moreOfMe` and `ownLevelsToRestore` on the session, two events,
  `mixAsPlayed` lifting, `reducedSending` clearing the restore) within eight named surfaces.
  - `MoreOfMe.swift`: the lift ports `MixSnapshot.moreOfMe` and `moreOfMeFor`: the musician's
    sides to 100, every other channel halved, locked sides as stored. Kotlin's `roundToInt` rounds
    a half toward positive infinity, so a halved level is `(Double(level) / 2).rounded(.up)`, exact
    for every `Int`; Swift's default `rounded()` agrees on 0...100 but not on negative halves, and a
    test pins Kotlin's rule there. The rules that turn More of me off are internal methods on
    `MoreOfMeState`.
  - `MixSession.swift`: `applyConfig` fits the musician's channel to the new strips and keeps the
    musician's own levels when a link turns More of me off; the engineer's lock or following turns
    it off, and following stopping clears its notice; a report that is not an echo of the mix as
    played turns it off with `mixChangedElsewhere`; after a link the musician's own levels go back
    (Android's `dropLinkedLift`) and the result is numbered when it differs from what the desk
    plays. The stored levels never carry the lift.
  - Ports: 19 from `MonitorReducerTest.kt`, the five `moreOfMe` tests of `MixSnapshotTest.kt` and
    `GroupSnapshotTest.moreOfMeKeepsTheGroupLevelsUntouched`; the deferred halves of four earlier
    ports completed in place. 14 Swift-only tests. Skipped: the `MonitorViewModelTest.kt` tests
    about what is sent (T5e or T6), storing the channel (T6), the amber strip (T8), recall (T9).
  - Divergences: `liftedLevels` returns the levels unchanged where Android returns null, and every
    Android caller then falls back to the stored mix, so behaviour is the same; the lift takes and
    returns levels only, so the Android checks that pans, masters and groups come through untouched
    are made on the session instead; Android's `sourceChannels <= 0` guard in `fittedTo` is left
    out because `applyConfig` already rejects such a `config`.
  - RED against the writer's stub: 130 mix tests, 38 failed with 64 expectations, no crash; five
    passed on the stub because they assert that nothing changes, and four `#require` checks were
    added so that tests built on a helper cannot pass against a reducer that does nothing. GREEN:
    130. Ten mutations in a scratch copy (Swift's own rounding, restoring without numbering,
    keeping the restore after a send, the engineer's locks in place of `lockedSides`, and more):
    all caught.
  - About 290 source and 620 test lines, over the ~400 heuristic, most of it the parity ports.
- Parent checks of the delegated unit: all seven changed files inside the surfaces; `swift test`
  rerun by the parent: 148, 130 and 29 passed; the writer's clean-copy build reported 0 warnings;
  its scratch inspected (the mutation copy's sources matched the repository's) and removed.
- Risk: none. Two public `MixEvent` cases added; the only consumers are this package's targets.
- Work-unit commit: `28d2b3f feat(ios): lift the musician's own channel with More of me`.
- Native review assessment against base 07a8711: `medium`, `review_due: false`, `under_budget`
  (40 paths, 7 601 lines).
- T5e done (route: delegated writer; trigger: the long-session backstop), closing T5. Within
  seven named surfaces, `ClientMessage.swift` limited to `MixCommand`'s doc comment.
  - `OutgoingMix.swift`: `outgoingMix()` numbers the mix `sync.latest` (never nil, the opening
    mix too), takes the master volume, the phone's own `levelCap` (not `effectiveCap`) and the mute,
    and the five lists from the mix as played, so More of me is lifted here and only here with the
    locked sides as stored. Every list goes out on every mix, as empty arrays before the first
    `config`. `sends(_:before:after:)` ports Android's four rules: an edit sends when the mix
    changed; a `config` when More of me went off for any reason but a link; a toggle when it
    changed; a channel choice when More of me was on and the choice was taken. Desk reports and
    session events never send through it. `reducedNumberingSends(by:)` is `reducedSending` with
    that rule.
  - `MixCommand`'s doc comment now keeps only the true part (an absent list leaves the desk's list
    unchanged) and says this client sends every list, as Android does; the matching claim in this
    document's T2 note is corrected in place, marked as corrected. The follow-up that tracked it is
    closed.
  - Ports: `aMixNeverCarriesTheEngineersControls` and `aMixCarriesTheNumberOfTheNewestChange…` from
    `ControlChannelTest.kt`; 23 send-rule tests from `MonitorViewModelTest.kt`, run through a
    socket-less harness (`Phone` in `Fixtures.swift`) that routes every event through
    `reducedNumberingSends` and records `outgoingMix()` whenever the number grew;
    `theMixBecomesTheSnapshotTheControlChannelSends` (649), never ported before; 914's snapshot
    check completed in place. 8 Swift-only tests. Skipped: the recall and saved-mix tests (T9), and
    those needing a socket, a queue, a reconnect, storage or the pill (T6, listed in Follow-ups).
  - Divergences: the session's number is never nil, so the "none without one" half of
    `aMixCarriesTheNumber…` is checked on a `MixCommand` built with no sequence;
    `aLimitOnlyReport…` checks `effectiveCap` where Android checks the receive thread's ceiling,
    which is copied from it.
  - RED: a first stub with no lists and no number made 13 tests fail by a thrown decode error
    rather than an expectation, so the stub became a constant mix with empty lists and seq 0; then
    164 mix tests, 31 failed with 64 expectations, no crash. Five passed on the stub because a stub
    that never sends satisfies them; `#require` checks were added to the ones that could pass
    vacuously, and one test's extra checks were added after RED without RED being rerun. GREEN: 164.
    Eleven mutations in a scratch copy, all caught. One of them ("a `config` never sends") was
    caught only by a Swift-only test: Android's suite never reaches a `config` that turns More of
    me off for a reason other than a link, which is the case for writing tests beyond the port.
  - About 100 source and 830 test lines, over the ~400 heuristic because of the 25 parity ports.
- Parent checks of the delegated unit: all seven changed files inside the surfaces, and only
  doc-comment lines changed in `ClientMessage.swift`; `swift test` rerun by the parent: 148, 164
  and 29 passed; the writer's clean-copy build reported 0 warnings; its scratch inspected (the
  mutation copy's sources matched the repository's) and removed.
- Risk: none. Public functions added to a module only this package uses, one property made
  internal, one doc comment edited.
- Work-unit commit: `b80bc49 feat(ios): build the outgoing mix and decide when the musician's
  action sends one`.
- Native review assessment against base 07a8711: `medium`, `review_due: false`, `under_budget`
  (43 paths, 8 550 lines).
- T5 complete: the mix module holds the channel layout, edits and `config`, reconciliation with
  the desk, More of me, and the outgoing mix. 341 tests: 148 protocol, 164 mix, 29 discovery.
- 2026-10-21, user's decision on how the Xcode project is defined: "`.xcodeproj` versionado
  (Recommended)", the committed project with synchronized folders, over XcodeGen and Tuist.
  Verified before it was offered: this Xcode's `Xcode3Core` knows
  `PBXFileSystemSynchronizedRootGroup`, and its own new-app prototype (334 lines) lists only the
  `.app` product, no source file. T6 sliced into T6a-T6c (see Tasks).
- T6a done (route: delegated writer; trigger: the long-session backstop), within seven named
  surfaces. What landed: `apps/ios/BandEar/BandEar.xcodeproj` built on Xcode's own app prototype
  (new object IDs, `MyApp` renamed, its placeholder `PROJECT_UNIQUE_VALUE` dropped, `SWIFT_VERSION`
  6.0), the synchronized `BandEar/` folder, `com.bandear.ios`, iOS 16, the local package
  `../BandEarKit` with its three products, and a shared `BandEar` scheme in Xcode's own template
  format. `NSBonjourServices` cannot be set from a build setting (`NSLocalNetworkUsageDescription`
  can), so `BandEar/Info.plist` holds both and is merged through `INFOPLIST_FILE`, kept out of the
  copied resources by a synchronized-group membership exception. The shell:
  `DeskDiscoveryModel` (`@MainActor ObservableObject`, callbacks hopped to the main queue, a run
  counter dropping reports from a stopped or replaced run, `stop()` clearing to idle, `deinit`
  stopping the browser) and `DeskListView`.
  - Parent change before commit: the prototype is multi-platform; the target is now iOS only
    (`SUPPORTED_PLATFORMS = "iphoneos iphonesimulator"`, `TARGETED_DEVICE_FAMILY = "1,2"`,
    `SDKROOT = iphoneos`, the macOS-only sandbox settings removed), as the PRD asks. Rebuilt by the
    parent: BUILD SUCCEEDED, no warning under `apps/ios/`, the built `Info.plist` showing
    `com.bandear.ios`, `MinimumOSVersion` 16.0, `iPhoneSimulator` only, both network keys.
  - Test-first exception: the shell holds no logic of its own (discovery is tested in
    `BandEarKit`), so there is no meaningful unit RED; its proof is the build, the built
    `Info.plist`, a fresh-copy build, the independent verifier and the simulator run below.
  - Risk: items 4 and 6, named by the writer and agreed by the parent (async ordering in
    `DeskDiscoveryModel`; no automated test covers the shell). An independent verifier ran: all
    seven claims held, no blocker. It drove the unchanged model against a live `NWListener`
    announcement from a scratch package, blocking the main thread so the browser's real reports
    queued up before `stop()` or a second `start()`, and ran three mutants (no run check, no stop
    in `deinit`, callbacks holding the model strongly), each caught. It left the workspace
    untouched (`find -newer`). Its advisories are under Follow-ups.
  - Proof of life on an iPhone 17 Pro simulator (iOS 27): the app showed "Searching for desks…";
    a desk announced from the Mac with `dns-sd -R "BandEar on SIM-TEST" _bandear._tcp local 50001
    v=1 sr=48000` appeared with its rate; stopping the announcement removed it on its own and the
    app went back to searching. T4's no-timer decision holds on iOS too. The announcement was
    visible on the local network for a few seconds.
  - Work-unit commit: `b577761 feat(ios): add the Xcode project and a shell that lists Bonjour
    desks`.
  - Native review. Assessed with `--escalate-item 4`: against 07a8711 the whole branch came back
    `high` and due (50 paths, 9 224 lines); the high-risk unit is reviewed alone, so it was
    assessed against its parent 098577a: `high`, `review_due: true` (7 paths, 600 lines). The
    preflight first asked for the untracked `.atl/` files to be declared (excluded, schema
    `gentle-ai.review-intended-untracked-selection/v1` with `untracked_scope`,
    `intended_untracked`, `expected_untracked_inventory`). The STATUS answering that submission,
    run with only its own tokens, offered a START against 07a8711 (tree 95e8de7, the whole
    branch); it was not run. Rerun with the original binding (`--base-ref=098577a
    --committed-only=true`), the START targeted 825320c..46ba67f, T6a alone, lenses
    `review-resilience` and `review-reliability`. Consent relayed: the user chose "Revisar este
    cambio" (`granted`). Lineage `review-94c7ed93bf2370fa` created. Both in-process reviewer
    captures then failed before starting (`invalid_request`, `mutation_outcome: not_started`,
    `retry_safe: true`): the reviewer runs through the `claude` CLI, whose OAuth session had
    expired and could not be refreshed. The bound STATUS still offers both slots; they are
    relaunched once the user signs in again (`claude auth login`).
  - The user signed in again (`claude auth login`). The bound STATUS still offered both slots;
    relaunched concurrently in provider order, both captures completed in 33 s and the review
    closed `approved` with four non-blocking findings (under Follow-ups, T8). Acknowledged with
    the provider's exact `review.acknowledge-approved` invocation: `gentle-ai.review-acknowledged/v1`,
    `action: acknowledged`, `authority: burned`. The reviewed boundary is now `b577761`;
    assessments from here on use it as `--base-ref`. The slice before T6a (07a8711..098577a, T1 to
    T5) was assessed `medium` and `under_budget` throughout and was never reviewed.
- 2026-10-21, user's decision on how T6 is checked against a real desk: "Tu máquina del desk
  (Recommended)": the user starts the desk on their own machine on this network and the simulator
  connects over the LAN; nothing is downloaded or built on this Mac. The alternatives were building
  the desk here (no crates cached, so a full dependency download, and the macOS microphone prompt,
  since the desk has no demo mode and captures a real device) or the fake server alone. The check
  needs the app wiring, so it moved to the end of T6c, and T6b stays inside `BandEarKit`.
- T6b done (route: delegated writer; trigger: the long-session backstop), within the three named
  surfaces. What landed in a new `BandEarSession` module (388 source lines):
  - `ControlTransport` and `TransportEvent`, as specified; a fake in the tests.
  - `ControlSession` (`@MainActor`): `mixSession`, `connection` (idle, connecting, connected,
    disconnected, failed), `mixHeldByDesk` (Android's "acknowledged"), the desk's `playbackBuffer`
    and `redundancyHoldBlocks` kept for T7, `lastError`, `stateDidChange`; `start` (throws on a
    blank name; resets the buffer, the hold, the held flag and the last error, as Android's session
    start does; a second `start` replaces the session), `stop`, `apply`, `send(stats:)`. Transport
    events reach the main actor through `DispatchQueue.main.async`, which keeps them in order;
    every connection is numbered and events from one stopped, replaced, closed or failed are
    ignored, as Android ignores old sockets. `register` (no capabilities) then the opening mix on
    open; `mix` and `stats` dropped while not connected; a socket closing does not end the session.
  - `WebSocketTransport`: `NWConnection` with `autoReplyPing`, its state on its own queue, a
    normal-closure frame on `close()`. One decision of the parent's did not hold: a typed desk was
    to become `NWEndpoint.hostPort`, but Network's WebSocket client aborts with `ECONNABORTED`
    before any handshake when given an IP address that way (IPv4 or IPv6; a `Host` header does not
    help). A typed desk becomes `.url(ws://host:port)` instead, as Android connects
    (`ControlChannel.kt:246`); IPv6 in brackets; a host no URL can hold falls back to `.hostPort`.
    A Bonjour desk is `.service`, which Network resolves itself.
  - Ports: 21 Android tests (9 new from `ControlChannelTest.kt`; the transport halves T5c left
    for T6; two of `ControlChannelReconnectTest.kt` that need no reconnect; three of
    `MonitorViewModelTest.kt`), 19 Swift-only tests, 2 endpoint-mapping tests. Skipped for T6c:
    every reconnect and storage test, listed by the writer.
  - The opt-in round trip (`BANDEAR_WS_IT=1`, `WebSocketTransportIntegrationTests`): by address in
    0.7 s and over Bonjour in 1.6 s, the phone received the desk's `config`, the desk heard the
    phone, three pings brought three pongs with the connection open, and closes from either side
    were reported once. It announced `BandEar WS IT <8 hex>` for about 1.6 s.
  - RED against the writer's stub: 39 of 40 failed with 66 issues, no crash; the one that passed
    gained a `#require` and then failed on the stub. GREEN: all, with 7 known issues (the
    divergences under Follow-ups). 14 mutations in a scratch copy, all caught; one only after
    `aDeskReportThatNumbersAChangeSendsIt` was added for it.
  - About 390 source and 1 300 test lines, over the ~400 heuristic, mostly the parity ports and a
    WebSocket desk built inside the test process.
- Parent checks: all eight changed paths inside the surfaces, `Package.swift` adding only the
  product and two targets; `swift test` rerun by the parent: 43 session (7 known issues), 148,
  164, 29; the writer's clean-copy build reported 0 warnings and the app still built; scratch
  inspected and removed.
- Work-unit commit: `fab5daf feat(ios): add the control session that speaks to the desk over a
  WebSocket`.
- Risk: items 4 and 6, named by the writer and agreed (the hop to the main queue and the guard
  against old connections; `WebSocketTransport` covered only by the opt-in round trip). An
  independent verifier was launched.
  - Independent verifier: no blocker. Claims 1-5 and 7 held; 6 held for everything the desk sends
    (the Close-frame case above is the one exception). It reproduced the `ECONNABORTED` finding on
    its own (with `.hostPort` and an IP literal the desk accepts TCP and receives zero handshake
    bytes), confirmed 491 events kept their order through the hop, the mix numbers 1, 2, 4, 5 with
    3-4 dropped while down and the next open sending the newest, pongs echoing the desk's payload
    and the phone surviving 12.5 s of a desk that drops silent clients after 10, and the session
    and transport released after `stop()`. Ten mutants, all caught; M7 only by its probe. Workspace
    untouched (`find -newer`).
  - Native review: assessed with `--escalate-item 4` against the reviewed boundary `b577761`:
    `high`, due (9 paths, 1 855 lines). START checked before it ran (base tree `46ba67f` =
    `b577761`, candidate `8418926` = `HEAD`). Consent relayed; the user chose "Revisar este
    cambio" (`granted`). Both reviewers completed in 55 s; `approved` with four non-blocking
    findings (under Follow-ups, T6c). Acknowledged: `authority: burned`, lineage
    `review-2d1cf4a399f54d0d`. The reviewed boundary is now `a0b94d8`.
- T6 resliced: the robustness work the verifier and the review handed over is T6c, inside the
  package and checked by `swift test`; identity, persistence, the app and the real-desk check are
  T6d.
- T6c done (route: delegated writer). `ReconnectPolicy` (0.5, 1, 2, 4 s, then 5 s; reset on the
  desk's first message), an injected `ControlScheduler` (`MainQueueScheduler`; `ManualScheduler`
  in tests), `ConnectionState` now idle, connecting, connected, reconnecting (`.disconnected` and
  `.failed` removed: a running session always reconnects), `connectionError` once per outage,
  `WebSocketTransport(connectTimeout: 10 s, pingInterval: 5 s)` failing a connection not open by
  the deadline or whose ping's pong has not come back by the next one, and `connection` set before
  `.sessionStarted`/`.sessionEnded` are reduced. Ports: all 12 of `ControlChannelReconnectTest.kt`,
  5 of `ReconnectPolicyTest`, both `MonitorViewModelTest` reconnect tests; `ConnectionGenerationsTest`
  covered through the session. RED 32 of 86 failing; GREEN 86 (7 known issues), 148, 164, 29;
  opt-in round trip 17/17, six repeated runs. 21 mutations, 20 caught including the verifier's M7
  retain cycle; the ordering fix's mutant is not catchable while neither session event can send.
  Parent checks: all eleven paths inside the surfaces, `swift test` rerun, scratch removed.
  Work-unit commit recorded in git log after `777d23e`.
  - Risk: items 4 and 6, named by the writer (retry scheduling, stale-connection guards, the
    transport's queue clocks; the clocks covered only by the opt-in round trip). PENDING: the
    independent verifier and the native review (assess with `--escalate-item 4` against `a0b94d8`)
    were not run, because the session hit its usage limit.
- T6c checks, completed in a later session after the usage limit:
  - Independent verifier: all nine claims held, no blocker. A real in-process desk restarted on the
    same port (`allowLocalEndpointReuse`) was noticed in 0.006 s and the phone was back 0.30 s after
    the restart, re-registered with the newest mix number; the backoff timeline matched Android's
    across failures, an open with no message and a first message; a fuzz of 40 seeds by 400 steps
    found no ordering violation; nine mutants caught. It confirmed the writer's claim from the code:
    the mix number only grows on a sent change (`MixSession.swift:76`) and on restoring the
    musician's own levels (`:229`, reached only from a desk report), so `.sessionStarted` and
    `.sessionEnded` can never send, and the decision-4 reordering is defence in depth no test can
    observe today.
  - Native review: `--escalate-item 4` against `a0b94d8`, `high`, due (12 paths, 1 595 lines);
    START checked (base `8418926` = `a0b94d8`, candidate `b10869b` = `HEAD`); the user chose
    "Revisar este cambio" (`granted`); approved in 49 s with two non-blocking findings (above);
    acknowledged, `authority: burned`, lineage `review-4e409e696a4429d2`. The reviewed boundary is
    now `3540b25`.
- Pending: T6d (delegated), T7, T8, T9.
