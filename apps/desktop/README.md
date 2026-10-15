# MixLink desktop

The mixing console for the MixLink server. The engine (`apps/server`) captures the audio
interface and sends each phone its own mix; this app runs that engine and shows two screens:

- **Mix**: every musician's mix: channel strips (group, input meter, pan, fader, Mute, Solo),
  group levels and the master (level, level cap, Mute, output meters). A change on the desk
  reaches the phone at once, and a change on the phone shows here.
- **Network and system**: the phones (name, link health, remove, add by address) and the audio
  input (driver, device, buffer).

ASIO loads a kernel-mode driver, so the app never picks it: choose it under Network and system,
then pick the driver and start on it.

## Layout

| Path | What it is |
| --- | --- |
| `src-tauri/` | The Tauri shell: the commands and the event pump around the engine. |
| `ui/index.html`, `ui/app/` | The console: plain ES modules and custom elements, no bundler. |
| `ui/app/lib/` | The pure logic (state, mix sync, view models), tested with `node --test`. |
| `ui/app/components/` | Presentational elements: atoms (fader, meter, ...) and organisms (strips, tables). |
| `ui/app/screens/` | The screens, the containers that use the console app and the organisms. |
| `ui/dev/` | The component gallery and a small static server for browser previews. |

## Run the app

From the repository root:

```sh
cargo run -p mixlink-desktop
```

The engine is built with ASIO support by default, which needs LLVM/Clang at build time; on a
machine without it, build the engine with `--no-default-features` (see `apps/server/Cargo.toml`).

## Preview in a browser

The console runs in a browser against a simulated engine (six channels, four phones, moving
meters), so the screens can be checked without an audio interface. ES modules do not load from
`file://`, so serve the folder:

```sh
node apps/desktop/ui/dev/serve.mjs 5199
```

Then open <http://127.0.0.1:5199/?fake> for the console and
<http://127.0.0.1:5199/dev/gallery.html> for every component in its states. Without `?fake`, a
browser shows that the console needs the app.

## Tests

```sh
node --test apps/desktop/ui/tests      # the console's logic
cargo test -p mixlink-desktop          # the desktop commands, DTOs and event pump
cargo test -p personal-monitoring      # the engine
```

Node 22 or later runs the UI tests; there are no npm dependencies. `node --test` given the
`tests` folder loads `tests/index.js`, so a new suite must be imported there.
