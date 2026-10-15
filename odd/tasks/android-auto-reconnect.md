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
  existing `controlExecutor`, with a capped backoff: 0.5 s, 1 s, 2 s, 4 s, then every 5 s. A
  successful open resets the backoff. Stop, the receiver dying on its own, and `onDestroy` cancel
  any pending attempt.
- **Nothing new on the server.** Every open already sends `register` then `mix`, and the server
  keeps the musician's target and mix for its 15 s grace period after the heartbeat or a close; a
  later reconnect simply re-registers and re-sends the phone's own mix.
- **The phone detects dead links itself:** OkHttp gets a 2 s `pingInterval`, so a half-open
  connection fails within a few seconds instead of hanging.
- **Callbacks are tied to their socket:** events from a socket that is no longer the current one
  are ignored.
- **The musician sees it:** a `RECONNECTING` control state with its own status label.

## Scope

- `ControlChannel`: stored host and control port, pure reconnect policy, scheduled attempts,
  socket-identity guard, `pingInterval`, cancellation.
- `MainActivity`: cancel on Stop, on `onStopped` (also closing the control channel) and on destroy.
- Status label for `RECONNECTING`.

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
  destroy. RED-first JVM tests for the pure policy, the guard and the label. Commit: this commit.
- [ ] T2 (live): verify on the S23 with the desktop: Wi-Fi off then on reconnects on its own and the
  musician turns green again with their mix; Stop during reconnection stops retrying.

## Acceptance criteria

- After a Wi-Fi blip, the phone reconnects on its own within about 5 s of the network returning,
  re-registers and keeps its mix; the desktop shows it green again.
- Restarting the desktop server: the phone reconnects once the server is back.
- Pressing Stop while reconnecting stops all attempts; nothing reconnects after Stop or destroy.
- A late callback from an old socket never affects the current connection.

## Acceptance checks

- `gradle -p apps/android :app:testDebugUnitTest assembleDebug` (with the JAVA_HOME and
  ANDROID_HOME used for the link-health feature)
- `cargo test -p personal-monitoring` (unchanged, regression only)

## Route

- T1: delegated direct, one writer (2+ non-trivial Android files).
- T2: parent with the user on the device.

## Delivery forecast

About 300 to 400 authored changed lines, mostly tests; one pull request stacked on
`feat/link-health-6-heartbeat` (#6). Strategy: `ask-on-risk`.

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

## Verification

- T1 RED (tests written first, production stubs that compile but behave wrongly):
  `gradle -p apps/android :app:testDebugUnitTest`: 94 tests, 12 failed (all new policy, guard and
  label tests except `aNewAttemptIsCurrent`, which the always-current stub happens to satisfy).
- T1 GREEN: `gradle -p apps/android :app:testDebugUnitTest assembleDebug`: BUILD SUCCESSFUL, 94
  tests (81 before, 13 new), 0 failures, 0 errors; `compileDebugKotlin` and `assembleDebug` ran
  with no Kotlin warnings.
- `cargo fmt --all -- --check`: clean (no Rust changes).
- T2 (live on the S23): pending.
