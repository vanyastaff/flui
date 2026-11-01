# ADR-0284: Independent UI runtimes may have dedicated owners

- **Status:** Proposed
- **Date:** 2026-11-01
- **Supersedes:** on acceptance only, [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md)
  §1's process-wide owner requirement, §2's unconditional denial of independent frame progress,
  §4's exhaustive list of public transfer positions, and §5's requirement to express dedicated
  ownership as a `WindowPolicy` variant. Its runtime isolation and addressed telemetry remain;
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §6's shrinking transfer-position
  list only for the dedicated launch factory bound described below.
- **Related:** [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) §2 (owner-local UI),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §6 (transfer-position gate),
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) §2–§6 (GPU ownership),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) §1 and
  [ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md)
  §1–§4 (the local pull text-store contract),
  [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (failure custody).

## Context and problem statement

The current runners place all UI runtimes in one thread-local `APP_RUNTIME` and dispatch
their transactions serially. State isolation does not prevent a long transaction delaying
another runtime. The 2026-11-01 experiment at FLUI
`bb618a7f7f1510ff11093109094ba02c937d1970` exercised production `UiRuntime::pump` through
`HeadlessHost`, without transferring UI values or modifying framework code:

| CPU measurement, median of three process means | Shared owner | Dedicated owners |
|---|---:|---:|
| Light runtime completion after a 1000-text-row runtime | 39.811 ms | 0.165 ms |
| Completion of two 1000-text-row transactions | 76.264 ms | 39.910 ms |

The shared-owner light-first control took 0.137 ms: ordering matters, and this is not proof
that all shared-owner work is slow. Dedicated owners improved aggregate CPU throughput 1.91
times in this workload. These are headless CPU transactions, not native input-to-present
latency or a claim of implemented multi-owner runners. Which ownership boundary permits
independent progress without making UI state transferable?

## Decision drivers

- Preserve synchronous build/layout/paint and `!Send` UI state, with no per-node locks.
- Isolate independent CPU transactions without promising isolated GPU or OS scheduling.
- Preserve existing local factories and synchronous sharing within one runtime.
- Keep native window, text-input and accessibility operations on their required threads.
- Bound transport and settle accepted work on failure, cancellation and retirement.
- Require executed host evidence before publishing a dedicated-owner support claim.

## Considered options

1. Keep one mandatory owner per process; rely on cooperative scheduling and background jobs.
2. Give every window its own UI and native event-loop thread.
3. Keep shared ownership as the default; admit explicitly constructed dedicated UI owners.
4. Keep one UI owner and add only raster threads or parallel paint.

## Decision

Chosen option: 3. Ownership is per runtime; placement is a host capability and an explicit
application choice. This is a proposed target, not a description of current runners.

### §1. Construction establishes ownership

A runtime is constructed, pumped and destroyed on one owner thread for its entire lifetime.
Its presentations stay with it. No live runtime, widget, signal, callback, local future or
native handle migrates. `WindowPolicy::Isolated` and `WindowPolicy::Shared` retain their
current meaning and signatures; neither silently requests a new thread.

A separate launch entry accepts a transferable construction recipe, not a constructed tree.
The intended Rust shape is the following declaration (not an implemented API):

```rust,ignore
pub fn open_dedicated_window<V, F>(factory: F) -> Result<WindowRequest, AppWindowError>
where
    V: View + 'static,
    F: FnOnce() -> (V, RuntimeWindowConfig) + Send + 'static;
```

The entry runs from a live application's native owner, like the existing secondary-window
entry. The factory executes on the new UI owner; its result has no `Send` bound. It creates
owner-local configuration, providers and hooks there. `RuntimeWindowConfig` has private
fields and only window/runtime options; application identity, process services, executors
and exit policy cannot be set through it. An existing `AppConfig` containing local callbacks
cannot be captured and smuggled through this boundary. The factory is never
stored in `AppHandle`'s closed command ingress.

This deliberately proposes one new facade transfer position: the launch factory's `Send`
bound. Acceptance must explicitly amend the shrinking-position policy for that named entry
in the `send-positions` allowlist and repository guidance; no blanket reseed, additional
UI bounds, unsafe `Send` implementation or `send_wrapper` exception is authorized. Until
acceptance, the existing gate and policy remain unchanged. A production consumer and a
compile-fail fixture for capturing `Rc` accompany that entry.

### §2. Native ownership and UI ownership are separate

The initial native integration keeps windows, native callbacks and surface operations on
the native event-loop thread. Dedicated UI owners exchange typed owned observations,
commands and scenes with it. `OwnerPlatform`, `HostWindow`, frame bindings and renderer
objects never become transferable to make this convenient. Headless uses the same addressed
protocol without claiming native conformance.

Local and remote delivery terminate in the same owner transaction implementation. An
`OwnerRoute` selects local delivery or a bounded remote endpoint; a typed owner incarnation,
`UiRuntimeId` and `PresentationAddress` identify destinations. A thread ID is diagnostic,
not lifetime authority. Neither owner synchronously waits for another owner to execute UI
code. Native synchronous queries use the native text/accessibility contract and versioned
published state; an unimplemented query contract blocks admission for that host.

The current `Rc<dyn TextStore>` and local `LockGrant` callbacks cannot implement that remote
contract by forwarding. This ADR does not supersede their accepted pull/edit semantics. The
native bridge is a blocking design proof: before it changes those semantics, its own narrow
text-contract amendment must be reviewed and accepted with native evidence. If a faithful
bridge cannot be demonstrated, keep dedicated admission closed and revisit native/UI
colocation on Win32 rather than weakening text correctness or adding synchronous RPC.

### §3. Progress has an explicit scope

A dedicated owner can progress while another UI owner executes a long synchronous turn.
It does not preempt its own callbacks or isolate the native loop, GPU, compositor, CPU
capacity or memory bandwidth. Windows with synchronous shared state stay in one runtime;
cross-owner application state uses versioned owned messages or immutable snapshots.

Frame requests coalesce only within their full address and generation. Scenes retain the
existing bounded latest-pending contract; recovery, installation and retirement are reliable
control obligations, not lossy frame telemetry. Admission returns typed refusal before
accepting work that cannot be retained. A missing or failed wake cannot erase accepted work.

### §4. Retirement is acknowledged, not detached

Launch progresses through reserved, starting, mounted, retiring and retired states. A failed
launch rolls back native and UI resources on their respective owners. Cancellation of an
unobserved receipt does not discard admitted work. Retirement fences new work, settles
receipts, revokes routes, releases presentation resources and drops UI captures on their
owner, then acknowledges completion before the native loop exits. No join blocks an active
native callback; no shutdown timeout detaches a live owner or drops its UI on another thread.

User callbacks, wakes, logging and destruction run with transport guards released and state
committed. A contained failure preserves the first failure while completing other owners'
retirement. A destructor panic during unwind follows ADR-0281. A callback that never returns
cannot be safely killed; the application remains shutting down until completion or an
explicit process-exit decision. Dedicated ownership is not process fault isolation.

### §5. Availability follows verification

Unsupported hosts return a typed dedicated-owner refusal before factory execution; they
never silently fall back to a shared owner. The shared-owner path remains available on all
current runners. Win32 and headless are the first implementation targets, not supported
platforms by virtue of this ADR. Native Wayland, native X11, winit, AppKit, UIKit, Android and
Web each require their own disposition and evidence. No compile-only or headless result
promotes a native host's status.

This ADR does not supersede ADR-0091 §2–§6, select per-window GPU contexts, enable threaded
Metal acquisition, or authorize parallel mutable layout. Those require separate decisions
and measurements. Keeping an existing inline native raster path is compatible with this
UI-owner decision; its contention must be measured separately.

### Consequences

- Good, because independent CPU work can overlap while the compiler protects local UI state.
- Good, because default applications and their local factories keep their existing contract.
- Bad, because launch, native queries, global quit and shared documents need explicit protocols.
- Bad, because extra threads and message copies cost memory and latency on small workloads.
- Neutral, because dedicated ownership alone promises neither higher FPS nor GPU isolation.

### Confirmation

All following multi-owner cases are required future checks, not tests reported as passing:

- Compiler fixtures reject moving `UiRuntime`, `OwnerPlatform`, local hooks or `Rc` captures;
  accept a transferable factory that constructs an `Rc`-holding view on its destination.
- `send-impls`, `send-positions`, `globals`, `panic-boundary` and `api-closure` continue to
  constrain the actual facade and native paths, with only the accepted factory exception.
- Public-path tests prove addressed delivery, full-queue refusal, wake replacement, stale
  completion rejection, owner-thread destruction and independent sibling progress.
- Existing shared-owner conformance is rerun after runner migration; a two-native-window
  suite checks input, IME, accessibility, resizing, surface recovery and global quit.
- A release benchmark records request-to-owner completion separately from native presentation,
  includes light-first and heavy-first controls, and preserves source pins and raw samples.
- Host-specific executed evidence, including negative controls, is required before updating
  [platform parity](../platform-parity.md). Detailed acceptance is in the
  [multi-owner specification](../plans/specs/multi-owner/requirements.md).

## Pros and cons of the options

### 1. Mandatory shared owner

- Good, because native access and shared local state need no transfer protocol.
- Bad, because cooperative budgets cannot interrupt a synchronous build/layout/callback;
  the measured heavy-first delay remains possible even with fair scheduling between turns.

### 2. One UI and native thread per window

- Good, because some native hosts can colocate their window and UI work on separate threads.
- Bad, because a presentation is not a runtime; shared presentations would acquire conflicting
  ownership. Native loop restrictions and process services also differ by platform.
- Bad, because it imposes thread and GPU costs without a workload-specific benefit.

### 3. Explicit dedicated runtime owner

- Good, because independent runtimes gain CPU progress without changing the ownership of UI.
- Good, because local and dedicated paths can share lifecycle and delivery semantics.
- Bad, because synchronous native queries and asynchronous installation need new integration.
- Bad, because applications must choose a boundary for state shared between owners.

### 4. Raster threads or parallel paint alone

- Good, because they can accelerate suitable rendering workloads without changing UI placement.
- Bad, because they do not isolate another runtime from a long synchronous UI callback.
- Bad, because neither universal acceleration nor the best FLUI phase follows from Raven.

## More information

The dated experiment ran on Windows 11, i9-13900K, 64 GB RAM, RTX 3070 Ti, Rust 1.99 release.
It used three processes per condition, five warmup and forty measured updates per process,
real text replacement/layout/paint, and scene/text validation. Semantics and native rendering
were outside that FLUI experiment. The two-heavy shared means ranged 75.985–77.762 ms and
dedicated means 39.791–40.261 ms. The external evidence bundle is identified by the report
SHA256 `ddd3f40a71e1dc32832ad256547a03cf197a18f98caec3e442abf021ca402d0e`;
the implementation must preserve reproducible probes and attach native run artifacts before
claiming conformance. No benchmark source is treated as production integration.

Primary counterexamples and limits, not reasons to copy their topology:

- [WPF Dispatcher](https://github.com/dotnet/wpf/blob/dbd3026982f09a279e4595666c07bb6136f579c3/src/Microsoft.DotNet.Wpf/src/WindowsBase/System/Windows/Threading/Dispatcher.cs#L35-L58):
  a dispatcher belongs to its thread. The separate two-window experiment used installed
  .NET Framework 4.8.10008.0, not a build of this source pin: heartbeat p99 was 209.24 ms shared,
  30.69 ms separate and 32.01 ms unloaded under approximately 200 ms CPU callbacks.
- [Raven paint](https://github.com/oscar-soirey/Raven/blob/21a2c6c1e45c628f63e61ae1912baf8b11d093f1/src/widget/paint.c#L121-L184):
  parallel subtree paint, not independent UI owners. CPU paint improved 1.87 and 1.72 times
  on the large measured scenes; small scenes did not improve. Separate Vulkan-renderer
  comparisons had overlapping ranges. Final captures checked an additional static repaint,
  not every timed frame.
- [Qt render loop](https://github.com/qt/qtdeclarative/blob/4332fbbb4c340463dc8cee36dda0c85d932122d8/src/quick/scenegraph/qsgthreadedrenderloop.cpp#L1263-L1285):
  per-window rendering is a different axis from UI ownership.
- [Locked Metal acquisition](https://github.com/gfx-rs/wgpu/blob/40f4a34ebaf56f9a046231f54125ad046239d3f3/wgpu-hal/src/metal/surface.rs#L347-L370):
  native access in the current acquisition path needs its own thread-affinity work. It does
  not establish that framework build/layout must execute on the AppKit thread.

Revisit dedicated admission if native-query fidelity cannot be preserved without cyclic
waits, or if real application measurements show transfer and resource costs outweighing
the isolation benefit. Do not replace evidence with a framework popularity argument.
