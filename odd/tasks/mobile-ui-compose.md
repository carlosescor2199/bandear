# Mobile UI in Compose

## Objective

Rebuild the Android app's screen as the approved redesign: a musician-first app with a simple
connection, the mix as the main screen, an own MixLink look consistent with the desktop, and the
diagnostics out of the way.

## Problem and why

The app is one long developer-style scroll (`activity_main.xml`): title "MixLink M0", raw host and
port fields, Start and Stop, two status lines, horizontal sliders, banks, More of me at the very
end, then eight lines of counters and the errors. On stage it is hard to use one-handed, More of
me is buried, and the diagnostics take the space of the mix. It also shows "No saved banks" right
above a saved bank.

## Design (approved)

The mockup is the Design artifact "MixLink mobile redesign"
(https://claude.ai/artifact/CSVZnwPMUeCxb4kgQ59MQU), five artboards:

- **Connect**: your name; discovered servers as large cards with one Connect action; the manual
  address lives in Settings.
- **Mix** (main): name and server, a status pill (connected, reconnecting, connecting, not
  connected), a large More of me toggle beside "My channel", a horizontally scrolling row of
  vertical channel faders with group chips and 44 px Mute and Solo buttons, a Master card with its
  level cap, and a bottom bar with Mix, Mixes and Settings.
- **Saved mixes**: save the current mix by name; a list with Recall; rename and delete behind a
  menu so a stray tap cannot delete a mix.
- **Settings**: the server card with Disconnect, your name, manual connection, compact diagnostics
  with the same link-health light as the desktop, and an expander for all counters and errors.
- **Connection states**: the pill states, the reconnecting banner and the link-health legend.

Look: ground `#0d1117`, surfaces `#161b22` and `#1f2630`, borders `#2b3440`, text `#e6edf3`,
secondary text `#9ba8b5`, amber accent `#f5a524`, green `#3fb950`, red `#f85149`, blue `#4cc2ff`
for connecting; IBM Plex Sans for text and JetBrains Mono for numbers (bundled in T5).

## Design decisions

- **Jetpack Compose**, chosen by the user over framework Views and Material Views: the design
  needs custom vertical faders, pills, chips and tabs, and it fits atomic design with
  container/presentational separation.
- Kotlin 2.0.21 ships the Compose compiler as a Kotlin plugin, so no Kotlin or AGP (8.7.3) upgrade
  is needed.
- State moves out of `MainActivity` into a ViewModel with pure, JVM-tested reducers; composables
  are presentational and receive state plus callbacks.
- The networking and audio classes (`ControlChannel`, `PmonPlayer`, `ServerDiscovery`,
  `MixBankStore`) keep their behaviour; only how the UI drives and reads them changes.
- No navigation library: three tabs plus the connect screen are plain state.

## Scope

- Compose setup, theme and atoms; the ViewModel; the four screens; switching `MainActivity` to
  Compose and deleting the XML layout; the saved-banks empty-state bug; bundling the two fonts;
  the MixLink launcher icon (added by the parent for T6).

## Out of scope

- New mixing features (the redesign shows what the app already does).
- Desktop changes.

## Constraints

- Clean-room design; own names, look and copy.
- Touch targets of at least 44 px; text contrast at least 4.5:1; state never shown by colour
  alone.
- Keep technical artifacts and UI copy in English.

## Tasks

- [x] T1 (base): Compose in Gradle; MixLink theme tokens; atoms: status pill, link dot, vertical
  fader, group chip, mute and solo buttons, primary and secondary buttons, bottom tab bar; previews;
  JVM tests for the pure logic (fader value mapping, state to pill and label mapping). The app
  still runs the XML screen. Commit: this commit.
- [x] T2 (state): `MonitorViewModel` holding connection, discovered servers, mix, saved mixes and
  diagnostics; pure reducers with JVM tests; `MainActivity` logic moved behind it (the Activity
  itself switches in T4). Commit: this commit.
- [x] T2b (review follow-ups): T2's native review (lineage `review-ead3157fe7a55d5c`) approved and
  acknowledged with two warnings, both fixed here: the receiver's stop report ends only its own
  session, once, and nothing after `onCleared`; a late `mix_ack` after the session ended changes
  nothing. Lands after T3. Commit: this commit.
- [x] T3 (screens): Connect, Mix, Saved mixes and Settings composed from the atoms, with previews;
  `MonitorApp(state, actions)` as the root; the T1 review's fader follow-ups. The app still runs
  the XML screen. Commits: `a17522b` (fader fixes) and this commit (screens).
- [x] T4 (switch): `MainActivity` hosts Compose; `activity_main.xml` deleted; the saved-banks empty
  state fixed; live check on the S23 (debug build; the items still unobserved are listed below as
  follow-ups).
  - [x] Code: T3 review follow-ups (`69f613d`), receiver stop ordering (`538f990`), the switch to
    Compose (`0f51cb2`), R8 for release (see Progress). Checks per commit in Progress.
  - [x] Live check on the S23 (parent), on the debug build, including
    the channel sheet (pan detent, TalkBack) and the strip height from T4b. Observed so far (parent,
    S23): the Mix screen fits (header, More of me, both strips with Mute and Solo, the Master card
    with the level cap, the tabs), and pan works end to end in the UI (after using the channel
    sheet, a recents screenshot showed CH 2 "R100" and CH 1 "C"). Not yet observed: the pan
    detent, TalkBack on the sheet, and the minified release build on the phone.
  - [x] Pan: the old screen had a pan slider per channel; the new screens had none, so after the
    switch pan could not be changed (it was still kept, sent, saved and recalled). The user chose:
    a strip's label opens a sheet for that channel with a large L–C–R pan control, its level, Mute
    and Solo, and each strip shows its pan under the label; the Mix screen must not grow. Done in
    T4b (`f86414e`; see Progress), which closes the acceptance criterion "every existing control"
    again.
- [x] T5 (fonts): bundle IBM Plex Sans and JetBrains Mono (OFL), after the user approves the
  download. The user approved it; see Progress. Commit: `e116aa3`.
- [x] T6 (launcher icon): the app still showed the default Android robot; an adaptive MixLink icon
  drawn in-house as vectors. Commit: `e1a4e7c`.
- [x] T4c (review follow-ups): the two suggestions of T4b's native review (lineage
  `review-750e0cd8bc15bcc0`, approved): the channel sheet finds its channel by index, and a paste
  with no room changes nothing. Commit: `fc925f9`.
- [x] Live check of the bundled fonts and the launcher icon on the S23: confirmed by the user.
  Native review of `e116aa3`..`fc925f9` (lineage `review-9478d7063e947bbb`) approved with no
  findings.

## Follow-ups

- Not yet observed on a phone: the pan detent at C, TalkBack on the channel sheet and the fader
  slider semantics, and the minified (R8) release build.
- The variable-font weights use the `@ExperimentalTextApi` `Font(..., variationSettings)`
  overload; the stable `Font(resId, weight)` sets the same `wght` axis and could replace it.
- No Compose UI tests (they need instrumentation); the screens are covered by previews, pure
  tests and the live checks above.

## Acceptance criteria

- The app matches the approved artboards in layout, colours and states.
- Connecting needs one tap on a discovered server; manual address still possible in Settings.
- More of me and the channel faders are reachable without scrolling the page vertically.
- Every existing control (channel level, pan, mute, solo, groups, master, level cap, banks, More
  of me, name, reconnect states, diagnostics) still works.
- The saved-banks list never shows "No saved banks" next to a saved bank.

## Acceptance checks

- `gradle -p apps/android :app:testDebugUnitTest assembleDebug` (JAVA_HOME and ANDROID_HOME as in
  the earlier features)
- Live on the S23 for T4.

## Route

- T1 to T4: delegated direct, one writer per task (2+ non-trivial files each).
- T4b (pan sheet and the T4 review follow-ups): delegated direct, one writer (2+ non-trivial
  files per work unit).
- T5: the parent downloaded and verified the fonts after the download permission; bundling and
  wiring them, delegated direct, one writer (the last units of the feature are one bounded
  writer's work).

## Delivery forecast

About 2000 authored changed lines across T1 to T4, mostly composables and tests; stacked pull
requests on `feat/reconnect-4-android-id` (#10), one per task, as the user chose for the previous
features.

## Progress

- Branch `feat/mobile-ui-compose` created from `ca22bee` (top of the auto-reconnect chain).
- T1 done (route: delegated direct, one writer; trigger: 2+ non-trivial new files). What landed:
  - Gradle: Kotlin Compose compiler plugin `org.jetbrains.kotlin.plugin.compose` 2.0.21 (declared
    at the root, applied in `app`), `buildFeatures.compose`, Compose BOM `2024.12.01` (Compose 1.7,
    Material 3 1.3) with `ui`, `foundation`, `material3`, `ui-tooling-preview` and debug-only
    `ui-tooling`, `activity-compose` 1.9.3, `lifecycle-viewmodel-compose` and
    `lifecycle-runtime-compose` 2.8.7. Every requested version resolved with compileSdk 35 and
    AGP 8.7.3; none was changed. `minSdk` stays 29.
  - `ui/theme`: `MixLinkTheme` (dark only) with `MixLinkColors` (the look tokens plus the pill
    tints, fader colours and four group tones from the artboards), `ColorToken` (GREEN, AMBER, RED,
    BLUE, NEUTRAL) so pure mappings stay testable without Compose, `MixLinkFonts` as the single
    font seam for T5 (system sans and monospace for now), `MixLinkType` for numbers and caps
    labels, Material typography, and `MixLinkShapes` (6, 8, 10, 12, 14, 22 dp).
  - `ui/components`, presentational with previews: `StatusPill`, `LinkDot` (optional words),
    `VerticalFader` (relative drag, tap to jump, horizontal swipes left to the parent row,
    slider semantics with `setProgress`), `GroupChip` with `groupToneFor`, `MuteSoloButtons`,
    `PrimaryButton`, `SecondaryButton`, `MoreOfMeToggle`, `BottomTabBar` with `MainTab`.
  - Pure logic: `faderValueAt` / `faderOffsetOf`, `pillKindFor` / `pillWords` / `pillToken`,
    `linkHealthWords` / `linkHealthMeaning` / `linkHealthToken` / `linkHealthDescription`,
    `moreOfMeCaption`, `groupToneFor`.
- Deviations and notes:
  - Mute and Solo keys are 44 x 44 dp, as the constraint asks; the artboard's 34 px keys do not
    fit it, so T3's channel strip needs about 104 dp instead of 84.
  - `StatusPill` takes a `PillKind`; `pillKindFor(ControlState)` maps the internal state to it.
    UNAVAILABLE and CLOSED read "Not connected"; who holds the mix belongs to the reconnecting
    banner (T3), not the pill.
  - Toggles report the new value (`(Boolean) -> Unit`), like Compose's own switches;
    `MuteSoloButtons` also takes the channel label so a screen reader says "Mute CH 3".
  - The selected tab also gets a small bar above its icon, so it is not told by colour alone.
  - Debug APK grew from 3,692,189 to 25,678,056 bytes: the unminified Compose and Material 3 dex,
    stored uncompressed because minSdk is 29 or higher. An unminified release APK is 19,276,096
    bytes; the app has no R8 setup yet (follow-up for T4).
  - `assembleRelease` fails in `lintVitalAnalyzeRelease` with the JDK version string `25.0.3`
    (AGP 8.7.3 lint on the Android Studio JBR 25); not part of the acceptance checks, not caused by
    these sources, and not verified on the base.
  - About 1,480 authored added lines, well over the ~400 heuristic: eight atoms with previews and
    KDoc, the theme, and 24 tests, which only make sense together as the base.
- T2 done (route: delegated direct, one writer; trigger: 2+ non-trivial new files). What landed, all
  new files under `ui/state`; `MainActivity`, the XML layout and T1's `ui/theme` and
  `ui/components` are untouched, and the app still runs the XML screen:
  - `MonitorUiState` (public, immutable): name; `ConnectionUiState` (pill, session running, server,
    who holds the mix, the existing status words, why a Connect was refused, the manual form);
    found servers as `ServerEndpoint`; `MixUiState` kept like the snapshot (levels, pans, mutes,
    solos, group levels, master, level cap) with derived `channels` (label "CH n", group tag) and
    `groups`; `MoreOfMeUiState` with a `MoreOfMeNotice`; `SavedMixesUiState` whose empty message is
    derived from the list, so it cannot show beside a saved mix; `DiagnosticsUiState`; the tab.
  - `MonitorEvent` and the pure `reduce(state, event)`, following `MainActivity`'s rules: config
    resizing as `applySourceChannels`, group levels kept by position, a musician channel the config
    drops is forgotten (More of me off with a notice), More of me needs a channel, recall applies
    the saved arrays and keeps what the server announced.
  - `phoneLinkHealth`: the phone judges its own link report with the server's thresholds
    (`health.rs`): nothing received is Silent, a timeout, 2 underruns/s or 2 % loss is Bad, any
    underrun or 0.5 % loss is a Warning.
  - `MonitorCollaborators`: seams `AudioReceiver`, `ServerScanner` and `MonitorStore` with private
    production adapters over `PmonPlayer`, `ServerDiscovery` and `MixBankStore`; the client id as
    `() -> String` from `ClientIdentity`; the real `ControlChannel` on its existing
    `ControlTransport` and `ControlScheduler` seams. No existing class changed.
  - `MonitorViewModel` (`ViewModel`, `Factory` from the application): `StateFlow<MonitorUiState>`,
    intents for connect (found server or manual address), disconnect, channel level, pan, mute and
    solo, group level, master volume, mute and level cap, More of me and the musician channel,
    save, recall, rename and delete a mix, the manual form, the name and the tab. Callbacks from the
    receiver, control and discovery threads become events applied by compare-and-set; session start
    and end are serialised by one lock, as the UI thread did. `onCleared` tears down like
    `onDestroy`. Only the master values are mirrored into `ClientMixState`, the one thing the
    receive thread reads; the control channel reads the mix from the state at send time.
- T2 deviations and notes:
  - Discovery also restarts when the receiver ends on its own; `MainActivity` restarted it only on
    Stop, which would leave the Connect screen's list frozen after a failed session.
  - Rename is new (the store has none): save under the new name, then delete the old one, so the
    renamed mix moves to the end of the list; a name another mix already has is refused, so a
    rename never overwrites a mix.
  - A control change that changes nothing (the same value, a channel that does not exist) sends no
    mix, as a SeekBar that did not move never did; levels and pans are clamped to 0..100.
  - A control callback that arrives after the session ended cannot raise the pill again.
  - "Active" saved mix: the one last saved or recalled, while it is still saved; moving a fader
    afterwards does not clear it.
  - Copy: notices say "mix", not "bank"; the More of me notices are reworded; the Connect
    validation messages are kept word for word.
  - Stats still arrive per packet (about 100/s), as the old text did. T3 should pass sub-states
    (`mix`, `connection`) to the screens: an unchanged sub-state keeps its instance across a stats
    update, so strong skipping skips those screens.
  - The new state layer is public where T3's composables need it; events, the reducer and the
    ViewModel are internal because they use internal types (`ControlState`, the seams).
  - About 2,550 authored added lines (1,210 main, 1,350 tests), far over the ~400 heuristic: one
    state layer, its reducer and its ViewModel only make sense together, and the 82 tests are most
    of it.
- T3 done (route: delegated direct, one writer; trigger: 2+ non-trivial new files). Two work-unit
  commits; `MainActivity`, the XML layout, `ui/state` and `ui/theme` are untouched:
  - `a17522b` fixes the T1 review's follow-ups (lineage `review-940bed3578872332`): the gesture
    arithmetic of `VerticalFader` (thumb-centre offset, drag seed from the current level,
    de-duplicated reports, step rounding and clamping) moved into the pure `FaderGesture`, and a
    fader with no travel (laid out no taller than its thumb, or not laid out) now ignores taps,
    drags and screen-reader steps instead of reporting 0, so input can never silently drop a level
    to silence.
  - Screens (`ui/screens`), presentational, each taking only the sub-states it shows plus
    callbacks: `ConnectScreen` (name, found servers as one-tap cards, Searching, the refusal, the
    manual address link), `MixScreen` (header, pill, connection banner, More of me with the "My
    channel" menu, a sideways row of 104 dp channel strips or group strips, the Master card with
    level, Mute and level cap), `SavedMixesScreen` (save, Recall, a ⋮ menu with Rename and a
    confirmed Delete, the empty state), `SettingsScreen` (server card with Disconnect, name, manual
    form, compact diagnostics with the phone's link health, an expander with every counter and
    the recent errors; with `onBack` it is the manual address form opened from Connect).
  - `MonitorApp(state, actions)`: Connect without a session, the three tabs with `BottomTabBar`
    with one; system back closes the manual form or returns to Mix; safe-drawing insets applied
    (they take effect once T4 makes the Activity edge to edge). `MonitorActions` groups every
    callback with the ViewModel's intent names and parameters, so T4 can bind it by adding the
    interface to `MonitorViewModel`.
  - New atoms (`ui/components`): `HorizontalFader` (master and level cap, on the same
    `FaderGesture`), `MixLinkTextField` and `FieldLabel` (the field keeps what is typed and ignores
    lagging echoes while focused), `StatusBanner`, `SegmentedChoice`, `DangerButton`, and
    `LineIcons` (the tab bar's private icon builder moved there to be shared).
  - Pure logic with JVM tests: `appScreenFor`, strip value, fill and description, group members,
    section and master texts, the "My channel" words, header texts, `connectionBanner`, the
    diagnostics readings and formatting (fixed `Locale.ROOT`, so a Spanish phone still shows
    "0.02 %"), server card texts, saved mixes title, detail and rename check, `adoptOutsideText`.
  - Previews for every screen at 390 x 844 dp from `PreviewSamples`: connected, More of me off, no
    channel chosen, reconnecting, lost the server, groups view, waiting for channels; Connect
    found, searching and refused; mixes filled and empty; Settings connected, all counters, and the
    manual form refused; the app on each tab.
- T3 deviations and notes:
  - The level cap gets its own slider in the Master card: the artboard showed it as text only,
    but it must stay adjustable. On a 390 x 844 dp screen the header, More of me and the full
    channel strips (Mute and Solo included) fit without scrolling, also with the reconnecting
    banner; the page scrolls a little to reach the level cap slider.
  - The pill also has a banner for a session whose control channel gave up ("Lost the server",
    with who holds the mix and how to reconnect), not only for reconnecting. The artboard's "Next
    try in 2 s" is left out: the state has no retry time.
  - Server cards read "MixLink server" with the address and port: beacons carry no server name
    or sample rate. Saved mixes show "Active now" only; the store keeps no dates.
  - No pan control: the old screen had none either; pans are kept in the mix and saved mixes as
    before.
  - The Channels and Groups toggle keys are 44 dp tall (36 on the artboard), for the touch target
    rule; the "My channel" caps are tighter (0.08 em) so they fit the 104 dp button.
  - The Channels or Groups choice and the manual form flag are screen state
    (`rememberSaveable`), not `MonitorUiState`.
  - About 3,400 authored added lines in the screens commit (about 3,000 main, 360 tests), far over
    the ~400 heuristic: four screens, their previews and the root only make sense together.

- T1 RED: the 24 new tests against `TODO` stubs: 24 run, 24 failed with `NotImplementedError`.
- T1 GREEN: the same 24 pass.
- T1 `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL; 0 failures,
  0 errors; no Kotlin warnings. Corrected count (parent recount with `--rerun-tasks`, 21 result
  files): 139 tests, that is 115 before plus the 24 new ones. The writer's report of 155 (131 + 24)
  was a miscount.
- T1 `cargo fmt --all -- --check`: clean (no Rust changes).
- T2 RED: the 82 new tests (49 reducer, 6 link health, 27 ViewModel) against `TODO` stubs of
  `reduce`, `phoneLinkHealth` and the ViewModel intents: 82 run, 81 failed with
  `NotImplementedError`; the one pass reads a derived default (`lossPercent` of the empty state).
- T2 GREEN: the same 82 pass.
- T2 `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD SUCCESSFUL,
  41 tasks executed; 24 result files, 221 tests (139 before plus 82), 0 failures, 0 errors; no
  Kotlin warnings.
- T2 `cargo fmt --all -- --check`: clean (no Rust changes).
- T3 fader fixes RED: the 14 new `FaderGestureTest` tests against `TODO` stubs: 14 run, 14 failed
  with `NotImplementedError`. GREEN: the same 14 pass. Then `gradle -p apps/android
  :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD SUCCESSFUL, 41 tasks executed; 235
  tests (221 plus 14), 0 failures, 0 errors; no Kotlin warnings.
- T3 screens RED: the 36 new tests (6 screen choice, 16 Mix texts, 8 Settings texts, 3 saved
  mixes texts, 3 text field sync) against `TODO` stubs: 36 run, 36 failed (35 with
  `NotImplementedError`, 1 assertion on the stubbed tab-bar flags). GREEN: the same 36 pass.
- T3 `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD SUCCESSFUL,
  41 tasks executed; 30 result files, 271 tests (235 plus 36), 0 failures, 0 errors; no Kotlin
  warnings. Debug APK 26,284,322 bytes.
- T3 `cargo fmt --all -- --check`: clean (no Rust changes).
- T3 has no Compose UI tests (they need instrumentation): the screens are checked by compiling
  their previews and by the pure tests; the live check is T4's.
- T2b done (route: delegated direct, one writer; a correction unit for T2's review, `ui/state`
  only). What landed:
  - `R3-late-onstopped-after-teardown`: `MonitorViewModel` counts launched sessions and stop
    reports under the session lock. The receiver reports exactly one stop per launch, in launch
    order, so the n-th report belongs to the n-th session; a report ends a session only when it is
    the latest session's and that session has not ended yet. `disconnect()` marks the session ended
    before asking the receiver to stop, so the late report finds nothing to end; a report for a
    session a newer one replaced does nothing; `onCleared` sets a cleared flag under the lock, so
    no later report reaches the shut-down channel or discovery. A stray extra report counts as the
    latest session's, so the counts cannot drift. The shared teardown is `closeSession`.
  - `R3-late-ack-not-session-guarded`: the reducer drops `MixAcknowledged` while no session runs,
    as it already dropped a late `ControlChanged`.
  - Fakes: `FakeReceiver.stop()` now only asks the loop to end; `finishStopping()` delivers the
    late report (flag down, then `onStopped`), as the real `finally` does. `FakeScanner.start()`
    fails after `shutdown()`, as the real discovery's executor rejects work.
- T2b note (not fixed, outside `ui/state`): `PmonPlayer`'s `finally` clears `receiverThread`
  before it sets the shared `running` flag false, so a session started in that window has its
  flag dropped by the previous receiver; that session then ends when its own loop sees the flag.
  The ViewModel now attributes both reports correctly (test
  `theLastSessionsLateStopReportCannotEndTheNextSession`), but the early end itself is
  `PmonPlayer`'s.
- T2b RED: the 7 new tests (5 ViewModel, 2 reducer) against the unchanged code: 89 `ui.state`
  tests run, 6 failed (4 assertions, 2 `IllegalStateException` from `scanner.start()` after
  shutdown); the seventh, `anAcknowledgementInTheNextSessionIsHeldByTheServer`, is a regression
  guard that passes on both. GREEN: all 89 pass.
- T2b `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD SUCCESSFUL,
  41 tasks executed; 30 result files, 278 tests (271 plus 7), 0 failures, 0 errors; no Kotlin
  warnings.
- T2b `cargo fmt --all -- --check`: clean (no Rust changes).
- T4 (route: delegated direct, one writer; trigger: 2+ non-trivial files per work unit). Work-unit
  commits, in order:
  - T3 review follow-ups (lineage `review-3a5eba80821594c9`, approved), this commit:
    - WARNING `SaveCard` cleared the typed name whatever the save did. `saveMix` (ViewModel and
      `MonitorActions`) now returns whether the mix was saved, and the field is cleared only then,
      through the pure `nameAfterSave`. The only refusal is a blank name: saving under a name
      already used updates that mix, as the old screen did (now pinned by a test).
    - SUGGESTION the horizontal fader's flip (tap measured from the right edge, drag delta negated)
      moved into the pure `HorizontalFaderGesture`, which `HorizontalFader` now uses.
    - SUGGESTION `RenameDialog` passes the trimmed name (`renameTarget`); `canRename` reads it.
    - RED: 13 new tests (8 `HorizontalFaderGestureTest`, 4 `SavedMixesTextTest`, 1 ViewModel) and
      a new assertion in `savingAndRecallingAMix`, against `TODO` stubs and a `saveMix` that
      always returned false: 48 run, 14 failed (12 `NotImplementedError`, 2 assertions). GREEN:
      all pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 31 result files, 291 tests (278 plus 13), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,300,706 bytes. `cargo fmt --all -- --check`: clean.
  - `69f613d` is the commit above. `PmonPlayer` stop ordering (the T2b note), this commit:
    - The receive loop's `finally` cleared the receiver before it lowered the shared session flag,
      so a session started in that gap had its flag dropped by the previous loop and ended early.
      The new `ReceiverLaunches` (in `PmonPlayer.kt`, pure, generic over the receiver) records the
      latest launch; a finishing loop lowers the flag only while it is still the latest launch,
      then reports `onStopped`, and only then frees the receiver. A caller that waits for the
      receiver to be free (the ViewModel does) therefore never starts a session the old loop can
      end, and still gets one stop report per launch, in launch order (now documented on
      `Listener.onStopped`). The `socket` field is cleared as before; a newer launch can only open
      its socket after the old loop has freed the receiver.
    - RED: the 5 new `ReceiverLaunchesTest` tests against `TODO` stubs: 5 run, 5 failed
      (`NotImplementedError`). A probe with the old ordering (free, then lower the flag
      unconditionally, then report; not committed) failed exactly the 2 race tests. GREEN: all 5
      pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 32 result files, 296 tests (291 plus 5), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,300,706 bytes. `cargo fmt --all -- --check`: clean.
  - `538f990` is the commit above. The switch to Compose, this commit:
    - `MainActivity` is a `ComponentActivity`: `enableEdgeToEdge` with dark bar styles (transparent
      bars, light icons whatever the phone's theme), `MonitorViewModel` from its `Factory` via
      `viewModels`, state collected with `collectAsStateWithLifecycle`, and
      `MixLinkTheme { MonitorApp(state, viewModel) }`. `MonitorViewModel` implements
      `MonitorActions` (`setChannelPan` stays a ViewModel-only intent).
    - Theme: `Theme.MixLink` now extends the dark `Theme.Material.NoActionBar` with the window
      background, status bar and navigation bar in the ground `#0d1117` (new `values/colors.xml`)
      and light bar icons, so nothing white shows at launch. The manifest label is
      `@string/app_name`, now "MixLink" (was "MixLink M0"); the activity uses `adjustResize` so
      the keyboard arrives as an inset the UI pads for.
    - Deleted: `res/layout/activity_main.xml`; the old-UI view builders only `MainActivity` used:
      `BankControlViews.kt` (`buildBankRows`, `buildMusicianChannelAdapter`),
      `ChannelControlViews.kt` (`rebuildChannelControls`), `FoundServerViews.kt`
      (`rebuildFoundServers`), `GroupControlViews.kt` (`rebuildGroupControls`). No other resource
      was used only by the layout. `ControlStatus.kt`, `ClientMixState.kt` and the rest stay: the
      ViewModel and reducer use them.
    - Old-activity behaviour compared with the ViewModel: start and stop, validation messages, the
      "still stopping" refusal, discovery stop and restart, registration with name, port and client
      id, name re-announce, config resizing, groups, More of me, saved mixes, link reports, stop
      handling and teardown are all the ViewModel's (T2, T2b). Carried over here, because only the
      old layout had them: the name fields stop at 64 characters (`MAX_NAME_LENGTH`, the length
      the server keeps; the old field had `maxLength="64"`), through the pure `keptEdit` in
      `MixLinkTextField`; the full counters and recent errors can be selected and copied
      (`SelectionContainer`; the old text was `textIsSelectable`).
    - Changed on purpose: turning the phone no longer ends the session (the old `onDestroy` ran on
      every rotation; the ViewModel survives it). The session still ends when the Activity
      finishes for good. Back on the root screen moves the app to the background on Android 12
      and later, as before.
    - Found missing, not fixed (a design decision): the per-channel pan sliders (see the T4 task
      items). The T3 note "No pan control: the old screen had none either" was wrong: the old
      `ChannelControlViews` built a "Pan n" slider under every channel.
    - RED: the 6 new `MixLinkTextFieldTest` tests against a `TODO` `keptEdit`: 9 run, 6 failed
      (`NotImplementedError`). GREEN: all 9 pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 32 result files, 302 tests (296 plus 6), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,248,536 bytes. `cargo fmt --all -- --check`: clean.
  - `0f51cb2` is the commit above. Release shrinking (the T1 follow-up), this commit:
    - `release` runs R8: `isMinifyEnabled` and `isShrinkResources`, the default
      `proguard-android-optimize.txt` and a new `proguard-rules.pro`. The app needs no keep rules
      (no reflection, JSON by explicit keys, the ViewModel from an explicit factory; the libraries
      bring their own); the file keeps source files and line numbers for readable crash traces.
      R8 needed no extra rule and reported no missing class.
    - `gradle -p apps/android assembleRelease -x lintVitalAnalyzeRelease`: BUILD SUCCESSFUL before
      and after. Unsigned release APK 19,682,720 bytes unminified (at `0f51cb2`), 2,390,604 bytes
      with R8 (about 88 % smaller). The build script prints the existing `kotlinOptions`
      deprecation warning when it is recompiled; it was not introduced here.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 302 tests, 0 failures, 0 errors. Debug APK 26,248,536 bytes
      (debug is not minified). `cargo fmt --all -- --check`: clean.
    - The minified release build has not run on a phone yet: the S23 check should use it too.
  - `3a27e61` is the commit above.
- T4b (route: delegated direct, one writer; trigger: 2+ non-trivial files per work unit). The pan
  decision and the follow-ups of T4's native review (lineage `review-22d833ad2e134030`).
  Work-unit commits, in order:
  - The channel sheet with pan (WARNING R3-pan-control-unreachable-after-switch), `f86414e`:
    - A strip's label is now a 44 dp button as wide as the strip ("Channel 3 settings" for a
      screen reader, which also hears "Pan left 30") showing the label and, under it, the pan:
      "C" at the centre, "L30" / "R20" for the share toward a side (L100 hard left, R100 hard
      right; the stored pan is 0..100 with 50 the centre, as the server reads it). It opens a
      bottom sheet (`ChannelSheet`, Material 3 `ModalBottomSheet`, skipping the half-open state)
      for that channel: a header with the channel, its group and "Your channel" when it is the
      musician's, the pan, the level (a `HorizontalFader`), wide Mute and Solo keys that read
      "Muted" / "Soloed" while on, and Done. It closes with Done, a swipe down, a tap outside it or
      back, and it closes if the server stops announcing the channel. Which sheet is open is the
      Mix screen's own state (`rememberSaveable`).
    - `PanControl` (new atom): 52 dp tall touch area across the sheet, a centre mark, the fill from
      the centre to a 32 dp thumb, and L, C and R marks under it; a slider for a screen reader
      whose value is spoken as "Left 30", "Centre" or "Right 20". It runs on
      `HorizontalFaderGesture`, which (with `FaderGesture`) now takes a `FaderScale`: `LevelScale`
      (the existing even mapping, the default) or `PanScale`, whose pure `panValueAt` /
      `panPositionOf` hold the middle tenth of the travel at the centre (the detent) and spread
      the rest evenly, so every pan is still reachable and the ends are hard left and hard right.
      Screen-reader steps ignore the detent.
    - `MonitorActions.setChannelPan` added and bound (the ViewModel already had the intent);
      `MixScreen` takes `onChannelPan`.
    - The Mix screen does not grow: the label button is 44 dp where the label was one line of
      about 17 dp, and the strip's top inset drops from 10 to 6 dp, so a channel strip's fader is
      212 dp instead of 236 (`VerticalFader` takes a `height`; group strips keep 236). The strip
      comes out the same height or up to a few dp shorter, depending on the monospace font.
    - Previews: the sheet at the centre (your channel), hard left and hard right (muted); the pan
      control; the sample mix now has varied pans, so the Mix previews show them on the strips.
    - RED: the 21 new pure tests (17 `PanScaleTest`, 4 `ChannelSheetTextTest`) against `TODO`
      stubs: 21 run, 20 failed with `NotImplementedError`; the one pass,
      `aScreenReaderStepIgnoresTheDetent`, never consults the scale. The new ViewModel test
      `aPanFromTheScreensIsKeptAndSentAsAMix` binds through `MonitorActions` and passed once the
      interface had the intent (it guards the binding). The existing `FaderGestureTest` and
      `HorizontalFaderGestureTest` stayed green on `LevelScale`. GREEN: all pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 34 result files, 324 tests (302 plus 22), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,346,840 bytes. `cargo fmt --all -- --check`: clean.
    - About 920 authored added lines, over the ~400 heuristic: a new atom and a new sheet with
      their previews, the pure mapping and 22 tests, which only make sense together.
    - No Compose UI tests (they need instrumentation): the sheet, the label button and the strip
      height need the live check.
  - Paste truncation in name fields (WARNING R3-keptedit-truncates-existing-text-on-mid-insert),
    `cc65cef`:
    - `keptEdit` kept the first `maxLength` characters of the whole new text, so a paste at the
      start or in the middle of a name that passed the limit cut off the end of the name (limit
      10, "Carlos", "Escudero " pasted at the start gave "Escudero C"). It now works like the old
      `InputFilter.LengthFilter`: it finds the edit (the selection, or nothing at the cursor,
      replaced by the pasted text; or, when the selection does not explain it, what lies between
      the start and the end both texts share, never splitting a two-unit character), keeps the
      text already there, cuts only the inserted part to the room left (one unit shorter rather
      than split an emoji), and puts the cursor after the kept part. The pure rule takes a new
      `FieldText` (text and selection); the plain-text form stays for callers without a cursor.
      Unchanged: an edit that fits or does not lengthen the text is kept whole, and at the limit
      a key or paste does nothing.
    - RED: 9 new `MixLinkTextFieldTest` tests (pastes that fit at the start, middle and end;
      overflowing pastes at the start, middle and end; over a selection; at the limit; an emoji
      at the cut; and the plain-text form found by comparison, twice) against a `TODO` stub for
      the `FieldText` rule and the unchanged plain-text rule: 18 run, 9 failed (7
      `NotImplementedError`; 2 assertions on the old rule, one of them "Escudero C" for
      "EscuCarlos"). GREEN: all 18 pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 34 result files, 333 tests (324 plus 9), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,363,224 bytes. `cargo fmt --all -- --check`: clean.
  - The fake receiver follows the real launch gating (SUGGESTION
    R3-fake-receiver-drops-flag-unconditionally), this commit; tests only:
    - `FakeReceiver` lowered the shared session flag on every stop and never counted as alive,
      while the real receiver (since `538f990`) lowers it only while its launch is the latest and
      counts as alive until its stop report has been handled. The fake now runs the real
      `ReceiverLaunches`: each launch is a loop that is alive until its report, a loop ends
      through `ReceiverLaunches.finished`, and `endOnItsOwn` / `finishStopping` take a
      `beforeReport` hook that runs between the flag drop and the report, where the main thread
      can still act. The `alive` switch is gone.
    - `theLastSessionsLateStopReportCannotEndTheNextSession` no longer starts a session before the
      last one's report, which the real receiver now prevents; it tests the race that is left: a
      Connect right after Stop is refused until the stopped loop has reported, and the session
      started afterwards ends only with its own loop. `connectWaitsForThePreviousReceiverToEnd`
      reaches the refusal the real way (Stop, then Connect before the report), and
      `aReceiverThatEndedOnItsOwnBeforeClearingAndReportsAfterTouchesNothing` clears the
      ViewModel through the hook instead of lowering the flag by hand.
    - New `aLoopEndingWhileTheMusicianStopsAndReconnectsEndsOnlyItsOwnSession` combines the real
      gating with the ViewModel's stop counting: a loop that ends on its own drops the flag, Stop
      and Connect land before its report (Stop does nothing, Connect is refused), the report ends
      its session once, and the next session's report, after Stop, ends nothing again.
    - RED: a probe fake with the old contract (flag dropped unconditionally, never alive; only the
      hook added so the tests compile; not committed): 92 `ui.state` tests run, 3 failed (the new
      test, the adjusted race test and the rewritten refusal test). GREEN with the real gating:
      all 92 pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 34 result files, 334 tests (333 plus 1), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,363,224 bytes (tests only). `cargo fmt --all --
      --check`: clean.
  - `d834256` is the commit above.
- T5 (route: delegated direct, one writer, with the launcher icon and the T4b review follow-ups
  as further work-unit commits). The fonts, this commit:
  - The user approved the download; the parent fetched both fonts from the official
    github.com/google/fonts repository (SIL Open Font License 1.1, TrueType) and verified them. The
    repository ships only variable fonts now, so each face is one file:
    `res/font/ibm_plex_sans_variable.ttf` (IBM Plex Sans, axes `wdth` and `wght`, 537,244 bytes)
    and `res/font/jetbrains_mono_variable.ttf` (JetBrains Mono, axis `wght`, 187,208 bytes),
    copied byte for byte (same SHA-256 as the downloads; "Plex" is a Reserved Font Name, so the
    files are not modified or renamed inside). The licenses ship with the app in
    `assets/licenses/IBMPlexSans-OFL.txt` and `assets/licenses/JetBrainsMono-OFL.txt`, as the OFL
    asks; git stores the IBM text with LF line ends (`core.autocrlf`), the words are unchanged.
  - `MixLinkFonts.Text` is IBM Plex Sans at 400, 500 and 600 and `MixLinkFonts.Numbers` is
    JetBrains Mono at 500 and 600 (`TextWeights`, `NumberWeights`): the weights the styles use.
    The new `variableFontFamily(resId, weights)` makes one `Font` per weight from the one file,
    each with `FontVariation.Settings(FontVariation.weight(n))`, so a style gets the font's own
    weight, never the nearest one or a synthesised bold (minSdk 29 reads variable fonts). That
    `Font` overload is still `@ExperimentalTextApi` in Compose 1.7, so the function opts in; the
    stable overload would set the same axis by default, but not visibly. The type scale is
    unchanged.
  - RED: the 4 new `MixLinkFontsTest` tests (a variable family sets each font's weight axis; the
    text face is IBM Plex Sans at 400/500/600; the number face is JetBrains Mono at 500/600; every
    style asks for a weight its face bundles) against a `TODO` `variableFontFamily` and the system
    faces: 4 run, 3 failed (1 `NotImplementedError`, 2 assertions); the fourth is a guard that
    passes on both. GREEN: all 4 pass.
  - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD SUCCESSFUL,
    41 tasks executed; 35 result files, 338 tests (334 plus 4), 0 failures, 0 errors; no Kotlin
    warnings. Debug APK 26,750,115 bytes (was 26,363,224).
  - `gradle -p apps/android assembleRelease -x lintVitalAnalyzeRelease`: BUILD SUCCESSFUL; R8 and
    the resource shrinker keep both fonts (in the APK as `res/f1.ttf`, 537,244 bytes, and
    `res/Mb.ttf`, 187,208 bytes: release builds shorten resource paths) and both licenses.
    Unsigned release APK 2,888,008 bytes (was 2,390,604). `cargo fmt --all -- --check`: clean.
  - `e116aa3` is the commit above. The launcher icon (T6), this commit:
    - Adaptive icon, own artwork, vectors only: `mipmap-anydpi-v26/ic_launcher.xml` and
      `ic_launcher_round.xml` (the launcher applies the mask) with the background in the ground
      colour (`@color/mixlink_ground`), the foreground `drawable/ic_launcher_foreground.xml` and
      the monochrome layer `drawable/ic_launcher_monochrome.xml` for themed icons (Android 13 and
      later). The mark: the desktop wordmark's amber (`#f5a524`) rounded square, corners a quarter
      of its side as on the desktop, with two arcs of signal in the light text colour (`#e6edf3`,
      new `@color/mixlink_text`) leaving its right edge. On the 108 dp canvas it spans x 28 to
      78.75 and y 37.1 to 70.9, so its farthest point is about 25 dp from the centre, inside the
      33 dp safe radius. The manifest sets `android:icon` and `android:roundIcon`.
    - No pure logic, so no test. Checked: an SVG copy of the layers rendered in headless Chrome
      (full canvas with the safe circle, a round mask at 144 and 48 px, a themed tint) shows the
      mark whole inside the safe circle; `aapt2 dump badging` of the debug APK reads
      `application: label='MixLink' icon='res/mipmap-anydpi-v26/ic_launcher.xml'`.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 35 result files, 338 tests, 0 failures, 0 errors; no Kotlin
      warnings. Debug APK 26,752,765 bytes. `cargo fmt --all -- --check`: clean.
  - `e1a4e7c` is the commit above. The T4b review follow-ups (T4c; lineage
    `review-750e0cd8bc15bcc0`, approved, two suggestions), this commit:
    - The open channel sheet found its channel by position in the list
      (`channels.getOrNull(openChannel)`) while a strip reports its `channel.index`; the two agree
      today only because the list holds every channel in order. The lookup moved into the pure
      `sheetChannelFor(channels, openChannel)` (`ChannelSheetText.kt`), which finds the channel by
      index; when that channel is no longer announced it returns null and `MixScreen` closes the
      sheet, as before.
    - `keptEdit`: with a text already longer than `maxLength` (given from outside), a paste over a
      selection had no room, so the selection was deleted and nothing pasted ("Carlos Escudero",
      limit 10, "Es" selected, "XYZ" pasted gave "Carlos cudero"). The same happened at the limit
      when the one unit of room would split an emoji pasted over a one-character selection. Now an
      edit none of whose text fits changes nothing, selection included, as the KDoc says for the
      limit; the KDoc names the over-limit case too.
    - RED: 5 new tests (3 `ChannelSheetTextTest`: by index not position, a channel no longer
      announced, no channel open; 2 `MixLinkTextFieldTest`: over the limit, and an emoji at the
      limit, each over a selection) against the extracted position lookup and the unchanged
      `keptEdit`: 27 run, 3 failed with assertions (`null` for channel 5 of [2, 5]; "Carlos cudero";
      "Carlo"); the other two sheet tests are guards that pass on both. GREEN: all 27 pass.
    - `gradle -p apps/android :app:testDebugUnitTest assembleDebug --rerun-tasks`: BUILD
      SUCCESSFUL, 41 tasks executed; 35 result files, 343 tests (338 plus 5), 0 failures, 0
      errors; no Kotlin warnings. Debug APK 26,752,765 bytes. `cargo fmt --all -- --check`: clean.
    - `gradle -p apps/android assembleRelease -x lintVitalAnalyzeRelease` (with T5 and T6): BUILD
      SUCCESSFUL; `aapt2 dump badging` reads `icon='res/BW.xml'` (the adaptive icon, path
      shortened), and both fonts and both licenses are in the APK. Unsigned release APK 2,890,420
      bytes.
  - Next: the parent's live check on the S23 of the fonts and the icon (and the open T4 items),
    then the stacked pull request.
