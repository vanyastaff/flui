# ADR-0239: Every debug FLUI app hosts the driver agent

- **Status:** Accepted (2026-10-20)
- **Date:** 2026-10-17
- **Supersedes in part:** [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) §3 as amended on
  acceptance: where the server lives (`flui-devtools` → `flui-app`), who installs it (the host,
  not application code), which hosts drive it (every host, not desktop and iOS only) and the
  build rule (`debug_assertions` or the `driver` feature). Unchanged: the owner-inbox execution,
  local endpoints only, the token (at least 32 bytes, constant-time compare, current-user pipe,
  `0700` socket directory), bounded waits and `may_have_run` on a timed-out action.
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md), ADR-0237 (the Android Java
  view, landing with `interaction/completeness`)
- **Spec:** [flui-driver](../plans/specs/flui-driver/design.md)

## Context

ADR-0095 §3 put the in-process server in `flui-devtools`, installed by application code through
`AppConfig::with_dev_agent`. `flui-app` could not name an official package, and the facade names
none (ADR-0088). The result is that no example is drivable as built, a test tool cannot drive an
app it did not write, and Android and web drive no hook at all. Flutter's experience points the
same way: its driver needed an extension each app had to enable, and `integration_test` replaced
that with a binding every test app gets.

## Decision

1. **The server moves into `flui-app`** (`app::agent`), compiled under
   `cfg(any(debug_assertions, feature = "driver"))`. The facade's `driver` feature forwards to
   it. `flui-devtools` keeps its profiler, timeline and inspector counters and loses its `agent`
   module. `DevAgentHook` stays the seam for other development hooks, and the host installs the
   driver agent through it itself.
2. **It attaches only when asked.** With no `FLUI_AGENT_ENDPOINT` (or its per-platform
   equivalent), nothing is created: no thread, socket, tap or journal. An optimized build without
   `driver` contains none of the code.
3. **Every host drives it**: desktop runners, iOS, Android, web and the headless host. The
   per-platform transport and launch contract:
   - Windows: current-user named pipe.
   - macOS and Linux: Unix socket in a `0700` directory.
   - iOS Simulator: Unix socket on the host filesystem, credentials through `SIMCTL_CHILD_` env.
   - Android: an abstract Unix socket, `adb`-forwarded to loopback by the driver, credentials
     through `am start` intent extras read by the Java view.
   - Web: a function the web runner exports, called through WebDriver BiDi `script.callFunction`,
     with events through a BiDi channel. The token is installed by the driver's preload script and
     is never in a URL.
   - Headless: in-process calls, no transport.
4. **The agent's operations** are ADR-0238's `Agent` methods, each an owner-inbox job at a frame
   boundary:
   - semantics read, locate and act;
   - element, render and layer inspection;
   - actionability facts;
   - synthetic input injected into the window's host input path, the same entry native events take;
   - the input journal and recording taps (ADR-0215 rows, native capture where built);
   - waits and events;
   - presented-frame capture, a one-shot copy of the surface texture before present, labelled
     `rendered` when the surface cannot be copied.
5. **The connection owns what it pressed.** Contacts and keys the driver injected are released or
   cancelled when the connection closes. With `FLUI_AGENT_EXIT_ON_DISCONNECT`, a launched app exits
   when its driver's connection closes. An attached app never gets the flag.
6. **User code runs only inside owner jobs**, after the job's answer slot is committed. The endpoint
   thread parses, enqueues, waits a bounded time and answers. Panics in a job answer `platform` with
   the first failure, and the agent keeps serving.

## Consequences

- Any debug example is drivable by `flui drive launch` and by `flui-driver` tests with no code
  change. Optimized-profile CI and device runs enable `driver`.
- `flui-app` gains `interprocess` as a dependency on desktop, Android and iOS, and a web export.
- The bespoke probes (`android_input_probe`, `web_input_probe`) and their command channels are
  deleted; their checks run through the agent.
- `AppConfig::with_dev_agent` remains for third-party development hooks. The devtools `agent`
  feature and `AgentServer` are removed.

## Alternatives considered

- **Keep the server in `flui-devtools` and have the facade install it.** Rejected: the facade names
  no package (ADR-0088), and an exception for a debug-only server would be the first breach of
  that rule.
- **App code installs it (status quo).** Rejected: no example and no third-party app would be
  drivable without editing it, which is exactly what the tool must avoid.
- **A TCP endpoint for devices.** Rejected: an `adb`-forwarded abstract socket reaches the device
  without opening a network port. BiDi already reaches the browser.

## Verification

- `cargo nextest run -p flui-app agent`:
  - a debug runner with no endpoint creates nothing;
  - with an endpoint it answers `hello`;
  - a dropped connection releases injected contacts;
  - `EXIT_ON_DISCONNECT` exits.
- The new driver crate's headless suite (feature `headless`): the owner-job failure rows (panic,
  reentrant window close, timed-out act runs at the next drain).
- A Linux CI test drives a debug example over the socket and kills it mid-call (`gone`, kind
  `process`).
- Android, web and iOS transports: each lane's scenario on its platform, listed in the PR.
