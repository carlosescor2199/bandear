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
for connecting; IBM Plex Sans for text and JetBrains Mono for numbers (system fonts until T5).

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
  Compose and deleting the XML layout; the saved-banks empty-state bug; bundling the two fonts.

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
- [ ] T4 (switch): `MainActivity` hosts Compose; `activity_main.xml` deleted; the saved-banks empty
  state fixed; live check on the S23.
- [ ] T5 (fonts): bundle IBM Plex Sans and JetBrains Mono (OFL), after the user approves the
  download.

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
- T5: parent, after the download permission.

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
