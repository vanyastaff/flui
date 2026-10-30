# ADR-0266: One owner thread hosts every UI runtime; a UI runtime is a unit of isolation, not of parallelism

- **Status:** Accepted (2026-10-30). Not implemented: frame telemetry does not name the UI runtime
  that produced a frame (§3).
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md), the verdict's sentence "Multiple
  UI runtimes may execute concurrently" and §1's sentence on runtime count per owner thread only
  (the ownership levels, the `!Send` model and the rest of §1 stand);
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §1 (its `!Send` flip has
  landed; its per-runtime owner-thread spike and `OwnerExecutor` trait are withdrawn)
- **Related:** [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md) (the
  `GlobalKey` scope a UI runtime owns), [ADR-0092](ADR-0092-per-realm-text-over-parley.md) (text
  per UI runtime), [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the host's `APP_RUNTIME`
  trampoline), [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) (the owner-local surface and its
  gates), [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) decision 8 (the span
  stream partitioned by runtime and presentation), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md)
  (the panic boundary of an owner turn)

## Context and problem statement

ADR-0027's verdict says several UI runtimes "may execute concurrently", and its §1 lets Win32,
Linux and headless hosts give each runtime its own owner thread. The code has one owner thread:
`flui-app` keeps every UI runtime in the host's thread-local `APP_RUNTIME`
(`crates/flui-app/src/app/runner/host.rs`) and runs them in turn. ADR-0091 §1 kept that through H2
and scheduled a spike on per-runtime threads behind an `OwnerExecutor` trait, after the UI surface
lost `Send`. That flip has landed (ADR-0136), so the spike's precondition holds, and the open
question is whether the spike is still the plan. An experiment on 2026-10-05
([realm-model experiment](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/plans/specs/realm-model/experiment.md))
found what the per-window isolation promised by `WindowPolicy` actually delivered: state, keys and
scheduling are separate, frame time is not. Which threading model does FLUI commit to for its UI
runtimes, and what does a UI runtime isolate?

## Decision drivers

- AppKit, UIKit and Android's view system run UI on the main thread only, and wasm has one
  thread: a model that needs a thread per runtime works on Win32 and Linux alone.
- The frame path is synchronous and owner-local (`AGENTS.md`, ADR-0136): parallelism belongs where
  the work is parallel (decode, shaping, IO, raster), not inside or between UI transactions.
- Cross-window features (drag and drop between windows, a shared document open in two windows)
  need cheap, synchronous access to state both windows show.
- Nothing published may close a later thread-per-runtime option for the hosts that can have it.

## Considered options

1. One owner thread per process; UI runtimes run serially on it and isolate state, not time.
2. A thread per UI runtime now, on the hosts that allow it.
3. An `OwnerExecutor` trait now, with one owner thread as its only implementation.

## Decision

Chosen option: 1, because it is the only model every host can run, it keeps state shared by
windows synchronous, and it leaves option 2 open as an additive policy.

### §1. One owner thread per process

Every UI runtime of a process runs on one owner thread: the platform's UI thread where the
platform has one (AppKit, UIKit, Android), the only thread on wasm, the event-loop thread
elsewhere. The host runs the runtimes' turns serially. `UiRuntime` and everything it owns is `!Send`
(ADR-0136), so a runtime cannot be moved off that thread, and the host's `APP_RUNTIME` slot stays
the one trampoline OS callbacks reach (ADR-0097). There is no `OwnerExecutor` trait.

### §2. A UI runtime isolates state, not frame time

A UI runtime is a unit of isolation for:

- the `GlobalKey` scope (ADR-0043), the widget state, focus and gesture arenas of its
  presentations;
- its scheduler, post-frame queue and async driver, and the wake that redraws its own windows (a
  sibling's wake never redraws them);
- teardown: closing its last presentation ends it and nothing else;
- failure: a panic in its owner turn is contained there (ADR-0258);
- text and fonts state (ADR-0092).

It is not a unit of frame time. A long synchronous callback, build or layout in one runtime
delays every other runtime's next frame, because they share the owner thread.
`WindowPolicy::Isolated` documents exactly this, and nothing promises more.

### §3. Which runtime made a frame slow is a typed fact

A delay from §2 is diagnosable only when frame telemetry names the runtime that produced the frame.
Telemetry carries a `PresentationAddress` (`ui_runtime_id`, `presentation_id`), never a bare
`PresentationId`, which repeats across runtime incarnations (`FrameStamp`'s rule). Today only the
frame-failure events carry the address; `FrameTiming`, `FrameSnapshot`, the `flui.frame`
`frame_telemetry` event and the build, layout, paint and compositing spans name the presentation
or nothing. Moving them to the address is the implementation of ADR-0242 decision 8's runtime
partition, and that is when this section is implemented.

### §4. Parallelism is where the work is parallel

Work that can run in parallel leaves the owner thread through the typed cross-thread handles:
`IoFuture`, `ComputeJob` and the host pools for workers, `SignalSender`, `RebuildHandle`,
`FrameWaker` and `AppHandle` for coming back, the raster thread for GPU work
([ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §4). Those are the
public API's only `Send` positions, held by `cargo xtask send-positions`.

### §5. What may come later, and how

- **State shared by windows** is an app-scope owner-local signal graph on the owner thread: read in
  any runtime's `build`, written through `EventCx`. It adds API and changes none.
- **A thread per runtime**, on the hosts that allow it, is a new variant of the
  `#[non_exhaustive] WindowPolicy` with its own ADR. No public signature names a thread, and a
  runtime's types never leave it, so the variant breaks nothing published.

### Consequences

- Good, because one model runs on every host, and shared-window state needs no locks or channels.
- Good, because a UI runtime's guarantees are stated as what it isolates, and frame time is
  explicitly not one of them.
- Bad, because one slow window delays every other window of the process, until a later
  thread-per-runtime policy exists for the hosts that can have it.
- Bad, because until §3 lands, telemetry cannot attribute a slow frame to a runtime by itself.
- Neutral, because a host with a single window, the common case, behaves the same under every
  option.

### Confirmation

- The compiler: `UiRuntime` and the owner-local surface are `!Send`; trybuild fixtures in
  `tests/ui/thread_boundary/` and `cargo xtask send-positions` / `send-impls` keep it so.
- `cargo xtask globals`: `APP_RUNTIME` is the host's one owner trampoline.
- `runtime_frame_wake_reaches_its_window_after_sibling_installation` (`flui-app` host delivery): a
  runtime's frame request redraws its own window and not its sibling's;
  `frame_waker_wakes_the_ui_runtime_from_a_worker` (`flui-runtime`): a worker's wake reaches its
  runtime only; `two_ui_runtimes_via_isolated_policy_share_nothing` (`flui-app` owner dispatch).
- §3, once implemented: a test that two runtimes' frames are told apart by their telemetry address.

## Pros and cons of the options

### 1. One owner thread, serial runtimes

- Good, because AppKit, UIKit, Android and wasm can run it.
- Good, because state shared by windows is plain owner-local state.
- Good, because the panic boundary, teardown and wake routing have one thread to reason about.
- Bad, because frame time is shared between windows.

### 2. A thread per UI runtime now

- Good, because a slow window cannot delay another on Win32 and Linux.
- Bad, because AppKit, UIKit, Android and wasm cannot have it, so applications behave differently
  per platform.
- Bad, because state shared by windows needs `Send` handles and cross-thread wakes, the shape
  ADR-0136 removed from the UI surface.
- Bad, because OS callbacks reach one thread; each would need forwarding to the owning runtime.

### 3. An `OwnerExecutor` trait now

- Good, because a second executor could be added behind it.
- Bad, because it is an abstraction with one implementation, and its shape would be guessed
  before the second implementation exists; option 1's `WindowPolicy` variant leaves the same
  room without it.

## More information

- Revisit when a measured application shows one window's frames missing their deadline because of
  another window's work on a host that allows a second thread, with §3's telemetry as the evidence.
