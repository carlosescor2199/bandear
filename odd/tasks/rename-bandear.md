# Rename MixLink to BandEar

## Objective

The product is called BandEar from now on: the desktop app, the phone app, the installers, the
release and the docs. A desk that already ran as MixLink keeps its sessions, backups and takes.

## Decisions (user, 2026-10-21)

- Full rename, technical identifiers included: Tauri identifier `com.bandear.desktop`, crate
  `bandear-desktop`, Android `applicationId`/`namespace` `com.bandear.android`.
- The desktop brings a MixLink desk's data over on its first start (data migration).
- The GitHub repository is now `carlosescor2199/bandear`.
- The server crate `personal-monitoring` keeps its name (internal); only its user-facing text
  changes. No wire protocol byte or field changes.
- Icons stay as they are; a new logo comes later. `vendor/cpal` is untouched.

## Scope

- Desktop (Tauri): product name, identifier, window titles, crate name, every user-facing string in
  the Rust desk and the UI, the log file `bandear.log`, the default recordings folder
  `BandEar\Recordings`.
- Desktop migration from the `com.mixlink.desktop` folders.
- Server: user-facing strings only.
- Android: package, namespace, applicationId, theme names, labels.
- CI release workflow, root README, desktop README, PRD.

## Constraints

- Build safety: `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`, one build at a time, no `--release`,
  gradle `--max-workers=2 --no-daemon`, full suites once at the end.
- Wire compatibility between old and new builds (PMON magic, control messages, discovery beacon
  magic `MLNK`) is preserved.

## Tasks

- [x] T1 Rename the desktop app (Tauri config, crate, UI, Rust strings, log file, recordings
  folder) and its tests. Route: inline (mechanical, scripted replacements). Commit `df28a3c`
  (41 files, +148/-148).
- [x] T2 Desktop data migration from `com.mixlink.desktop` (RED then GREEN). Route: inline (one new
  module plus wiring). Commit `eebb31e` (4 files, +718/-10).
- [x] T3 Rename the phone app (package move, applicationId, theme, labels) and its tests. Route:
  inline (mechanical `git mv` + scripted replacements). Commit `7b425c1` (113 files, +755/-755).
- [x] T4 Server user-facing strings, CI release names, docs (README, desktop README, PRD).
  Route: inline. Commits `3dd472b` (server, +9/-6), `6f5a111` (CI, +4/-4), `75b3515` (docs,
  +29/-26).
- [x] T5 Full verification and leftover audit (see Verification).
- [x] T6 Apply the approved BandEar logo (see Logo). Route: delegated writer. Commits `bfddda7`
  (desktop icons, sidebar mark, favicon, brand sources), `8cec6c5` (Android adaptive icon) and
  the README/doc commit.

## Migration design (T2)

`apps/desktop/src-tauri/src/migration.rs`, called from `run()`'s setup before `SessionStore::open`.

- Old folders: the sibling of each new Tauri folder named `com.mixlink.desktop` (Tauri names the
  config and local data folders `<base>/<identifier>`).
- Runs when the new config folder has no `session.json` and no `mixlink-migration.txt`, and the old
  config folder exists.
- Copies `musicians.json`, `sessions/`, `backups/`, then `session.json` last; each file through a
  `.migrating` temporary file and a rename; an existing new file is never replaced (listed as kept).
  Nothing else (damaged files set aside, logs) comes over. The old folders are only read.
- Recordings: the old default folder is worked out exactly as the old build did
  (`default_recordings_folder_for("MixLink", music, old local data, old config)`). When it holds a
  take (a subfolder with `take.json`), the new default folder does not exist, and the session's
  `recording.folder` is null or absent, the migrated `session.json` gets `recording.folder` set to
  the old folder (a minimal `{"version":1,"recording":{...}}` when the old desk had no session). A
  folder the engineer chose is kept as is.
- Writes `mixlink-migration.txt` when it finished without a failure, so it runs once even for a
  desk with no session; after a failure it runs again next start, still never replacing files.
- Logs every copied and kept file, the recordings folder kept and any failure (area `migration`).

RED evidence: with `bring_over` stubbed to `None`, `cargo test -p bandear-desktop --lib migration`
gave `8 failed, 3 passed` (the three that expect `None` or test the sibling path). GREEN: 11 passed.

## Acceptance criteria

- `rg -i mixlink --glob '!target' --glob '!vendor' --glob '!odd'` shows only intentional leftovers.
- Migration copies once, never overwrites, skips when the new session exists, leaves the old
  folders untouched, and keeps takes in the old default recordings folder reachable.

## Checks

- `cargo test -p personal-monitoring`
- `cargo test -p bandear-desktop`
- `cargo check -p bandear-desktop --all-targets`
- `node --test apps/desktop/ui/tests`
- gradle `:app:testDebugUnitTest`, `:app:assembleDebug`

## Verification (run once each, at the end)

- `cargo test -p personal-monitoring`: 648 passed, 0 failed.
- `cargo test -p bandear-desktop`: 352 passed, 0 failed (11 of them the migration).
- `cargo check -p bandear-desktop --all-targets`: finished, no warnings; `Cargo.lock` unchanged.
- `node --test apps/desktop/ui/tests`: 619 passed, 0 failed.
- gradle `:app:testDebugUnitTest`: BUILD SUCCESSFUL, 40 suites, 517 tests, 0 failures.
- gradle `:app:assembleDebug`: BUILD SUCCESSFUL.

## Intentional leftovers

`rg -i mixlink --glob '!target' --glob '!vendor' --glob '!odd'`:

- `migration.rs`, `lib.rs` comment, `recording.rs` doc: the old identifier `com.mixlink.desktop`,
  the old `MixLink\Recordings` folder, the marker `mixlink-migration.txt` and their log lines.
- `apps/desktop/README.md`: the "Coming from MixLink" section, and the `MixLink patch` markers in
  `vendor/cpal` (vendor stays untouched).
- `apps/server/src/discovery.rs`: why the beacon magic `MLNK` stays (wire compatibility). PMON and
  the control messages carry no product name.
- Root `README.md`: the note that BandEar used to be called MixLink.

## Logo (T6)

- Sources in `docs/brand/`: `bandear-mark.svg` (the mark: six round-capped strokes, `#FFBF43`),
  `bandear-icon.svg` (the app icon: the mark on a `#192223` rounded square) and
  `bandear-preview.png` (sizes preview). The in-context mockup was not kept.
- Desktop icons: `cargo tauri icon ../../docs/brand/bandear-icon.svg` from `apps/desktop`
  (tauri-cli 2.12.1 reads SVG) regenerated `icon.ico`, `icon.icns`, `icon.png`, `32x32.png`,
  `128x128.png`, `128x128@2x.png` and the Windows Store `Square*Logo.png`/`StoreLogo.png`. The
  CLI's extra `64x64.png` and its `android/` and `ios/` folders were deleted (the phone app is
  native). `icon.png` stays: it was already tracked and is Tauri's fallback window icon off Windows.
- Console: `apps/desktop/ui/app/brand/bandear-mark.svg` (a copy of the mark, shipped with
  `frontendDist`) replaces the amber square in the sidebar brand row and is the favicon of
  `index.html` and `dev/gallery.html` (no more `/favicon.ico` 404 on the dev server).
- Android: the adaptive icon's foreground and monochrome layers are the mark's six paths in a
  1024 viewport, scaled 0.82 around the mark's centre into the 66 dp safe circle (farthest point
  ~303 of 313 units), stroke 46, on the new `bandear_icon_ground` (`#192223`); the mark's colour
  is `bandear_mark` (`#FFBF43`). There are no legacy PNG mipmaps (minSdk 29, `mipmap-anydpi-v26`
  covers every launcher reference: manifest `icon` and `roundIcon`).
- README: the header image is `docs/brand/bandear-mark.svg` at width 120.
- The empty `com/mixlink` source folders (main and test) were deleted.
- Not done (outside the logo task's edit surface): the phone's Connect screen
  (`ConnectScreen.kt`, `BrandHeader`) still draws the amber square next to "BandEar".
- Checks: `node --test apps/desktop/ui/tests` 619 passed, 0 failed;
  `cargo build -p bandear-desktop` finished (icons embedded); gradle `:app:assembleDebug` BUILD
  SUCCESSFUL; browser (`desktop-ui`, `?fake`): the sidebar shows the mark beside "BandEar", the
  favicon loads (200), no console errors.

## Notes

- Android: the new `applicationId` makes BandEar a new app beside MixLink on a phone; Android keeps
  each app's data apart, so the phone's saved mixes and client identity start fresh (the desk sees
  a new client id; its label can be set again). The preference files were renamed to
  `bandear_identity` and `bandear_banks` for the same reason: there is nothing to carry over.
- Windows: the new identifier installs BandEar as a separate app; an old MixLink install stays
  until it is uninstalled.
- Empty `com/mixlink` source folders remained in the working tree after the `git mv` (git does
  not track folders); they were deleted in T6.
- The optional one-line rename note in older `odd/tasks/*.md` was skipped (28 files, noisy).

## Progress

- 2026-10-20: feature document created; T1-T5 done; all checks green.
- 2026-10-20: T6 done: the approved logo on desktop, console, Android and README; checks green.
