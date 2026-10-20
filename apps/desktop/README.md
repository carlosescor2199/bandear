# MixLink desktop

The mixing console for the MixLink server. The engine (`apps/server`) captures the audio
interface and sends each phone its own mix; this app runs that engine and shows three screens:

- **Mix**: every musician's mix: channel strips (group, input meter, pan, fader, Mute, Solo),
  group levels and the master (level, level cap, Mute, output meters). A change on the desk
  reaches the phone at once, and a change on the phone shows here. The last tab, **Local mix**,
  is the engineer's own mix for this computer's output.
- **Record**: multitrack recording of the inputs, the takes, and the virtual soundcheck.
- **Network and system**: the phones (name, link health and its history, playback buffer,
  redundancy, remove, add by address), the session (named sessions and **Mix backups**) and the
  audio setup (driver, device, buffer, and **Local mix out**).

ASIO loads a kernel-mode driver, so the app never picks it: choose it under Network and system,
then pick the driver and start on it.

## Network health

Each phone's link is kept as a history of one-second samples (the last 10 minutes), so a bad
patch can be seen after it happened.

1. On Network and system, press **History** in a musician's Link cell (or click their row).
2. Pick a range: **1**, **5** or **10 min**. Hover the chart, or use the arrow keys, to read a
   second; **Show as table** lists every second, newest first.
3. Press History again, or **Close**, to close it.

| Line | What it means |
| --- | --- |
| Summary | The link's level (as in the table), the loss over the range, the round trip now and on average, the underruns, and when any the timeouts, seconds without reports and reconnects. |
| Loss (%) | The packets lost in each second. |
| Round trip (ms) | The control channel's ping to pong time, measured every 2 s, for any phone. |
| Jitter (ms) | How irregularly the packets arrive. Only from a phone with a newer app ("not reported by this phone" otherwise). |
| Cushion (ms) | The audio the phone holds back before playing, a redundancy hold included. Newer app only. |
| Shaded band | Seconds with no report from a connected phone. |
| Dashed "Reconnected" line | The phone connected again; its history is kept. |
| Ticks | Underruns (the phone ran out of audio) and receive timeouts, in their own lanes. |

A new engine starts every history empty.

## Phone buffer

The **Buffer** column sets how much audio a phone holds before playing.

| Choice | Behaviour |
| --- | --- |
| Auto (default) | The phone adapts its own cushion, from one to four packets, as before. |
| Fixed | Type 5 to 500 ms and press Enter (or leave the field; Escape reverts). The phone plays that much, rounded up to whole packets and clamped to what it can hold, and stops adapting. |

- The cell shows what the phone really plays ("plays 42.7 ms"), from its reports.
- A phone that is not connected gets the setting when it is back. The choice is kept in the
  session.
- A phone with an older app shows "needs a newer app" and no control; it keeps adapting.

## Redundancy

Turned on for a musician, each audio packet is sent twice, so a lost packet's copy can still
play.

| Copy | Cost | Covers |
| --- | --- | --- |
| Immediate (default) | 2× that musician's bandwidth, no added latency. Every phone takes it. | Isolated losses. |
| 1 to 4 blocks later | 2× bandwidth, plus the delay on that phone: about 5.3 ms a block at 256 frames and 48 kHz (2 blocks: +10.7 ms). | Wi-Fi bursts shorter than the delay. |

- A delayed copy needs a phone that holds packets back for it: other phones show the delays
  disabled with "needs a newer app".
- While on, the cell shows the copies sent and, for a phone that reports them, the packets
  recovered (packets a delayed copy filled in time). Immediate copies are never counted.
- The choice is kept in the session. Only that musician's bandwidth doubles.

## Log file

The engine and the desk write to a log file, so a release build (which has no console) can be
diagnosed.

| Topic | Behaviour |
| --- | --- |
| Where | `mixlink.log` in the app's log folder: on Windows `%LOCALAPPDATA%\com.mixlink.desktop\logs\`, on macOS `~/Library/Logs/com.mixlink.desktop/`. The `log_location` command answers `{ directory, file, failedLines }`, and the file's path is the log's first line. |
| Line | `2026-10-18T19:04:05.123Z WARN  [control] message`: UTC time, level, area (`engine`, `control`, `session`, ...). |
| Rotation | At 5 MB the file moves to `mixlink.log.1` (older ones to `.2` ... `.5`); five old files are kept. |
| Failure | A line the file cannot take goes to stderr, and is counted in `failedLines`. A rotation that fails keeps writing to the current file. |
| Development | Debug lines are written only in a development build, which also mirrors every line to stderr. |

## Mix backups

The musicians' live mixes are backed up as they change, so a crash loses at most a few seconds.

| Topic | Behaviour |
| --- | --- |
| When | 3 s after the mixes go still (at most 20 s after the first change), on engine stop and on exit. An unchanged backup is not written again. |
| Where | `backups/mixes-<UTC stamp>.json` in the app's config folder (on Windows `%APPDATA%\com.mixlink.desktop\`), written through a temporary file. The newest 10 are kept. |
| What | Each musician's name, client id, address and mix. Locks and limits are not in it: the session keeps them. |
| Restore | Network and system, **Mix backups**: pick a backup, **Restore**, confirm. Each mix goes to that musician if they are connected now; the others keep their own. A damaged backup is listed with its problem and cannot be restored. |

A phone that reconnects keeps its own opening mix: restoring is always the engineer's choice.

## Demo source

The engine can run with no audio interface, to show the app or test phones.

1. Under Network and system, choose the driver **Demo (no interface)** and start.
2. The top bar shows **Demo source — no interface in use**, and the window's title says so.

| Topic | Behaviour |
| --- | --- |
| Sound | A generated four-bar loop at 120 BPM on 8 inputs (kick, snare, hi-hat, bass, keys, pad, a vocal-like line and room noise), each at -18 dBFS. The same on every run. |
| Format | 8 channels, 48 kHz, 32-bit float, in blocks of the buffer chosen. |
| Allowed | Phones, mixes, the local output and recording (the take's device is "Demo (no interface)"). |
| Refused | The virtual soundcheck, and switching to any other device: start on another driver instead. |
| Default | Never chosen on its own, even with no interface; a machine with no input is told it can pick it. |

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
