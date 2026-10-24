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
- [x] T8e — The T8d review's four warnings, which the user asked for on 2026-10-23: "dale, arreglá
  primero los warnings de T8". Route: one delegated writer, because the parent context is past the
  long-session backstop. The review's suggestions stay under Follow-ups.
  - R4-mirror-republish-per-change: a receiver report must not rebuild the Mix screen.
    - The per-packet figures (receiver stats, link report, underrun total, receiver error,
      `outputPlaying`) move to a model of their own that only Settings and Diagnostics observe.
    - `mirror()` assigns a published value only when it changed.
  - R2-duplicate-tone-mapping: one tone, colour, symbol, pill and banner style for every screen. The
    Mix header uses the shared `ConnectionToneStyle`, `ConnectionToneDot` and `ConnectionBannerRow`;
    its copies (`MixTone`, `MixBannerRow`, `MixConnectionPill`) go.
  - R3-refused-start-restore-unpublished: `MonitorAudio.start` reports a change after it puts the
    last figures back on a refused start, with a test.
  - R3-discovery-double-start: discovery starts once per return to the foreground, never twice at
    launch.
- [x] T9 — Mix banks and recall. Device-local saved mixes (`MixBankStore.kt`) and recalling one
  into the mix. Split from T5 because it needs device storage, which the pure mix module does not
  have. The user asked "dale, seguí con T9" (2026-10-23). The look follows T8's decision ("Nativo
  iOS, mismos textos y reglas").
  - Sources:
    - the T8 map of Android's UI (Progress, "T8 inputs");
    - `MixBankStore.kt` and `MixSnapshot.kt` (with `MixSnapshotTest.kt` and `GroupSnapshotTest.kt`);
    - the recall rules in `MonitorReducer.kt:404-419` and their reducer tests;
    - the saved-mix actions in `MonitorViewModel.kt:433-478`;
    - the `MonitorViewModelTest` cases `savingAndRecallingAMix`,
      `savingUnderAnExistingNameReplacesThatMix`, `savingNeedsAName`,
      `recallingAMixThatIsGoneRefreshesTheList`, `renamingAndDeletingAMix` and
      `recallingWhileFollowingSendsNothingAndSaysWhy`;
    - the `SavedMixesScreen.kt` copy.
  - [x] T9a — The domain (route: one delegated writer, past the long-session backstop).
    - In `BandEarMix`:
      - the snapshot of a mix;
      - the list of saved mixes with Android's rules (save under a name, a name already taken
        replaces that mix, rename, delete, which one is active);
      - recall as a `MixEvent`, reduced as Android does: locked sides kept, refused while
        following, strips sized from the saved mix until the next `config`, and the musician's
        channel surviving a recall.
    - In `BandEarSession`:
      - a store port;
      - a `UserDefaults` adapter that keeps the saved mixes as JSON and reads them back as
        tolerantly as Android does.
    - It ports Android's snapshot, bank and recall tests, including the recall cases deferred in
      `MixReducerParityTests`.
  - [x] T9b — The coordinator and the Mixes screen (route: one delegated writer, after T9a). It
    covers:
    - save, recall, rename and delete through the session, with the store's results as Android's
      notices in `BandEarScreens`, and the active mix tracked;
    - a recall sent to the desk by the existing send rule;
    - the Mixes screen in native SwiftUI: the save field, the list with "Active now" and Recall,
      Rename and Delete under the ⋮ menu, their dialogs, and the notices;
    - ports of the six `MonitorViewModelTest` cases.
  - [x] T9c — The T9b review's three warnings, which the user asked for on 2026-10-23: "dale,
    arreglá primero los warnings de T9". Route: one delegated writer, past the long-session
    backstop.
    - R3-delete-dialog-state-not-cleared: confirming a delete clears the pending delete itself, so
      deleting "Verse" and then saving "Verse" again never shows the dialog.
    - R3-repeated-notice-not-announced: every action that gives an outcome is announced to
      VoiceOver, even when its words equal the last notice's (a second blank save, a re-save).
    - R2-init-doc-misleading: `SavedMixes.init`'s comment says what `init` does (it builds the bank
      from the store and applies nothing). The adjacent suggestion is fixed too: `notRecalled`'s
      comment names its real label, `because`.
    - T9a's unclamped recall values and the views' missing tests stay under Follow-ups.
- [x] T6 — App shell, project file and control session. Grown since first written: the follow-ups
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
  - [x] T6d — Identity, persistence and the app (route: delegated writer; trigger: the long-session
    backstop). A stable client id
    (`ClientIdentity.kt`); the musician's name and channel kept across launches; the Android tests
    T5c-T5e left to T6 that need storage; connecting from the desk list to a session the screens
    observe. Ends with the end-to-end check against the user's own desk machine.
- [x] T7 — UDP receive path and AVAudioEngine output. Sliced from a read-only map of Android's audio
  path (`PmonPlayer.kt`, about 470 lines with no test of its own, plus small pure helpers that are
  tested):
  - [x] T7a — Decoders and `stats` agree with Android (route: inline). A `playback` or
    `redundancy` message the phone cannot use is refused (`ServerMessageError.unusableField`, or
    `missingField("ms")` for a fixed buffer with no length), so the session keeps its setting.
    `LinkStats` rounds each extra to 0.01 after its range check. All 7 known issues are gone.
    Commit `7556156`.
  - [x] T7b — A `BandEarPlayback` module that depends on `BandEarProtocol`. `ReorderHold` and
    `CushionController` move there with their tests (route: inline, mechanical). Commit `c2e9bc5`.
  - [x] T7c — Ports of `HoldPlayout` and `LinkStatsAccumulator`, with Android's tests (route:
    delegated writer A, launched together with T7d: two independent units with disjoint surfaces,
    each over 100 lines). Commit `06eb2b0`.
  - [x] T7d — Ports of `Pcm16Processor` (local protection), the pure `PlaybackLatency` measures,
    and the arrival spacing and jitter estimate from `PmonPlayer.kt:222-233` (route: delegated
    writer B, launched together with T7c). Commit `0aa3674`.
  - [x] T7e — The receive engine and output, split at a seam the parent wrote first:
    `PlaybackSink` and `PlaybackSinkFactory` (`Sources/BandEarPlayback/PlaybackSink.swift`), this
    client's stand-in for Android's `AudioTrack`. Two delegated writers are launched together,
    each in its own worktree; each unit is over 100 lines with disjoint surfaces. Both touch
    concurrency (high-risk item 4), so each gets an independent verifier and a native review.
    - [x] T7e1 — Output: a single-producer, single-consumer ring of PCM16 frames in a small C
      target with C11 atomics, and a `RingPlaybackSink` conforming to `PlaybackSink`.
      - Its render side is a pure function: it fills two `Float` channels, needs no lock and
        allocates nothing.
      - It plays silence until the target is queued, and starts over after it runs dry, counting
        an underrun.
      - The app's `AVAudioSourceNode` will call it in T7f.
      - Commit `67d35df`.
    - [x] T7e2 — Receive: a UDP socket with `SO_RCVTIMEO` 250 ms on its own thread, and the
      receive loop ported from `PmonPlayer.kt:113-357`.
      - It is written against the protocol, with a fake sink, a fake datagram source and an
        injected clock.
      - It also ports `ReceiverLaunches`.
      - Commit `88408f6`.
  - [x] T7f — Wiring, in three units:
    - [x] T7f1 — Session side in `BandEarKit` (route: delegated writer, launched together with
      T7f2):
      - `LinkStatsReport` converts to `LinkStats`, with Android's JSON check from
        `HoldPlayoutTest.kt:113-114`.
      - Registration announces the four capabilities.
      - A coordinator in `BandEarSession` owns `PlayerControls` and an `AudioReceiving` port.
      - It copies into the controls the session's buffer, hold, desk-owns-the-mix flag, volume, mute
        and cap. The cap is the lower of the musician's cap and the engineer's limit.
      - It forwards each link report to `send(stats:)`, and keeps the latest receiver stats for T8.
      - It starts and stops the receiver with the session, and refuses to start while the previous
        receiver is alive (A3).
      - It includes ports of the `MonitorViewModelTest` cases for these rules.
    - [x] T7f2 — App-side output (route: delegated writer, launched together with T7f1):
      - A `PlaybackSinkFactory` on `AVAudioEngine`, with an `AVAudioSourceNode` whose render block
        calls `RingPlaybackSink.render`.
      - The sample rate is bounded before the ring is sized (A5).
      - The sink stays strongly held until the engine stops (A4).
      - `outputLatencyFrames` comes from `AVAudioSession`, and the category is `.playback`.
    - [x] T7f3 — `SessionModel` joins the two, then the check by ear against the desk (route:
      inline).
    The original scope of T7f:
    - announce the four capabilities;
    - hand `playback` and `redundancy` to the player;
    - send `stats` through the session, with `recovered` always present as on Android;
    - apply local protection only while the desk has not acknowledged a mix;
    - set the cap to the lower of the musician's cap and the engineer's limit;
    - set up the app's audio session, and start and stop with the control session;
    - check it end to end by listening to the desk.
- [x] T7g — The output after an interruption never goes silent unnoticed. The user asked for it
  on 2026-10-23: "dale, arreglá primero los warnings de interrupción". It fixes the T7f review's
  R4-001 and R3-001 (quoted under Follow-ups). Route: one delegated writer, because the parent
  context is past the long-session backstop.
  - `OutputRecovery` in `BandEarPlayback`, a pure state machine tested with `swift test`, decides
    when the app's output restarts and whether it plays:
    - When an interruption begins, the output stops and is interrupted.
    - When an interruption ends asking to resume, it restarts.
    - When an interruption ends without asking to resume, it stays stopped, as Apple's guidance
      asks (another app took the audio). It restarts when the app becomes active again.
    - A configuration change stops it and restarts it, unless it is interrupted.
    - When the app becomes active, any interruption ends and a stopped output restarts. This rule
      was rewritten by the user's decision of 2026-10-23, logged below. A system that still holds
      the audio, such as a call in progress, refuses the restart, which then leaves the output
      stopped.
    - A restart that fails leaves it stopped, and the next of those events tries again.
  - `EngineOutput` follows `OutputRecovery`:
    - It reactivates the audio session (`setActive(true)`) before every restart.
    - It never drops a restart's error silently: a failure marks the output stopped.
    - It also listens for the app becoming active.
  - `PlaybackSink` gains `isPlaying` (default `true`). `ReceiverStats` gains `outputPlaying`,
    sampled from the sink with each packet. The session screen says when the output has stopped,
    instead of showing Connected over silence.
  - Out of scope: R3-002 (the session activated before the format) and R3-003 (the bounds' tests)
    stay under Follow-ups, since the user asked only for the interruption warnings.
- [x] T7h — The receive thread never waits for an output restart (route: inline, a small fix in
  one file). The user asked for it on 2026-10-23: "dale, arreglá el bloqueo del hilo de
  recepción". It fixes the T7g review's R4-001 and R3-001:
  - `EngineOutput` publishes `recovery.isPlaying` behind its own `OSAllocatedUnfairLock`, held
    only to copy one value.
  - The value is updated after every recovery change, including before a restart is tried.
  - `isPlaying` reads only that published value. The lock order is the output's lock, then the
    published one.
  - Commit recorded in Progress.
- [x] T8 — Screens: Connect, Mix, Mixes, Settings. The user asked "dale, seguí con T8" (2026-10-23).
  Asked about the look, the user chose "Nativo iOS, mismos textos y reglas (Recommended)": standard
  SwiftUI (`TabView`, `List`, `Slider`, sheets), with Android's content, its words verbatim and its
  rules. Android's own faders, theme, fonts and gesture maths are not ported.
  - Sources:
    - A read-only map of Android's UI (`ui/screens`, `ui/components`, `ui/state`). Every string is
      hard-coded in Kotlin.
    - The T8 inputs under Follow-ups.
  - Screens, as Android has them (`AppScreen.kt`, `MonitorApp.kt`):
    - Connect, while no session runs, with the manual address reached from it.
    - Then tabs: Mix, Mixes and Settings. Back goes to Mix, and a session start shows Mix.
    - The channel sheet opens from a strip.
    - Diagnostics live inside Settings.
  - The split with T9: T8 builds the Mixes tab's layout and empty state ("No saved mixes yet" /
    "Save the current mix above, then bring it back here with one tap.") and the pure text helpers
    of `SavedMixesText.kt`. Storage, the snapshot, recall and the notices driven by the store are
    T9's.
  - [x] T8a — Connection and diagnostics words, in a new pure module `BandEarScreens` (route:
    delegated writer, launched together with T8b and T8c). It covers:
    - the screen choice (`appScreenFor`);
    - the connection pill words;
    - the control status detail;
    - the reconnecting, lost and following banners;
    - the connection form's errors, and `previousReceiverStopping`;
    - Settings text;
    - link health (`phoneLinkHealth`, the words and their meanings);
    - loss, the diagnostics formats, and Connect's copy.

    It ports `AppScreenTest`, `ConnectionPillTest`, `SettingsTextTest`, `LinkHealthTest` and
    `PhoneLinkHealthTest`, plus the reducer's pill, banner and diagnostics cases.
  - [x] T8b — Mix words, in the same module (route: delegated writer, launched together with T8a
    and T8c). It covers:
    - strip and group text, and pan text with its 5% detent;
    - master, cap and the engineer's limit;
    - More of me's caption and its seven notices;
    - the channel sheet's text;
    - the amber strip rule;
    - the labels per channel kind;
    - the empty rows;
    - the `SavedMixesText.kt` helpers.

    It ports `MixScreenTextTest`, `ChannelSheetTextTest`, `PanScaleTest` (text and detent),
    `AtomLabelsTest` and `SavedMixesTextTest`.
  - [x] T8c — The session's gaps for the screens, in `BandEarSession` (route: delegated writer,
    launched together with T8a and T8b):
    - the name is stored as typed and re-announced to the desk after a typing pause, as Android
      does (`aNameChangeIsStoredAndAnnouncedAfterThePause`);
    - whether a session runs, made public;
    - the running underrun total per session, reset when a session starts, as Android does (its
      writer found `SessionEnded` only resets link health, `MonitorReducer.kt:70-81`);
    - the T7f verifier's AD1 (reports from a previous launch), AD2 (a late link report after a
      stop) and AD3 (the weak owner read on the receive thread).
  - [x] T8d — The SwiftUI screens on top, replacing `DeskListView` and `SessionView`:
    - [x] T8d1 — The seam (route: inline, by the parent). `SessionModel` publishes the session,
      the mix, the diagnostics, the tab, the manual form and the refusal. Its actions:
      `connect(to:)`, `connectManually()`, `disconnect()`, `setName(_:)`, `apply(_:)`. The app
      links `BandEarScreens`.
    - [x] T8d2 — Connect, Settings (with Diagnostics), the app shell and the Mixes tab (route:
      delegated writer, launched together with T8d3):
      - the shell: Connect while no session runs, tabs Mix, Mixes and Settings while one runs, and
        the manual address opened from Connect, all chosen by `AppScreen`;
      - Connect: the name, the desks found, the manual entry, and the refusal banner;
      - Settings: the server card, Disconnect, the name, the manual connection, and Diagnostics
        with its expander;
      - Mixes: its layout and empty state only (T9 adds the rest).
    - [x] T8d3 — Mix and the channel sheet (route: delegated writer, launched together with
      T8d2). It covers:
      - the header, the pill and the banners;
      - More of me and the channel picker;
      - channels and groups, each strip with a native `Slider`, mute and solo;
      - the master card with volume, mute, the level cap and the engineer's limit;
      - the channel sheet with pan (`PanScale` and its detent), level, mute, solo, and the locked
        and stereo notes.
    Both writers use only `SessionModel` and `BandEarScreens`, edit no package code, and never edit
    `SessionModel.swift`. A gap in the model goes back to the parent.
  - Route: T8a, T8b and T8c are independent units with disjoint surfaces, each over 100 lines.
    They run as three writers, each in its own worktree. The parent first adds the empty
    `BandEarScreens` module, so no writer edits `Package.swift`.

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
- **Desk-side findings from the first end-to-end run (server; out of this feature's scope, the
  user's call).** The desk's control channel listens on IPv4 only (`0.0.0.0`,
  `apps/server/src/control.rs:357`) while its Bonjour announcement carries IPv6 link-local
  addresses too, including the loopback's `fe80::1`, published on `en1` as well as `lo0`. A client
  that resolves the service where only IPv6 addresses exist reaches nothing; the iOS client now
  forces IPv4 (`4694c2e`), and listening dual-stack or announcing IPv4 only would fix it on the desk.
  Separately, with the desk running on macOS the Android discovery beacon fails on every send with
  `No route to host` (os error 65): a broadcast to `255.255.255.255` needs an interface on macOS,
  so an Android phone cannot find a macOS desk by beacon.
- **T8 inputs from T6d's verifier.** A refused `start` still saves a client id (the id is resolved
  as an argument before `Registration` throws; build the registration first). An empty name field
  stores the device's name, so the next launch shows it, where Android keeps what was typed. A
  session that ends on its own leaves `SessionView` on screen with the desk set (T7 or T8). A stored
  `Bool` or a whole `Double` reads as a channel through `NSNumber` bridging; the app never writes
  either.
- **(Done in T7a, except `recovered`, which is for T7f.) T7 inputs from T6b: align the `playback` and
  `redundancy` decoders with Android.** The T2b
  decoder reads an unusable message (an unknown mode, `fixed` with no `ms`, `ms` outside 5-500, a
  delay outside 0-4) as Auto or clamps it; Android ignores such a message and keeps the setting it
  has. That reading was a guess made in T2b, the same kind T2c corrected for `config`. Parent's
  decision: ignore them as Android does, done in T7 before it announces `playback.buffer` and
  `redundancy.hold`, since only then can the desk send them. Six parity cases in
  `ControlSessionParityTests` are marked `withKnownIssue`, which fails the suite the day the
  decoder agrees, so the markers come off with the fix. Also for T7: `LinkStats` does not round the
  extras to a hundredth (Android does; one more known issue), and leaves `recovered` out when nil,
  where Android always sends it.
- **Both clients count a refused packet as played.** The T7e review found it (R3-001/R4-001,
  warnings). `ReceiveLoop.play` counts the packet, its rate and the packets per second before the
  sink opens or the rate is checked. So a packet refused for a rate change, or because the sink
  failed to open, still counts in `ReceiverStats.packets` and `packetsPerSecond`, and its error
  also goes to `onError`. Android does the same (`PmonPlayer.kt:241-244`, before :257-260), so
  changing it is a decision for both clients, not a port fix. Only the diagnostics are affected.
- **A desk restarted mid-session stalls the phone's playback until it reconnects (T7f3, end to
  end).**
  - What happened: the restarted desk numbered its packets from 0 again, while the phone's reorder
    hold was past 16 000. The counter of played packets stopped, while the socket received the full
    stream.
  - A new session (Disconnect, Connect) played at once.
  - To do: check Android's `ReorderHold` for the same jump back. If both stall, it is a decision for
    both clients.
- **Desk-side findings from the T7f3 end-to-end check** (server, for the user to decide):
  - Its own mDNS publishes, on `lo0`, only IPv6 for its host name, so from the desk's Mac an IPv4
    connection resolved over `lo0` finds no address. The phone connects once its reconnect lands on
    Wi-Fi: up to about 40 s, or at once.
  - Its default `--target 127.0.0.1:50000` lands on the simulator's socket next to the registered
    stream, so the phone receives two numberings of the same audio. A real phone never sees the
    loopback target.
  - Capture from a Continuity microphone stops when the iPhone leaves: the desk then exits with
    `OSStatus 1937010544`.
- **T8 inputs from the T7f verifier.**
  - AD2: a link report that arrives after `stop()` sets `linkReport` again. Android does the same
    (`MonitorReducer.kt:177`), but the link health shown in T8 should ignore it.
  - AD1: reports still queued from the previous launch can be applied after a new start in the same
    main-queue turn. That is unreachable from the UI. Tagging events with their launch would close
    it.
  - AD3: `ReceiverEvents` reads its weak `owner` on the receive thread. Read inside the main-queue
    block instead, so a release at that moment cannot deinit off main. It is unreachable while
    `SessionModel` keeps the coordinator.
  - AD4: resuming after an interruption calls `engine.start()` without `setActive(true)`. Only a
    device test proves it.
- **The app's output after an interruption, from the T7f review (warnings, not fixed).**
  - R4-001 / R3-001: `restartIfStopped` calls `engine.start()` with `try?` and no `setActive(true)`
    first, and an interruption that ends without `.shouldResume` never restarts. Either way the
    output can stay silent for the rest of the session while the screen says Connected; only
    short writes in the stats show it. Reactivate the session before restarting, and report a
    failed restart (for example through the sink, so `onError` reaches the screen).
  - R3-002: `EngineOutput.init` activates the audio session before it builds the format. A format
    that cannot be built throws before `deinit` can run, which leaves the session active. Build the
    format first.
  - R3-003: the A5 bounds (8 000-192 000 Hz, 1-16 384 frames) have no test, because the app has no
    test target. Moving the check into `BandEarPlayback` as a pure function would let `swift test`
    pin it.
- **Gaps left by T7g** (named by its writer, its verifier and its review). An interruption that
  never ends, `.appWasSuspended` included, no longer leaves the output stuck: becoming active ends
  it (`3a469b8`).
  - (Fixed in T7h.) R4-001 / R3-001 of the T7g review: a restart runs `setActive(true)` and
    `engine.start()` under the output's lock, and the receive thread's per-packet `isPlaying` read
    took the same lock. While a restart ran (tens to hundreds of ms, longer when a call refuses
    it), the receive thread waited. Datagrams then queued in the socket and the arrival-timing
    stats inflated.
  - R4-002 (also the verifier's A3): a failed restart's error is dropped, so the screen cannot say
    why the output stopped (a call, a format, the background). Keep or log the last error.
  - R3-002: the app-side glue has no automated test, since the app has no test target. That glue is
    the mapping from interruption notifications to events and the `follow(_:)` contract.
  - A2 (verifier): a late `.appWasSuspended` beginning can mark the output stopped while it plays.
    Only a device test settles the order.
  - `init` starts the engine outside the lock, so a configuration change in that window could race
    a restart. This was already so before T7g.
  - T7h review (suggestions): the published flag starts as a literal `true` rather than a copy of
    `recovery.isPlaying`. Both start playing today, so they agree. Seeding it from the recovery, or
    publishing once at the end of `init`, keeps a single source if that default ever changes.
    R3-002 of the same review: the publish order (a stop is published before a restart, the result
    after) has only the verifier's simulator probe as proof. Moving that sequence into
    `BandEarPlayback` would let `swift test` pin it.
- **T8d inputs from the T8c verifier.**
  - A2: `ControlSession.start` reports a change before `MonitorAudio` resets its diagnostics, so
    the first change of a start still carries the previous session's underruns and error. A model
    that reads them inside `stateDidChange` shows them for one change. Reset the figures before
    `session.start` to close it. The T8c review found it again (`R3-start-observer-stale-diagnostics`),
    so T8d's integration fixes it.
  - T8c review, `R4-launch-tag-read-at-delivery`: the launch tag is right only while a receiver
    stays alive until its `onStopped` has returned. `AudioReceiver` honours that (verified), and
    `AudioReceiving`'s documentation should state it as the port's contract.
  - A3: Android's name field cuts only the text an edit inserts (`keptEdit`, `BandEarTextField.kt`).
    The session cuts the end of the whole name, which agrees for typing at the end but not for a
    paste inside a name. The field would need `keptEdit` to match exactly.
- **From the T8d verifier (advisories).**
  - The refusal order differs from Android: `SessionModel.start` checks the form before the
    previous receiver. The words differ only when both apply, during a stop that lasts
    milliseconds.
  - `mirror()` republishes every field on every change, the receiver's per-packet stats included,
    so the Mix list re-renders with each packet. This is performance only.
  - The `held` check (locked or following) is written in both the strip and the sheet, as on
    Android, with no helper in `BandEarScreens`.
  - A simulator app installed under a new bundle id can have its incoming UDP blocked by the macOS
    firewall ("Link silent"), while its control connection works.
- **From the T8d review. The four warnings are fixed in T8e; the suggestions are not.**
  - R4-mirror-republish-per-change (also the verifier's): `mirror()` assigns every published field
    on every change, so each receiver report rebuilds the whole Mix list. Assign only what changed,
    or move the per-packet figures out of the mix screen's observation.
  - R2-duplicate-tone-mapping: `MixTone`, `MixBannerRow` and `MixConnectionPill` copy
    `ConnectionToneStyle`, `ConnectionBannerRow` and `ConnectionToneDot` with different symbols, so
    one state looks different on Mix and on Connect or Settings. Keep one.
  - R3-refused-start-restore-unpublished: a refused start restores the last figures without calling
    `stateDidChange`. `ControlSession.start` throws before any change (the verifier's claim 5), so
    no observer sees the cleared values today. Calling it after the restore would make that
    structural.
  - R3-discovery-double-start: `RootView` starts discovery on appear and again when active unless it
    is browsing, so it can start twice at launch. `DeskDiscoveryModel.start` bumps its run and
    starts the browser again; whether that is harmless depends on `BonjourDeskBrowser.start`.
  - Suggestions:
    - `ChannelSheet` says it opens fully expanded but sets no detent;
    - `ChannelSheetKey` and `StripKey` are near copies;
    - the stale-figures tests do not check `linkReport`;
    - `SessionModel`'s start path has no automated test.
- **From T8e (its writer named these).**
  - A failed discovery browser is now retried only after a return from the background, and no
    longer after a brief interruption such as Control Center. The local network wait clears by
    itself once access is granted.
  - Once, coming back from the background, the app showed Mix instead of Settings, and a second try
    kept Settings. Nothing in T8e writes the tab. Watch whether the shell resets it.
  - The `linkReport` checks added to the stale-figures tests are weak guards: an ended session
    already clears `linkReport`.
- **From the T8e review (not fixed).**
  - Warnings, both already known:
    - R3-discovery-retry-narrowed: the verifier's A1. A failed browser now retries only after a
      background trip.
    - R3-app-models-untested: the app's change-only mirrors and the idempotent discovery have no
      automated test, because the app has no test target.
  - Suggestions:
    - `DiagnosticsSection`'s `diagnostics` property (a `DiagnosticsModel`) reads close to the
      `Diagnostics` type. A name such as `figures` would be clearer.
    - The assign-if-changed rule is written by hand per field in both mirrors. A helper would keep
      it in one place, but not an `inout` one: passing a `@Published` property `inout` always
      writes it back and publishes (the T8e writer measured it). A key-path assignment would work.
- **From the T9 verifier (advisories).**
  - The delete confirmation, a popover anchored to the ⋮, shows no Cancel button: tapping outside
    cancels. Android shows Cancel.
  - "New name" is the rename field's placeholder, so it is hidden while the field holds the old
    name. Android shows it as a label.
  - Stored keys are written sorted, where Android keeps insertion order. The JSON means the same.
  - A refused recall while following was not exercised end to end. Tests and mutants cover it.
- **From the T9 reviews. T9b's warnings, and the `because` suggestion, are fixed in T9c; the rest is not.**
  - T9a, R3-recall-unclamped-values / R4-recall-unclamped-stored-values: a recalled snapshot's
    volume, cap, group levels and channel values are applied and sent without clamping. The
    tolerant reader accepts any whole number. Only damaged or hand-edited storage reaches this.
    Android does not clamp either.
  - T9b, warnings:
    - R2-init-doc-misleading: `SavedMixes.init`'s comment says the stored mixes are recalled into
      the session; `init` only builds the bank.
    - R3-delete-dialog-state-not-cleared: the delete confirmation does not clear `deleting` itself.
      It relies on SwiftUI calling `isPresented`'s setter for a row the delete removes. If that
      does not happen, deleting "Verse" and then saving "Verse" again could show the dialog again.
    - R3-repeated-notice-not-announced: VoiceOver hears a notice only when its text changes, so a
      repeated outcome (a second blank save, a re-save) is not announced.
  - T9b, suggestions:
    - `SavedMixesNotice.notRecalled`'s comment names a `reason` parameter that is labelled
      `because`;
    - `MixesModel` and `MixesContent` have no automated test.
- **T8 input from T6b.** An unreadable desk message is ignored; Android shows "Invalid control
  response" (an `error` with no message, an unnumbered ack that cannot be read).
- **(Done in T7b.) `BandEarProtocol` holds more than the protocol.** `ReorderHold` and `CushionController` are
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
- T6, T8: route decided when reached.
- T7: T7a and T7b inline. T7c and T7d are delegated writers launched together: two independent
  units with disjoint surfaces in `BandEarPlayback`, each over 100 lines. T7e and T7f are decided
  when reached.

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
- T6d done (route: delegated writer). `MusicianStore` and `UserDefaultsMusicianStore`
  (`bandear.client_id`, `bandear.musician_name`, `bandear.musician_channel`), `ClientIdentity`
  (Android's `resolveClientId`, reusing `Registration`'s id rule), `ControlSession` taking the store
  (name and remembered channel restored at construction, the channel stored only when the desk
  announced it and forgotten when a `config` drops it, `R3-002` fixed), and the app's
  `SessionModel`, name field, desk-to-session navigation and provisional `SessionView`. 115 session
  tests; 14 of 14 mutations caught. The writer's early test runs left 64 empty
  `com.bandear.tests.<UUID>.plist` files in `~/Library/Preferences` (`removePersistentDomain` leaves
  the file); it switched to suites named by a path in a temp directory, and the parent removed the
  64 after checking each was 42 bytes and matched the pattern. Work-unit commit `8597da5`.
  - Independent verifier (item 6 named by the writer): all six claims held, no blocker; it drove the
    real `inSession` binding of `DeskListView` and the model against fakes, and ran ten mutants, all
    caught. It noticed the parent's uncommitted IPv4 change mid-run and verified `8597da5` without it.
- End-to-end against the real desk (the user's choice of their own desk machine; asked to start it,
  the parent built it on this Mac, which is that Mac mini: `cargo build -p personal-monitoring`, 24 s,
  run with no arguments). The desk captured "Microsoft Teams Audio" (one channel, 48 kHz) and
  announced "BandEar on Carloss-Mac-mini". On an iPhone 17 Pro simulator the app listed it with its
  rate; the first connect showed "Reconnecting… not open after 10.0 seconds". Diagnosis: a manual
  handshake to `192.168.1.54:50001` returned `101 Switching Protocols` and the desk's real `config`,
  while both IPv6 link-local addresses were refused; the service had resolved over `lo0`, where the
  announcement holds IPv6 only. Correction to what the parent first concluded: the old build was not
  stuck for good, since the desk's log shows its retry registered over IPv4 at 18:23:07; the first
  attempt simply lost 10 s. Fix (`4694c2e`, inline): `WebSocketTransport` requires IPv4; RED/GREEN on
  a structural test of its parameters (`ip.version` `.any`, then `.v4`); the 18 opt-in round trips
  still pass. Rebuilt and relaunched: the desk logged `control registration from
  192.168.1.54:49934: 192.168.1.54:50000 as "iPhone 17 Pro"` about a second after the tap, the app
  showed Connected with 1 strip (the desk's one channel), and the name stored by the previous run
  was restored on relaunch. Disconnect ended the connection with no error on the desk, where the
  connections killed by relaunching logged a reset without a closing handshake.
- Native review of T6d and the IPv4 fix: `--escalate-item 6` against `3540b25`, `high`, due (19
  paths, 958 lines); START checked (base `b10869b` = `3540b25`, candidate `d3ce3de` = `HEAD`); the
  user chose "Revisar este cambio" (`granted`); approved in 45 s. Its four non-blocking findings are
  the verifier's advisories found again independently (a refused start saving an id, the device
  name stored for an empty field, `NSNumber` bridging), already under Follow-ups for T8.
  Acknowledged; the reviewed boundary is now `2e151e7`.
- T6 complete. Pending: T7, T8, T9, and the small unit for `R3-001` (an injected clock on the
  transport, so its deadline and pings run in the default suite).
- T7 started. The user chose "T7: el audio". A read-only explorer mapped Android's audio path. What
  the T7 units must keep in mind:
  - Android takes datagrams from any sender (no `connect`, no source filter).
  - The 250 ms socket timeout is the only clock for link reports and hold releases.
  - `WRITE_BLOCKING` paces the loop.
  - A sample-rate change drops every later packet with an error and never reopens the track.
  - The `stats` jitter is the mean absolute deviation, per interval, from an EWMA-0.1 arrival
    spacing (not RFC 3550's 1/16).
  - Local gain is linear with no ramp, and applies only while `mixAcknowledged` is false.
  - The cap is `min(levelCap, engineer limit)`.
  - `ClientMixState`'s per-channel fields are never read by the player.

  Parent's technical choices for T7e, to confirm when it is reached:
  - A BSD socket with `SO_RCVTIMEO`, to keep Android's loop and clock.
  - The ring's indices in a small C target with C11 atomics, because iOS 16 has no
    `Synchronization.Atomic` and a package dependency would be a delivery risk.
  - The source node declares the desk's format and lets the engine convert it to the hardware
    rate.
- T7a (inline, test-first):
  - RED: the 7 former known issues, plus the new refusal and rounding cases (25 issues across 3
    suites).
  - GREEN: 461 tests with 0 known issues. A clean copy builds with 0 warnings.
  - `-0.004` and `60_000.004` were already dropped and stay dropped. They pin the order: the range
    check runs on the measured value, before rounding.
  - Commit `7556156`. Assessed against `2e151e7` with `.atl/` excluded: `medium`, `under_budget`.
    It stays pending in the slice, and the boundary stays `2e151e7`.
- T7b (inline): `git mv` of the two sources and four test files; `CushionController` imports
  `BandEarProtocol` for `PlaybackBufferSetting`. 461 tests, the same as before the move (protocol
  152 → 88, playback 64). 0 warnings on a clean copy. Commit `c2e9bc5`.
- T7c and T7d ran as two delegated writers, launched together, each in its own worktree. The
  worktrees kept the shared module building for each writer, since a half-written file from one
  would break the other's build. Both worktrees were created from `main`, so each writer first
  moved its own throwaway branch to `bf10548`. A writer with worktree isolation must check its base
  before it writes.
- T7c, writer A: `HoldPlayout`, `LinkStatsAccumulator` and `LinkStatsReport`.
  - RED: 29 of 32 tests failed against stubs. The other 3 hold on an empty stub: the two "no report
    yet" cases and the constants.
  - GREEN: 32 tests, 493 in the full suite. 0 warnings on a clean copy.
  - Mutants: 13 of 14 caught. The 1 ms lower clamp on the interval can never be reached, because a
    report is due only after 1000 ms. It is kept to match Android line for line.
  - Two mutants are caught only by Swift tests Android lacks: the floor at 0 on underruns, and the
    playout passing on its underruns and extras.
  - Divergences:
    - `nil` instead of NaN.
    - The playout owns its accumulator and adds `onArrivalDeviation`.
    - The interval limit is read from `LinkStats.maximumIntervalMilliseconds`.
    - Android's first `HoldPlayoutTest` case also checks the JSON its report becomes. That needs
      the conversion to `LinkStats`, so it waits for T7f (`HoldPlayoutTest.kt:113-114`).
  - Parent's correction: it was first a struct holding the `ReorderHold` class, so a copy would
    have shared the hold but split the counts. It is now a `final class`, like `ReorderHold` and
    `CushionController`.
- T7d, writer B: `Pcm16Processor`, `PlaybackLatency` and `ArrivalTiming`.
  - RED: 27 of 32 tests failed against stubs.
  - GREEN: 34 tests after the correction below, 495 in the full suite. 0 warnings.
  - 8 of 8 mutants caught, plus 4 on the clock change.
  - Local protection works in place on an `UnsafeMutableBufferPointer<Int16>` (or an `inout [Int16]`
    overload), so no packet allocates. Android returns a new array.
  - Halves round up, as with Kotlin's `roundToInt`, using `(x + 0.5).rounded(.down)`. A probe
    matched the JDK's `Math.round` on all 6,619,136 combinations of sample and whole-percent
    volume.
  - Reused from `CushionController.swift`: `capacityFrames` (Android's `trackCapacityFrames`) and
    `next` (`nextCushion`).
  - Not ported: `keepsLowLatency`, which only means something for an `AudioTrack`.
  - `reportExtras` takes `presentedFrame: Int64?`, where Android takes a flag and a frame.
  - Parent's correction: `ArrivalTiming` takes `nowNanoseconds: UInt64`, because Android times
    arrivals with `System.nanoTime()` (`PmonPlayer.kt:222`). A millisecond clock added about
    0.44 ms of false jitter at 5.33 ms spacing.
  - The interval is `Int64(bitPattern: now &- previous)`. A clock that goes backwards yields a
    negative interval, as Android's `Long` does, instead of trapping on the receive thread.
- Parent's integration:
  - Every path was inside its writer's surfaces, and the copies were byte-identical.
  - Combined suite: 527 tests (playback 130), 0 known issues, 0 warnings on a clean copy. No name
    collided across the two units.
  - The worktrees, their branches and both scratch folders were removed.
  - The slice was assessed against `2e151e7` with `.atl/` excluded: `medium`, `under_budget` (33
    paths). The boundary stays `2e151e7`.
- The desk always sends stereo: `OUTPUT_CHANNELS = 2` (`apps/server/src/mix.rs:11`).
- T7e: two delegated writers launched together, each in its own worktree, against the seam
  `9c208e9`. Both checked their base first (`git merge --ff-only feat/ios-client`, then
  `9c208e9`).
- T7e2 (receive), writer:
  - RED: 55 of 57 tests failed against stubs.
  - GREEN: 61 tests after the correction below.
  - Thread Sanitizer clean; 0 warnings; 17 of 17 mutants caught.
  - Finding: on Darwin, `shutdown(SHUT_RDWR)` on an unconnected UDP socket fails with `ENOTCONN`
    and does not wake a blocked `recv`. Closing the socket from another thread does not wake it
    reliably either. So a stop took up to one 250 ms timeout.
  - Parent's correction: T7f restarts the receiver on the same port when it reconnects, and a
    socket still bound would make the new `bind` fail with `EADDRINUSE`. Now:
    - `stop` lowers `running`, then `wake()` sends one empty datagram to `127.0.0.1` on the bound
      port. It holds the source's lock and does nothing once the source is closed, so a late
      wake-up cannot reach a newer socket bound to the same port.
    - The loop leaves before parsing once `running` is down.
    - The descriptor is still closed on the loop thread.
    - Tests: a stop takes under 100 ms; `onStopped` is reported once, with no error; a restart on
      the same port within 100 ms binds and plays.
  - Divergences from Android:
    - The controls are one locked snapshot read per wake-up. Android reads each value as it uses
      it.
    - `serverOwnsMix` is one of the controls.
    - The sink opens at the packet's rate, with no 48 kHz fallback.
    - A stat not yet measured is `nil`. The backlog is seeded once.
    - `onError` takes `any Error`.
    - `EINTR` is retried.
    - The loop closes the sink, then the socket, then reports.
    - A change of sample rate errors for every later packet and never reopens the sink, as on
      Android.
  - `AudioDatagram` allocates its samples per packet. That is acceptable on the receive thread,
    which is not real time.
- T7e1 (output), writer:
  - RED: 20 of 20 tests failed against stubs. GREEN: 20.
  - 13 of 13 mutants caught, the three memory-ordering mutants by Thread Sanitizer.
  - Finding: in a debug build, `memcpy` becomes the fortified `__memcpy_chk`, which Thread
    Sanitizer does not see. A relaxed publish would pass unnoticed, so the ring copies sample by
    sample. `Thread.sleep` creates a happens-before edge under TSan, so race probes must spin.
  - Design: C provides only write, queued, peek and consume; the state machine and the Float
    conversion are in Swift. The render reads straight from the ring.
    - Frames consumed is the read position.
    - `outputLatencyFrames` is atomic.
    - `close()` keeps the storage until `deinit`.
    - The first callback that finds the target queued plays it at once.
    - A capacity of 0 or less becomes 1.
- Inputs for T7f:
  - `AudioDatagram.sampleRate` is a `UInt32` with no upper bound, and the ring's capacity grows
    with it. The factory must check the rate before sizing the ring.
  - A target smaller than the render callback counts an underrun on each start from priming, and
    the cushion controller then grows the target.
  - `HoldPlayoutTest.kt:113-114`: a report's JSON, once the conversion to `LinkStats` exists.
- Parent's integration:
  - Every path was inside its writer's surfaces, and the copies were byte-identical.
  - `Package.swift` had not changed on the branch since the base.
  - Combined suite: 608 tests (playback 211), 0 known issues, 0 warnings on a clean copy.
  - `--sanitize=thread` over all 211 playback tests: exit 0, no reports.
  - The worktrees, their branches and the scratch folders were removed.
- Closing the pending slice before the high-risk unit:
  - The assessment always measures HEAD, so the slice `2e151e7..9c208e9` (T7a-T7d and the seam) was
    reviewed from a detached worktree at `9c208e9`, under the home directory.
  - Assessed `medium`, `under_budget` (34 paths, 4825 lines). START checked: base `d3ce3de` =
    `2e151e7`, candidate `5fb3a62` = `9c208e9`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: resilience and reliability, run
    concurrently.
  - Approved with no findings. Lineage `review-4ee1db544ed87fdc`, acknowledged. The reviewed
    boundary is now `9c208e9`, and the worktree was removed.
- T7e independent verifier (high-risk item 4). The previous session ended mid-run, so it was
  resumed.
  - Claims 1-3 and 5-7 PASS:
    - Release object code shows acquire as `ldapr` and release as `stlr`.
    - `render` calls only the ring's C functions, `_bzero` and the stack guard. It has no retain,
      release or allocation, and no indirect call.
    - A 12M-frame ring stress probe (capacity 1009, and 3) and a TSan run of 10M frames found no
      torn, reordered, missing or extra frame.
    - An end-to-end probe sent PMON over UDP to `AudioReceiver`, through `RingPlaybackSink`, to a
      render thread. Every frame came out in order, also under TSan.
    - Stop and restart, 300+300 rounds plus 150+150 under TSan: exactly one `onStopped` each time,
      0 errors, every stop under 1 ms.
    - The suite: 608 tests, TSan clean, 0 warnings.
  - Claim 4 FAIL, blocker B1:
    - A fixed cushion was sized in the current packet's frames but applied in the opening
      packet's frames, so a mid-stream block-size change doubled it. The probe asked for 2048
      frames where Android asks for 1024.
    - Root cause in T3: `CushionController.onPacket` folded Android's `packetFrames` (the cushion
      unit) and `packetSamples` (time) into one value.
    - Correction `6a02bf1`: `onPacket` takes `packetFrames` and `playedFrames`, and the loop passes
      the opening size and the played packet. RED against the old call: `[960, 2880]`.
    - 610 tests, 0 warnings, TSan clean. The recheck, limited to B1: PASS, 1024 frames as on Android.
  - Advisories, all inputs for T7f:
    - A1: `start(port:)` resolves no host (Android checks that the name resolves,
      `PmonPlayer.kt:155`). The phone binds a port and takes datagrams from any sender.
    - A2, unproven: a loopback wake-up could arrive after the old socket closes and reach a newer
      socket bound in that window, giving one spurious `headerTooShort`. It was not seen in 900
      rounds.
    - A3: `AudioReceiver.start` does not refuse while an older loop is still alive. T7f must wait
      for `isAlive == false`, as Android's view model does ("The previous receiver is still
      stopping").
    - A4: the ring is freed in the sink's `deinit`. T7f must keep a strong reference to the sink
      for the render callback until the engine has stopped.
    - A5: `RingPlaybackSink.init` calls `fatalError` when allocation fails. The factory must bound
      the sample rate before sizing the ring.
- T7e native review:
  - Assessed against `9c208e9` with item 4 escalated: `high`, due (19 paths, 3072 lines).
  - The preflight asked first for the untracked declaration: `.atl/` excluded. That STATUS refuses
    lenses and escalation flags; they go on the STATUS that offers START.
  - START checked: base `5fb3a62` = `9c208e9`, candidate `d1ce0c3` = `13a1dac`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: resilience and reliability.
  - Approved. Lineage `review-9c2046daec99c0bb`, acknowledged. The reviewed boundary is now
    `13a1dac`.
  - Its two warnings are one finding: a refused packet is counted as played, which Android does
    too. It is recorded under Follow-ups.
  - Tooling note: the session restart cleared the scratchpad, so the capture launcher was
    recreated before the reviewers could run.
- T7e complete. Next: T7f, the wiring and the end-to-end listening. The verifier's A1-A5 and the
  T7e1 inputs above are its inputs.
- T7f1 (session side), writer in a worktree:
  - `MonitorAudio` coordinator plus `AudioReceiving`, the four capabilities announced, and
    `LinkStats(LinkStatsReport)` with `recovered` always present. Android's JSON check from
    `HoldPlayoutTest.kt:113-114` is ported.
  - RED: 36 of 36 tests. GREEN: 645 in the full suite. TSan clean, 0 warnings, 17 of 17 mutants
    caught.
  - One mutant first survived: a late stop report ending the next session. It is now pinned by a
    test where three sessions start and stop back to back.
  - Divergences: stop reports reach the main actor late, so they are matched to launches by count.
    A refusal while the previous receiver stops throws Android's exact message.
  - Commit `40dea66`, with a stale comment in `ControlSession` fixed.
- T7f2 (app output), writer in a worktree:
  - `EngineOutputFactory` and `EngineOutput`: the ring held by the render block, which is made in
    a `nonisolated` function so the app's main-actor default never reaches the audio thread.
  - Bounds: 8 000-192 000 Hz and 1-16 384 frames (A5).
  - `close()` stops the engine, then the ring (A4).
  - A configuration change restarts the engine.
  - Smoke test on the simulator: 2 s of sine, `close` in 3-20 ms, no isolation trap.
  - Disassembly of the render path shows no lock, no allocation and no actor hop.
    `UnsafeMutableAudioBufferListPointer` was replaced with plain pointers, because its accessors
    are not inlined across modules.
  - Commit `6522b37`.
- T7f3 (inline):
  - `SessionModel` runs through `MonitorAudio` with `EngineOutputFactory`, and clears `desk` when a
    session ends on its own. The screen shows the packets played and the receiver's problem.
  - `EngineOutput` gained a 5 ms preferred I/O buffer (Android's low-latency track), a restart after
    an interruption that asks to resume, and session deactivation on close.
  - `Info.plist` declares `UIBackgroundModes` `audio`.
  - Commit `dc368e3`. The app builds with no warning in its files, and the package suite is 645,
    0 known issues.
  - An app-side fix was tried and reverted: keeping Bonjour off loopback broke the in-process
    Bonjour round trip (`BANDEAR_WS_IT=1`). A connection to the Mac's own address goes through
    `lo0`, so the cause stays with the desk's announcement.
- End to end against the desk on this Mac (simulator, iPhone 17 Pro):
  - The desk was run with `--device "Microsoft Teams Audio" --target 127.0.0.1:59999`.
  - The app found the desk, registered (`control registration ... as "iPhone 17 Pro"`), showed
    Connected and 1 strip, and played 3 935 packets in about 21 s. That is about 183 per second
    against the desk's 187.5 (48 000 / 256), within the screenshots' timing.
  - Over the same window, `nettop` showed the app receiving exactly what the desk sent to it.
  - The sound itself was not judged by ear: the virtual input carries silence. The user can listen
    with a live input.
- T7f independent verifier (high-risk item 4): no blocker; claims 1-6 PASS.
  - Probe: 3 seeds × 120 rounds; 8 threads per launch, 28 800 reports. No race under TSan, no
    session ended by an old report, every report applied on main.
  - Release disassembly: the render closure calls only `RingPlaybackSink.render`. The block thunk
    retains and releases the closure context once per callback.
  - Suite: 645 tests, TSan clean, 0 warnings, and the app builds.
  - Its four advisories are recorded under Follow-ups as T8 inputs.
- T7f native review:
  - Assessed against `13a1dac` with item 4 escalated: `high`, due (16 paths, 1754 lines).
  - START checked: base `d1ce0c3` = `13a1dac`, candidate `6a59918` = `e58e7e8`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: resilience and reliability.
  - Approved. Lineage `review-86e00a7b8617fc95`, acknowledged. The reviewed boundary is now
    `e58e7e8`.
  - Its four warnings (R4-001, R3-001, R3-002, R3-003) are under Follow-ups. They are not fixed:
    findings alone authorize no change.
  - Tooling note: the scratchpad was cleared between turns, taking the capture launcher with it.
    The grant and the captures then ran from one inline script.
- T7 complete. Pending: T8 (screens), T9 (mixes), the small `R3-001` unit (the transport's
  injected clock), and the follow-ups above, which are the user's to prioritise.
- T7g, one writer in a worktree (long-session backstop). The previous session ended mid-run, so it
  was resumed.
  - `OutputRecovery` (pure, 12 tests). `PlaybackSink.isPlaying`, with a default of `true`.
    `ReceiverStats.outputPlaying`.
  - `EngineOutput` follows the recovery: `setActive(true)` before every restart, no `try?`, and
    nothing after `close()`. It now also listens for an interruption beginning and for
    `didBecomeActive`.
  - The screen line shows while the output is stopped.
  - RED: 11 of 12 recovery tests failed against stubs (15 failed expectations), plus 1 loop test.
  - GREEN: 658 tests, 0 warnings on a never-built copy, and the app builds. 11 of 11 mutants were
    killed.
  - Smoke check on the simulator, with interruption notifications posted by hand and the real
    `setActive` and `engine.start()`: `isPlaying` went true → false → true (resume) → false
    (began, then an end without resume) → true (became active), and stayed false after
    `close()`.
  - Commit `db53b76`. Risk: item 4 (the restart runs under the lock, and the handlers run on the
    private queue).
- T7g independent verifier (high-risk item 4): no blocker. Claims 1-5 PASS.
  - Policy and tests: 5 spot mutants killed.
  - `setActive` comes before both `engine.start()` calls, and no `try?` remains.
  - One lock order: the output's lock, then AVAudioSession/AVAudioEngine. The observers use
    `queue: nil` and only enqueue async, so a notification posted inside `engine.start()` cannot
    re-enter the lock.
  - Package TSan probe: 6 seeds, clean.
  - Simulator stress of the real `EngineOutput` (8 threads posting notifications, `close()`
    racing), plain and under TSan: 40 of 40 opened, 0 restarts after close, 0 deadlocks, 0 TSan
    warnings.
  - 658 tests, 0 warnings, and the app builds.
  - Advisories:
    - A1, a product decision: an interruption whose end never comes (another app keeping its
      session active) leaves the output stopped for the rest of the session. Becoming active does
      not restart it while interrupted, and the screen copy ("or when you return to the app")
      promises otherwise.
    - A2: a late `.appWasSuspended` beginning can mark the output stopped while it plays.
    - A3: a failed restart's error is not logged.
    - A4: a configuration change can try a restart in the background. Only a device test proves
      what happens.
    - A5: `ReceiverStats.init` now needs `outputPlaying`. Only callers in this repository use it.
  - Note: the simulator's `com.bandear.ios` is now the verifier's TSan harness build. Reinstall
    the app before the next run.
- 2026-10-23, the user's decision on the verifier's A1. Question: "Si una interrupción nunca termina
  (otra app que mantiene su sesión de audio activa), ¿qué hace la app cuando el músico vuelve a
  abrirla?" Answer: "Volver a la app la reanuda (Recommended)".
  - Done in `3a469b8`: `OutputRecovery` ends the interruption when the app becomes active and
    restarts a stopped output.
  - RED: the new test failed (2 expectations). Becoming active was taken out of "restarts nothing
    while interrupted".
  - GREEN: 659 tests, 0 warnings. The screen copy is now true.
- T7g native review:
  - Assessed against `e58e7e8` with item 4 escalated: `high`, due (11 paths, 497 lines).
  - START checked: base `6a59918` = `e58e7e8`, candidate `8026d92` = `825e558`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: resilience and reliability.
  - Approved. Lineage `review-07942a25e4974537`, acknowledged. The reviewed boundary is now
    `825e558`.
  - Findings, all warnings or suggestions:
    - R4-001 / R3-001: the restart holds the lock that the receive thread's `isPlaying` read
      takes.
    - R4-002: a failed restart's error is dropped.
    - R3-002: the app glue has no test.
    - R3-003: the stale `.appWasSuspended` gap in this document, rewritten above.
  - The first three are under "Gaps left by T7g". They are not fixed, since findings alone
    authorize no change.
- T7g complete.
- T7h (inline), commit `58c1b24`:
  - `EngineOutput.isPlaying` now reads a value published behind its own lock. The recovery
    publishes it after `handle` (so a stopping event shows before the restart) and again after the
    restart's result.
  - Every access to `recovery` stays under the output's lock. The receive thread takes only the
    published lock, and none of its calls (`capacityFrames`, `write`, `setTargetFrames`,
    `underrunCount`, `presentedFrame`, `isPlaying`) takes the output's lock.
  - The app builds with no warning in its files. There is no automated test, because the app has
    no test target; its independent verifier is asked for a runtime probe.
- T7h independent verifier (high-risk item 4): no blocker; claims 1-5 PASS.
  - Measured on the simulator: a scratch hook held the restart for 300 ms under the lock while a
    reader called `isPlaying` every ~200 µs.
    - Longest read: 0.009 ms and 0.384 ms at `58c1b24`, against 309.7 ms and 305.5 ms at the base
      (one blocked read per restart).
    - The reader saw `false` from the interruption's beginning through the restart, then `true`
      0.2-0.3 ms after it.
  - Thread Sanitizer, linked into the simulator build: 4 runs with 0 warnings. A deliberate race
    produced 1 warning, which shows TSan was active.
  - 659 tests, and the app builds.
  - Advisories, all already true at the base:
    - `close()` can wait for one restart, so `onStopped` comes up to one restart later.
    - `isPlaying` keeps its last value after `close()`, which nobody reads.
- T7h native review:
  - Assessed against `825e558` with item 4 escalated: `high`, due (2 paths, 93 lines).
  - START checked: base `8026d92` = `825e558`, candidate `bdc2a8d` = `9641033`.
  - The user chose "Revisar este cambio" (`granted`).
  - Approved. Lineage `review-03e2e6bd2a108df4`, acknowledged. The reviewed boundary is now
    `9641033`.
  - Findings: two suggestions (seed the flag from the recovery) and one warning (R3-002, the
    publish order has no test). They are under "Gaps left by T7g" and not fixed.
- T7h complete.
- T8a, T8b and T8c: three writers launched together, each in its own worktree, all on `4801871`
  (the empty `BandEarScreens` module).
- T8a (connection and diagnostics words): 76 tests (RED 70 of 77), 10 of 10 mutants caught.
  - Numbers round half up, as Java's `String.format` does, so one packet lost in 800 reads
    "0.13 %".
  - Android's `UNAVAILABLE` and `CLOSED` control states have no iOS counterpart, and their words
    are not ported.
  - A desk typed by hand is named "host:port".
  - After the parent's message it maps no `nameIsBlank`, since T8c removed that error.
- T8b (Mix words): 71 tests (51 ported, RED 70 of 71), 10 of 10 mutants caught.
  - `PanScale` binds a native `Slider` over 0...1, with Android's 5% detent each side of centre.
  - The kind labels come from `ChannelKind.label` (`ClientMixState.kt`), which no Android test
    pins.
  - Android's gesture tests are skipped, since its own faders are not ported.
- T8c (the session's gaps): 27 tests, TSan clean, 15 of 15 mutants caught.
  - The name is kept as typed (cut at 64 UTF-16 units without splitting a character) and stored
    trimmed. A rename is announced 400 ms after typing stops (`NAME_RESEND_DEBOUNCE_MS`).
  - A blank name registers as `""`, as Android does, so `ClientMessageError.nameIsBlank` is gone.
  - `MonitorAudio.isRunning` and `underrunTotal`.
  - AD1: reports are tagged with their launch. AD2: late link reports are dropped. AD3: the owner
    is read on main.
  - Changed expectations, on purpose: four blank-name cases, plus `remembersTheName` (the session
    now keeps `" Bea "` as typed while the store keeps `"Bea"`). The verifier's A1 corrected
    this count.
- Parent's integration:
  - Every path was inside its writer's surfaces, on the same base, and the copies were identical.
  - The following banner's words existed twice (T8a's `ConnectionBanner`, T8b's `MixWords`).
    `ConnectionBanner.following` now uses `MixWords`, since Android keeps those words in
    `MixScreenText.kt`.
  - Combined suite: 834 tests (660 + 76 + 71 + 27), 0 warnings on a never-built copy, and the app
    builds.
  - Commits: `bb5562a` (T8b), `3e590c3` (T8a), `1e315f0` (T8c, alone, since it touches
    concurrency, item 4).
- Open for T8d: `SessionModel` connects under the device name when the field is empty. Android
  sends a blank name, and the user chose Android's rules, so T8d drops that substitution.
- T8c independent verifier (high-risk item 4): no blocker; claims 1-5 PASS.
  - Real-time pauses: five renames 100 ms apart send exactly one `register`, about 400 ms after the
    last.
  - `isRunning` agreed with `isSessionRunning` at every change over 650 starts.
  - Hammer probe under TSan (25 s, 9 956 steps, 1 627 renames): 0 violations.
  - Mutants removing the AD1 or AD2 guard, or reading the tag on main, were caught.
  - 834 tests, 0 warnings, and the app builds.
  - The differential against the base matches the changed expectations once A1's correction is
    counted.
  - A2 and A3 are T8d inputs (Follow-ups). A4: Android adds a late report's underruns to an ended
    session; iOS drops them, as AD2 asks.
- Native reviews for T8a-T8c:
  - The pending slice `9641033..3e590c3` (the T8 plan, the module, T8b, T8a) was reviewed from a
    detached worktree at `3e590c3`: `medium`, `under_budget` (44 paths). The user chose "Revisar
    este cambio" (`granted`). Lenses: reliability and readability.
    - Approved. Lineage `review-ce445ad2b433a1b5`.
    - R2-001: the header names a desk by `DiscoveredDesk.name`, the banners by
      `ConnectionWords.deskLabel`. The model names a typed desk by `deskLabel`, so they agree.
    - R2-002 and R2-003: two test names overstate their bodies.
    - R3-001: `PanScale.value` does not guard non-finite input. That is defensive only, since a
      `Slider` does not produce it.
  - T8c (`3e590c3..1e315f0`), from a detached worktree at `1e315f0`, with item 4 escalated: `high`
    (11 paths, 857 lines). The user chose `granted`. Lenses: resilience and reliability.
    - Approved. Lineage `review-3eab327013d37338`, acknowledged. The
      reviewed boundary is now `1e315f0`.
    - Two warnings, under Follow-ups: the launch tag's contract, and stale diagnostics at a start.
- T8d:
  - The seam: `SessionModel` (`ad8d2e1`) and a placeholder `MixView` with the shell's signature
    (`19e804c`). Then two writers launched together, each in its own worktree.
- T8d2 (shell, Connect, the manual address, Settings with Diagnostics, Mixes), `6badef1`:
  - `RootView` picks the screen with `AppScreen`: Connect in a `NavigationStack`, with the manual
    form pushed; a `TabView` while a session runs.
  - Discovery stops in the background and starts again on return.
  - The manual address screen is the Settings screen with a "Servers" back button, as on Android.
  - It builds, the suite passes, and every visible word matched `BandEarScreens`.
- T8d3 (Mix and the channel sheet), `c4a3bd1`:
  - `MixView` is the container; `MixContent` is presentational and has debug-only previews.
  - The pan `Slider` uses `PanScale` with the 5% detent. Once a drag leaves it, the thumb can
    trail the finger; that is worth a feel test on a device.
  - The sheet opens fully expanded (Android's `skipPartiallyExpanded`).
  - Simulator screenshots with a sample mix covered channels, groups, the master with the limit,
    the sheet, the notice, and following while reconnecting.
- Parent's integration:
  - The two units' helpers did not clash, and the copies were identical to the writers'.
  - My first copy missed the two modified files (a relative `cd` in a subshell); copying them by
    hand fixed the build.
  - The model's leftovers from the provisional screen were dropped (`6f009f3`).
  - The T8c verifier's A2 and the T8c review's warning were fixed test-first in `4f77237`:
    - the last session's figures are cleared before `session.start`, and put back if the session
      refuses the start;
    - RED: the new test caught the stale figures;
    - GREEN: 836 tests, 0 warnings, TSan clean, and the app builds;
    - `AudioReceiving.isAlive` now states its contract.
- End to end on the simulator against the desk on this Mac (`--device "Microsoft Teams Audio"
  --target 127.0.0.1:59999`):
  - Connect listed "BandEar server / BandEar on Carloss-M…" with Connect.
  - After typing "Carlos" and connecting, the desk logged `registration ... as "Carlos"`, and the
    app showed the Mix tab:
    - "Carlos / BandEar on Carloss-Mac-mini" with the green Connected pill;
    - More of me off, "MY CHANNEL None", and `CHANNELS · 1`;
    - CH 1 at 100% with "L100". That is the desk's default (`default_pan`, even sources hard left,
      `mix.rs:508-516`), not a fault.
    - the master at 100% with "Level cap 100% · protects your ears".
  - Mixes showed its intro and empty state.
  - Settings showed:
    - the server card ("… · connected") and Disconnect;
    - the manual connection, disabled with "Disconnect first to connect to another server.";
    - Diagnostics: Link good, 185 packets/s (the desk sends 187.5), loss 0.00 %, 0 underruns.
  - Typing " E" after the name made the desk log `registration ... as "Carlos E"` on the same
    connection: one announcement after the pause.
- Risk for T8d: item 6. The views have no automated test; the app has no test target. Their rules
  and words are tested in the package. Item 4 applies too: `4f77237` changes the order of the
  coordinator's state at a start.
- T8d independent verifier (items 6 and 4): no blocker. Claims 1-5 and 7 PASS, and claim 6 PASS for
  every listed exercise.
  - No copy in the views apart from the recorded stopped-output line.
  - Navigation matches `AppScreen.kt`.
  - Every edit goes through the model.
  - The stale-figures fix: its test fails against the base `MonitorAudio` and passes at HEAD.
  - End to end against the desk, through a passive observer WebSocket that receives the desk's
    `mix_state` pushes:
    - a slider to 62, mute on and off, pan dragged to centre (50);
    - More of me lifting to 100, then back;
    - a rename to "Verifier" on the same connection;
    - a manual connect named `192.168.1.54:50001`;
    - an empty name, registered "(unnamed)";
    - a Bonjour connect, and Disconnect.
  - Audio was not re-proven in its run: the macOS firewall blocked incoming UDP for its scratch
    bundle id. The parent's run on the same commit had 185 packets/s.
  - 836 tests, 0 warnings, and the app builds. Its trivial advisory (an unused `import UIKit`) is
    fixed in the next commit; the rest are under Follow-ups.
- T8d native review:
  - Assessed against `1e315f0` with item 4 escalated: `high`, due (22 paths, 2098 lines).
  - START checked: base `2c8bad3` = `1e315f0`, candidate `0cdb31e` = `de5d54f`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: resilience, readability and
    reliability.
  - Approved. Lineage `review-37d22f13a853d20b`, acknowledged. The reviewed boundary is now
    `de5d54f`.
  - 4 warnings and 4 suggestions are under Follow-ups. They are not fixed: findings alone authorize
    no change.
- T8 complete. Saving, recalling, renaming and deleting mixes are T9's.
- T8e (one writer in a worktree), commits `980f814` (package) and `bebd734` (app):
  - R4: the per-packet figures moved to `DiagnosticsModel` (`session.diagnostics`), which only
    `DiagnosticsSection` and the stopped-output row observe. `mirror()` assigns only changed
    values; errors are compared by type and `String(reflecting:)`, since a cast to `any Equatable`
    warns on Darwin.
    - Measured with scratch body counters at 185-187 packets/s: on Mix, `MixView.body` went up 375
      times every 2 s at the base. At the head it stays flat while `DiagnosticsSection` follows the
      reports.
  - R2: `MixTone`, `MixBannerRow` and `MixConnectionPill` are deleted. Mix uses
    `ConnectionBannerRow` and a new shared `ConnectionPillBadge`.
  - R3 (refused start): `MonitorAudio` reports one change after it restores the figures.
    - RED: the new test heard nothing. GREEN: 837 tests.
    - A mutant without the call is caught.
    - The `linkReport` suggestion was folded in.
  - R3 (discovery): `DeskDiscoveryModel.start()` does nothing until `stop()`. iOS 16 has no
    `onChange(initial:)`, so both triggers stay and the model is idempotent. Launch started the
    browser once, and each return from the background starts it once.
  - Checks: 0 warnings on a never-built copy, and the app builds.
  - Simulator tip: the desk's `--target 127.0.0.1:59999` reaches a manual connection to
    `127.0.0.1` with audio port 59999 over loopback, which the firewall does not block.
  - Risk: item 6. The app side has no automated test, only the probe and screenshots.
- T8e independent verifier (item 6): no blocker. Claims 1-6 PASS on `072e170` against the base
  `2d9c707`.
  - Its own body counters at 186-187 packets/s, rebuilds every 2 s:
    - base on Mix: about 380-401 rebuilds of `RootView`, `MixView` and `MixContent`;
    - head on Mix: 0 over five readings;
    - head on Settings: only `DiagnosticsSection` and the stopped-output row follow the reports.
  - A refused start reports one change at the head and none at the base. A successful start is
    unchanged.
  - Discovery started once at launch, and once per return across six trips to the background.
  - The tab after the background: not reproduced. Only a connect and the user write it. Nothing in
    T8e touches it.
  - 837 tests, 0 warnings, the app builds, and the end-to-end check against the desk passed
    (connect, slider, rename, disconnect, Bonjour).
  - Advisories: A1 (a failed browser retries only after a background trip) is already under
    Follow-ups. A2: the restored `linkReport` is always nil. A3: Mix's banners now look like
    Connect's. A4: the tab note remains unexplained.
- T8e native review:
  - Assessed against `de5d54f` with item 6 escalated: `high`, due (12 paths, 438 lines).
  - START checked: base `0cdb31e` = `de5d54f`, candidate `8674c13` = `9907f62`.
  - The user chose "Revisar este cambio" (`granted`). Lenses: reliability and readability.
  - Approved. Lineage `review-f598fe3b1e0b5532`, acknowledged. The reviewed boundary is now
    `9907f62`.
  - Two warnings (both already known) and two suggestions are under Follow-ups.
- T8e complete. Next: T9 (mix banks and recall), or the remaining follow-ups, as the user decides.
- T9a (one writer in a worktree), commit `0f5537b`:
  - It stopped once, correctly: a new `MixEvent` case needs a branch in `OutgoingMix.swift`'s
    exhaustive switch, a file the parent had left out of its surfaces. The parent widened the
    surface by that file, with Android's rule: a recall is sent unless the mix was following
    someone (`MonitorViewModel.kt:454`).
  - `MixSnapshot` (`MixState.snapshot`, never the More of me lift).
  - `MixBank`:
    - save trims the name, and a taken name replaces that mix;
    - rename refuses a blank, missing, unchanged or taken name, in Android's order;
    - delete;
    - the active mix is always a saved one.
  - `MixBankStore` and `UserDefaultsMixBankStore`, keyed `bandear.banks`, use Android's JSON. A
    damaged entry loses only itself.
  - `MixEvent.mixRecalled(MixSnapshot)` with `MixSession.recallRefusal` (`.following(leader:)`). It
    is the one rule the reducer, the send rule and the screens read.
  - The deferred recall cases at `MixReducerParityTests.swift:218` and `:296` are completed.
  - RED: 25 of 31 failed, then 10 of 10 for the recall. GREEN: 876 tests, 0 warnings, no stray
    preferences file, and 10 of 10 mutants caught.
  - Divergences:
    - stored text where a number belongs reads as the default, as in the `config` decoder;
    - a rename removes exactly the renamed mix;
    - `.mixRecalled` carries no name, since the active mix belongs to `MixBank`;
    - recalled values are not clamped, as on Android.
  - Not pinned: a recall that changes nothing is still sent, as on Android.
  - Risk: none. It adds a public `MixEvent` case, whose only exhaustive switches are updated. The
    `bandear.banks` key stores new data only.
- T9b (one writer in a worktree), commits `469d6ca` (package) and `0090894` (app):
  - `SavedMixes` (`@MainActor`, in `BandEarSession`) holds `mixes`, `activeName`, a typed `notice`
    and `stateDidChange`, and offers `save(as:)`, `recall(_:)`, `rename(_:to:)` and `delete(_:)`.
    It reuses `MixBank` and `recallRefusal`.
  - Android's notices and the remaining Mixes copy are in `SavedMixesWords`, verbatim.
  - `MixesModel` is observed only by the Mixes tab. `MixesContent` has the save field, the list with
    "Active now" and Recall, Rename and Delete in a ⋮ menu with their dialogs, the notice and the
    empty state.
    - `.menuOrder(.fixed)`: an upward menu on iOS 26 otherwise put Delete where Rename was drawn.
  - RED: 41 of 43 new tests failed. GREEN: 919 tests, 0 warnings, no stray preferences file, and
    12 of 12 mutants caught.
  - Divergences:
    - the list is read back before every action;
    - it is written only when it changed;
    - a refused recall never reaches the session;
    - delete asks in a popover anchored to the row's ⋮.
  - The writer again needed a file outside its surfaces: `RootView` constructs every tab. The parent
    applied that one line at integration (`MixesView(mixes: session.mixes)`).
  - End to end against the desk (a manual connection to `127.0.0.1`, audio port 59999, watched by a
    passive observer):
    - a blank save showed "Enter a name for the mix";
    - " Ballads " was saved as `Saved "Ballads"`, Active now;
    - CH 1 moved from 49 to 92, and Recall brought back 49 on screen and at the desk;
    - after a relaunch "Ballads" was still listed, and recalling it again gave 49;
    - a rename gave `Renamed "Ballads" to "Main set"`;
    - a delete gave `Deleted "Main set"`, with `bandear.banks` holding `[]`.
  - Risk: item 6. The views have no automated test; their rules and words are tested in the package.
- T9 independent verifier (item 6; T9a and T9b together, against the base `9907f62`): no blocker.
  Claims 1-6 PASS.
  - Its own 12 mutants were all caught.
  - End to end on its own iPhone 17 simulator, watched by a passive observer:
    - a blank save stored nothing;
    - " Verse " was saved and active;
    - 47 recalled to 100;
    - a re-save at 31 kept a single entry;
    - after a relaunch, Recall gave 31 at the desk;
    - a rename gave "Main set", still active;
    - a delete left `[]`.
  - The stored shape matches Android's: a `snapshot` held as a JSON string.
  - 919 tests, 0 warnings, the app builds, and Mix, Settings and Connect are unchanged.
  - Its minor advisories are under Follow-ups.
- Native reviews for T9:
  - The pending slice `9907f62..b2893cd` (the T9 plan and T9a) was reviewed from a detached worktree
    at `b2893cd`: `medium`, `under_budget` (12 paths).
    - The user chose `granted`. Lenses: reliability and resilience.
    - Approved. Lineage `review-a550eb0d00308ffc`.
    - One finding, unclamped recall values (Follow-ups).
  - T9b (`b2893cd..55cd593`), with item 6 escalated: `high` (12 paths, 1351 lines).
    - The user chose `granted`. Lenses: reliability and readability.
    - Approved, and acknowledged. The reviewed boundary is now `55cd593`.
    - Three warnings and two suggestions (Follow-ups).
- T9 complete. Every task of the plan (T1-T9) is done. What remains is under Follow-ups, for the user
  to prioritise. Pushing, pull requests and merging remain the user's decisions.
- T9c (one writer in a worktree), commit `2574f1b`:
  - Delete: `confirmDelete(name)` clears `deleting` before it deletes, and Cancel clears it too. On
    the simulator, deleting "Verse" and saving "Verse" again showed no dialog. The popover's Cancel
    cannot be reached on screen, since tapping outside closes it.
  - Notices: `SavedMixes.noticeNumber` goes up with every outcome, equal words included, and stays
    the same for an action with none (an unchanged rename). `MixesModel` publishes it, and the tab
    announces on it.
    - RED: 2 of 11 tests failed. GREEN: 921 tests.
    - Two mutants caught: no bump on equal words, and a bump with no outcome.
    - A scratch logging build confirmed six separate announcements for repeated blank saves, saves
      and recalls.
  - Comments: `init` says it lists the store's mixes, none active, applying and sending nothing.
    `notRecalled` names `because`. The parent fixed the same wrong wording on a test helper at
    integration.
  - 921 tests, 0 warnings, no stray preferences file, and the app builds.
  - Risk: item 6. The two `MixesContent` changes have no automated test; they rest on the simulator
    and the logging build.

