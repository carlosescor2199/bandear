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
- [ ] T2 (state): `MonitorViewModel` holding connection, discovered servers, mix, saved mixes and
  diagnostics; pure reducers with JVM tests; `MainActivity` logic moved behind it.
- [ ] T3 (screens): Connect, Mix, Saved mixes and Settings composed from the atoms, with previews.
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

## Verification

- T1 RED: the 24 new tests against `TODO` stubs: 24 run, 24 failed with `NotImplementedError`.
- T1 GREEN: the same 24 pass.
- T1 `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL; 0 failures,
  0 errors; no Kotlin warnings. Corrected count (parent recount with `--rerun-tasks`, 21 result
  files): 139 tests, that is 115 before plus the 24 new ones. The writer's report of 155 (131 + 24)
  was a miscount.
- T1 `cargo fmt --all -- --check`: clean (no Rust changes).
