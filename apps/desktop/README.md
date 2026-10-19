# MixLink desktop

The mixing console for the MixLink server. The engine (`apps/server`) captures the audio
interface and sends each phone its own mix; this app runs that engine and shows three screens:

- **Mix**: every musician's mix: channel strips (group, input meter, pan, fader, Mute, Solo),
  group levels and the master (level, level cap, Mute, output meters). A change on the desk
  reaches the phone at once, and a change on the phone shows here. The last tab, **Local mix**,
  is the engineer's own mix for this computer's output.
- **Record**: multitrack recording of the inputs, the takes, and the virtual soundcheck.
- **Network and system**: the phones (name, link health, remove, add by address) and the audio
  setup (driver, device, buffer, and **Local mix out**).

ASIO loads a kernel-mode driver, so the app never picks it: choose it under Network and system,
then pick the driver and start on it.

## Local mix

The local mix plays on an output of this computer, so the engineer can listen without a phone.

1. Under Network and system, pick an output pair in **Local mix out**. The default is
   **None (lowest load)**: nothing is opened.
2. On the Mix screen, open the **Local mix** tab and mix it like any musician's mix (no lock,
   level cap or limits).

| Topic | Behaviour |
| --- | --- |
| Outputs offered | Under WASAPI, every output device. Under ASIO, only the outputs of the driver that captures. |
| Rate | 48 kHz only. |
| Clock drift | A WASAPI output runs on its own clock: the engine pads with silence or drops frames to stay in step (there is no resampler), and the audio setup shows both counters. |
| Kept | The mix and the output choice are kept in the session. At start the output reopens only if the driver still offers that exact device and pair; otherwise it stays off and says why. |
| Failure | An output error stops only the local output, with its reason; the phones are not affected. |

## Recording

Every armed input records to its own file while the show runs; the phones hear exactly what they
heard before.

1. On the Record screen, arm the tracks (and **Also record the local mix** under Settings if you
   want it).
2. Press Record, add markers while it records, press Stop.
3. The take is listed under Takes; **Open folder** shows it in the file manager.

| Topic | Behaviour |
| --- | --- |
| Where takes go | The recordings folder in Settings. The default is your Music folder + `MixLink\Recordings` (if there is no Music folder, the app's data folder; never inside the app's config folder). |
| Take folder | `<YYYY-MM-DD_HHMMSS>_<take name>`, in local time; ` (2)`, ` (3)`... when that name exists. A blank name is `Take`. |
| Files | One mono file per armed input (`01 Vocal.wav`), one stereo file per linked pair (`03-04 Keys.wav`, either side arms it) and `Local mix.wav` when asked. Tracks are written before any processing. |
| `take.json` | Written when the take starts and again when it ends (through a temporary file and a rename): tracks with their frames, start time, device, rate, format, markers, dropped frames and how the take ended. |
| Formats | WAV, 24-bit integer (default) or 32-bit float, 48 kHz. A file past 4 GB is written as RF64. |
| Disk | The screen shows the minutes left on the folder's disk for the armed tracks, with a warning under 30 minutes and a stronger one under 10 (the free space is read on Windows). |
| Device switch | A device or driver change is refused while a take records: stop the take first. A take never mixes two devices. |
| Disk error | Only the take stops: the failed file keeps what its header last counted, the other files are finished, and the take ends as a disk error with the reason. |
| Crash or power loss | Headers are refreshed every second and the files synced to the disk every 5 seconds, so the files stay playable: an app crash loses about the last second, a power loss about the last 5 seconds. The take is listed as interrupted, with the frames its files hold. |

## Virtual soundcheck

A take plays back into the mixer in place of the live inputs: the phones, the local output and
the meters hear it as they would hear the band, so mixes can be set with nobody playing.

1. On the Record screen, press **Soundcheck** on a take, or pick it in the **Virtual soundcheck**
   card and press Start. The transport's Play starts the card's take, else the last take.
2. Pause, play, move with the slider, go back to the start or loop it.
3. Press **Back to live inputs** to end it. The input device stayed open, so live audio returns
   at once.

| Topic | Behaviour |
| --- | --- |
| Which takes play | Only take folders of the recordings folder, recorded at 48 kHz (there is no resampler). |
| Channel layout | The take's tracks must fit the device's inputs and linked pairs now; a take that does not fit is refused and says why. Inputs the take did not record are silent; its local-mix track is not played. |
| Pacing | The local output's clock when one plays, else a timer. Disk shortfalls are padded with silence and counted as dropouts. |
| End of take | Without loop it pauses at the end ("At the end"); Play again starts it from 0:00. A seek plays a take that stopped at its end and keeps a paused take paused. |
| While it plays | Recording, a device switch and a driver change are refused, and the SOUNDCHECK chip shows on every screen. A soundcheck cannot start while a take records. |
| Read error | The take falls silent and says why; only Back to live inputs is left. |

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
