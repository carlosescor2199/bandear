# Android Auto-Reconnect

## Objective

When a musician's phone loses its control connection (a Wi-Fi blip, a server restart), the app
reconnects on its own and the musician keeps their mix, without touching Stop and Start.

## Problem and why

Found in the live test of the link-health feature on a Galaxy S23: after turning Wi-Fi off and on,
the phone never came back. The control channel only updates its state on failure
(`ControlChannel.kt` `onFailure` / `onClosed`), Start stays disabled while the audio receiver runs,
and the only way back is Stop then Start. On stage a five-second Wi-Fi blip would leave the musician
without control until someone touches the phone.

Three facts from the code make it worse:

- The channel does not keep the server host or control port, so it has nothing to reconnect with
  (only the registration message and the UDP port are stored).
- `OkHttpClient()` runs with no `pingInterval`, so the phone never notices a dead connection on its
  own; it only finds out when a write fails.
- One shared `WebSocketListener` serves every connection with no check of which socket a callback
  came from, so a late `onClosed` from an old socket can null a newer connection and mark it closed.

## Design decisions

- **The phone owns reconnection.** While a session is running (`running` is true) and the control
  connection fails, closes or is closed by the server, the channel schedules a new connection on its
  existing `controlExecutor`, with a capped backoff: 0.5 s, 1 s, 2 s, 4 s, then every 5 s. The
  backoff resets once a connection hears from the server (its first message, the `config` sent
  right after connect), not on open, so a server that accepts and drops at once still backs off.
  Stop, the receiver dying on its own, and `onDestroy` cancel any pending attempt. The control
  error is shown once per outage, not once per failed retry.
- **Nothing new on the server for reconnecting itself (T1).** Every open already sends `register`
  then `mix`, and the server
  keeps the musician's target and mix for its 15 s grace period after the heartbeat or a close; a
  later reconnect simply re-registers and re-sends the phone's own mix.
- **The phone detects dead links itself:** OkHttp gets a 5 s `pingInterval` (2 s in T1, raised
  after review), so a half-open connection fails within about 10 s, inside the server's own 10 s
  silence limit, instead of hanging.
- **Callbacks are tied to their socket:** events from a socket that is no longer the current one
  are ignored.
- **The musician sees it:** a `RECONNECTING` control state with its own status label.
- **A phone that comes back from a new IP keeps its row (T3).** The server keys musicians by IP,
  so when DHCP hands the returning phone a new address it used to become a second row while the
  old one showed as lost until the heartbeat (10 s) and the grace period (15 s) removed it. The
  phone now generates a random client id once (a UUID), keeps it, and sends it in every
  `register` as `client_id`. When a `register` carries an id that an entry at another IP holds,
  the server moves that entry to the new address (peer IP plus `udp_port`, still never taken from
  the message): mix state, counters, report slot and list position are kept, the old address
  leaves the registry at once, and any other entry still keyed to the new IP gives way (one entry
  per IP). The link report follows the existing rule: the first `register` of a control session
  clears it. The old IP's leftover connection finds no entry at its IP when it closes, so it
  neither marks the moved entry disconnected nor starts its grace period. Without `client_id`
  (older phones) the behaviour is exactly as before. An id is accepted when it is 1 to 64 ASCII
  letters, digits and hyphens; anything else (another JSON type included) is read as no id and
  never fails the registration. Every registration records the id it announced, or none, so a row
  belongs to whoever registered from its IP last.
- **Trust model unchanged (T3).** The control channel stays a trusted-LAN protocol. A client id is
  random, never displayed and only travels on the LAN, so it is not guessable by another phone; a
  client able to read LAN traffic could already claim any musician by IP before this change.

## Scope

- `ControlChannel`: stored host and control port, pure reconnect policy, scheduled attempts,
  socket-identity guard, `pingInterval`, cancellation.
- `MainActivity`: cancel on Stop, on `onStopped` (also closing the control channel) and on destroy.
- Status label for `RECONNECTING`.
- T3, server: optional `client_id` in `register`, entry move in `TargetRegistry::register`, the
  move logged on the control connection.
- T3, Android: a client id generated once and kept in `SharedPreferences`, sent in every
  `register`.

## Out of scope

- Following a server whose IP changed (discovery stays stopped during a session).
- Network-change callbacks (`ACCESS_NETWORK_STATE`); the 5 s cap bounds the wait.
- The mobile UI redesign (its own feature, next).

## Constraints

- Clean-room design.
- The audio receiver is not restarted; only the control channel reconnects.
- Pure, JVM-testable decisions (JUnit 4 only; no Mockito or Robolectric in the project).
- Keep technical artifacts in English.

## Tasks

- [x] T1 (android): reconnect on control failure with a capped backoff, socket-identity guard,
  client `pingInterval`, `RECONNECTING` state and label, cancellation on Stop, receiver death and
  destroy. RED-first JVM tests for the pure policy, the guard and the label. Commit: `2d73b7a`.
- [x] T1b (android, review follow-ups): backoff resets on the first server message instead of on
  open; one control error per outage; client ping 5 s; a transport and scheduler seam so the
  channel's state machine runs under JVM tests with fakes. Commit: this commit.
- [x] T2 (live): verify on the S23 with the desktop: Wi-Fi off then on reconnects on its own and the
  musician turns green again with their mix; Stop during reconnection stops retrying. Done: a
  Wi-Fi blip and a desktop engine restart both reconnected on their own. The Wi-Fi return came
  with a new DHCP address, which produced a duplicate row (see T3).
- [x] T3 (server + android): a phone that returns from a new IP keeps its musician row, through a
  stable client id announced in `register`. Commits: `bbe4070`, then this commit.
  - [x] T3a (server): optional `client_id` parsed leniently; a known id from a new IP moves the
    entry; the old IP's leftover connection leaves the moved entry alone. Commit: `bbe4070`.
  - [x] T3b (android): generate and keep the client id, send it in every `register`. Commit: this
    commit.
- [ ] T4 (live): on the S23, a Wi-Fi blip that changes the phone's IP leaves one row on the
  desktop, with its mix, and the server logs `..., moved from <old address>`.

## Follow-ups

- From review `review-f90e423bce7de49c` (test gaps, not defects; Android, not scheduled):
  - The 5 s ping value is only asserted as a constant; nothing checks it reaches the OkHttp client.
  - `ExecutorControlScheduler` dropping work after `shutdown` has no test.
  - The outage flag resetting on `connect` after a Stop has no test.

## Acceptance criteria

- After a Wi-Fi blip, the phone reconnects on its own within about 5 s of the network returning,
  re-registers and keeps its mix; the desktop shows it green again.
- Restarting the desktop server: the phone reconnects once the server is back.
- Pressing Stop while reconnecting stops all attempts; nothing reconnects after Stop or destroy.
- A late callback from an old socket never affects the current connection.
- A phone that returns from a new IP (DHCP) keeps one row on the desktop, with its mix; no second
  row appears and no row is left showing as lost. Older phones without a client id behave as
  before.

## Acceptance checks

- `gradle -p apps/android :app:testDebugUnitTest assembleDebug` (with the JAVA_HOME and
  ANDROID_HOME used for the link-health feature)
- `cargo test -p personal-monitoring` (regression for T1/T1b; new tests for T3)
- `cargo test -p mixlink-desktop`, `cargo fmt --all -- --check`,
  `cargo check --workspace --all-targets` (T3)

## Route

- T1: delegated direct, one writer (2+ non-trivial Android files).
- T1b: delegated direct, the same writer (seam plus behaviour fixes across 2+ Android files).
- T2: parent with the user on the device.
- T3: delegated direct, one writer (2+ non-trivial files across the server and the Android app),
  two work-unit commits: server, then Android.

## Delivery forecast

About 300 to 400 authored changed lines, mostly tests; one pull request stacked on
`feat/link-health-6-heartbeat` (#6). Strategy: `ask-on-risk`.

T3 was added after the forecast: about 520 authored changed lines for the server commit (mostly
tests) plus the Android commit, so the branch is now well over the ~400 forecast. The chain
strategy for it is still to be chosen.

## Progress

- Branch `feat/android-auto-reconnect` created from `9a37013` (top of the link-health chain).
- T1 done (route: delegated direct, one writer). What landed:
  - `ReconnectPolicy.kt` (new): `ReconnectPolicy` (0.5 s, 1 s, 2 s, 4 s, then 5 s forever; `reset`
    after an open; `shouldReconnect(sessionRunning, stopped)`) and `ConnectionGenerations`, the
    socket-identity guard: every attempt takes a generation, a callback acts only while its
    generation is current, and `end` claims the end of a connection exactly once.
  - `ControlChannel`: stores host and control port at `connect`; one listener per attempt carrying
    its generation; `onFailure`, `onClosing` (answered with a close) and `onClosed` of the current
    connection set `RECONNECTING` and schedule the next attempt on `controlExecutor` while the
    session runs and was not stopped; a stale socket is only ever closed, never used; OkHttp client
    built with a 2 s `pingInterval`; `close` cancels the pending attempt, invalidates every
    generation and closes the socket even while it is still opening; executor submissions drop
    work after `shutdown` instead of throwing.
  - `MainActivity.onStopped` now closes the control channel and sets `IDLE` when the session is
    over, so a receiver that ends on its own stops the retries too. Stop and `onDestroy` already
    call `close`.
  - `RECONNECTING` labels: "reconnecting - mix held at the server" (server owns the mix) and
    "reconnecting - mixing on this device" (it does not), matching the existing lost-channel wording.
- Deviations and notes:
  - The guard is a generation counter rather than a comparison of `WebSocket` references, because a
    failure can arrive before `newWebSocket` even returns its socket; the generation exists first.
  - A server that is unreachable when Start is pressed is now retried too (shown as reconnecting)
    instead of staying "unavailable"; this is what lets a restarted server be picked up.
  - Pre-existing, not changed: `PmonPlayer` sets `running` to false in its `finally` after a new
    session may already have started; `onStopped` only closes the channel when `running` is false.
  - About 450 authored changed lines, over the ~400 heuristic, mostly tests and KDoc.
- T1 review: native review of `2d73b7a` (lineage `review-7c305b3072d3aed3`) approved and
  acknowledged, with four advisories. The user accepted all four as T1b.
- T1b done (route: delegated direct, one writer). The four advisories, each fixed here:
  - R3-backoff-reset-on-open: the backoff and the outage flag reset on the first message of the
    current connection (`connectionHeardFrom`), no longer on open.
  - R3-error-per-retry: `connectionEnded` shows the failure only when the outage has not shown one
    yet; the flag clears at `connect` and when a connection hears from the server. The state and
    label still change on every end.
  - R3-ping-interval-aggressive: `CONTROL_PING_INTERVAL_SECONDS` is 5 (was 2).
  - R3-state-machine-untested: `ControlTransport.kt` (new) holds the seams, `ControlTransport` /
    `ControlSocket` / `ControlSocketEvents` with the OkHttp implementation, and `ControlScheduler` /
    `ScheduledTask` with the single-thread executor implementation (which drops work after
    shutdown). `ControlChannel` takes both as constructor parameters with production defaults, so
    `MainActivity` is unchanged. `ControlChannelReconnectTest` (new, 12 tests) drives it with a fake
    transport and a manual scheduler: first connection, register-then-mix on every open, retry with
    the policy delay, server close answered and retried once, Stop cancelling a pending retry (and an
    already-due attempt doing nothing), Stop closing a socket that is still opening, late callbacks
    from an older attempt ignored, flapping server backing off to the cap, backoff reset after the
    server is heard, one error per outage, receiver end stopping retries, and the ping interval.
- Review `review-f90e423bce7de49c` left three test-gap suggestions; they are recorded under
  Follow-ups.
- T2 done (live on the S23 with the desktop). A Wi-Fi blip and a desktop engine restart both
  reconnected on their own: registrations from new ports without any user action. When Wi-Fi
  returned, DHCP gave the phone a new IP (192.168.1.10 to 192.168.1.3), and the server, which keys
  musicians by IP, showed a second row while the old one stayed red ("lost") until the heartbeat
  closed its connection and the grace period removed it. Server log, in order:
  `control registration from 192.168.1.10:57874 ...`, `control registration from
  192.168.1.3:58058 ...`, `control connection 192.168.1.10:57874 closed: nothing received for
  10s, heartbeat missed`, `control registration 192.168.1.10:50000 removed after the grace
  period`. That duplicate row is T3.
- T3a done (route: delegated direct, one writer). What landed:
  - `protocol.rs`: `RegisterCommand.client_id: Option<String>`, read through a lenient
    deserializer: a string of 1 to 64 ASCII letters, digits and hyphens is kept; any other string
    or JSON type is read as no id and the registration still parses.
  - `targets.rs`: `TargetOrigin::Registered` stores the announced `client_id`;
    `TargetRegistry::register` takes the id and returns a `Registration { address, moved_from }`.
    A known id at another IP moves that entry (same `MixState`, `TargetCounters` and report slot
    allocations, same list position, grace period cancelled), and an entry still keyed to the new
    IP is removed so the table keeps one entry per IP.
  - `control.rs`: passes the id to the registry and logs a move as
    `control registration from <peer>: <address> as "<name>", moved from <old address>`. The
    first-register-of-session rule clears the report on the moved entry because it now lives at
    the peer's IP; the old IP's `PeerGuard` finds no entry at its IP, so its later close neither
    marks the moved entry disconnected nor removes it.
  - About 520 authored changed lines, over the ~400 heuristic: about 400 of them are tests.
- T3b done (route: delegated direct, the same writer). What landed:
  - `ClientIdentity.kt` (new): `validClientId` (the server's rule: 1 to 64 ASCII letters, digits
    and hyphens), `newClientId` (`UUID.randomUUID()`), `resolveClientId` (keeps a valid stored id,
    otherwise generates one and saves it, which also replaces a damaged value), and
    `ClientIdentity`, which reads and writes the id in its own `SharedPreferences` file
    (`mixlink_identity`), so `MixBankStore` is untouched.
  - `ControlChannel`: `buildRegisterMessage` takes an optional client id and adds `client_id` only
    when there is one; `connect` takes an optional `clientId` (default null, so existing callers
    and tests are unchanged), keeps it for the session and uses it for every registration: each
    open, each reconnect, and a rename.
  - `MainActivity`: creates the `ClientIdentity` in `onCreate` and passes its id to `connect` on
    Start.
- T3 commits: `bbe4070` (server), then this commit (Android).

## Verification

- T1 RED (tests written first, production stubs that compile but behave wrongly):
  `gradle -p apps/android :app:testDebugUnitTest`: 94 tests, 12 failed (all new policy, guard and
  label tests except `aNewAttemptIsCurrent`, which the always-current stub happens to satisfy).
- T1 GREEN: `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 94
  tests (81 before, 13 new), 0 failures, 0 errors; `compileDebugKotlin` and `assembleDebug` ran
  with no Kotlin warnings.
- `cargo fmt --all -- --check`: clean (no Rust changes).
- T1b RED, in two steps: with the tests written first, `:app:testDebugUnitTest` failed to compile
  (27 errors, all the missing seam). After the behaviour-preserving seam refactor, 106 tests ran
  and 3 failed, exactly the three behaviour changes: ping `expected:<5> but was:<2>`; errors per
  outage `expected:<1> but was:<4>`; flapping backoff `expected:<[500, 1000, 2000, 4000, 5000,
  5000]> but was:<[500, 500, 500, 500, 500, 500]>`. The other 9 new tests passed already: they pin
  the T1 behaviour that the seam made testable.
- T1b GREEN: `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 106
  tests (94 before, 12 new), 0 failures, 0 errors; `compileDebugKotlin` and `assembleDebug` ran
  with no Kotlin warnings. `cargo fmt --all -- --check`: clean.
- T2 (live on the S23): reconnection verified by the user; the duplicate row on an IP change is
  the evidence that opened T3 (see Progress).
- T3a RED (tests first; stubs that compile: `client_id` skipped by serde, `register` ignoring the
  id): `cargo test -p personal-monitoring`: 170 tests, 7 failed: both protocol tests that expect an
  id (`left: None, right: Some(...)`), four registry tests (`moved_from: None` where
  `Some(192.168.1.10:50000)` was expected; "the moved row is still connected"), and the socket
  test (`one row, not a second one for the new IP`, `left: 2, right: 1`). The other 4 new tests
  passed already: they pin the id-less behaviour, the ignored invalid ids and the id recorded on
  takeover.
- T3a GREEN: `cargo test -p personal-monitoring`: 170 passed (159 before, 11 new), 0 failed;
  `cargo test -p mixlink-desktop`: 21 passed; `cargo fmt --all -- --check`: clean;
  `cargo check --workspace --all-targets`: no warnings.
- T3b RED (tests first; stubs that compile: `validClientId` always null, `newClientId` empty,
  `resolveClientId` returning the stored value without saving, `client_id` never put in the
  message, the channel not keeping the id): `gradle -p apps/android :app:testDebugUnitTest`: 115
  tests, 6 failed: the message keys (`expected:<[type, name, udp_port, client_id]> but
  was:<[name, udp_port, type]>`), the channel test (every registration of the session carries the
  id: open, reconnect, rename), the valid-id rule (`expected:<a> but was:<null>`), the UUID
  (`Invalid UUID string`), and both generate-and-save cases. The other 3 new tests passed already:
  invalid ids rejected, a valid stored id kept without saving, and no `client_id` when there is no
  id.
- T3b GREEN: `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 115
  tests (106 before, 9 new), 0 failures, 0 errors; no Kotlin warnings. Rust unchanged since
  `bbe4070`: `cargo fmt --all -- --check` clean, `cargo test -p personal-monitoring` 170 passed,
  `cargo test -p mixlink-desktop` 21 passed, `cargo check --workspace --all-targets` no warnings.
