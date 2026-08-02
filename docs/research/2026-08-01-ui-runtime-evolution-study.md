# UI Runtime Evolution Study

> Research basis for FLUI's runtime, concurrency, multi-window, embedding, and
> presentation architecture. This study complements Flutter parity work: Flutter
> remains the behavioral reference for the widget-tree core, while runtime and
> process topology are sanctioned leapfrog zones under ADR-0027.

**Date:** 2026-08-01
**Scope:** desktop-first runtime architecture, with mobile, web, realtime, and
engine embedding kept possible without speculative game-engine code.

## Executive decision

FLUI should be a **hostable, multi-presentation UI runtime built from explicit
single-writer ownership domains**. It should not be a shared-mutable tree, a
process-global binding graph, or a game engine hidden inside a UI toolkit.

The existing direction in ADR-0027 and ADR-0037 is sound, but the repository has
not completed the migration. The immediate architecture work is therefore not a
new design. It is the removal of transitional contracts that contradict the
accepted design:

- process/thread-local binding singletons;
- the at-most-one-`UiRealm` process guard;
- `Arc<RwLock<PipelineOwner>>` as the ownership carrier;
- one-presentation-only realm storage;
- a platform trait that combines owner-affine and cross-thread operations;
- a global 60 FPS scheduler model;
- one Tokio pool per platform instance with informational-only priority;
- render-side thread-safety bounds and unsafe pointer traits imposed before a
  real parallel execution model exists.

The stable long-term shape is:

```text
App
└── AppRuntime
    ├── shared engine services and application-owned models
    ├── one platform/event-loop host
    ├── one or more UiRealm owners
    │   └── one or more PresentationState roots
    └── one raster owner per presentation or backend-selected raster lane
```

UI transactions remain serial within one realm. Real parallelism occurs between
realms, raster/compositor work, immutable worker jobs, I/O, and later proven
independent render boundaries. This preserves Flutter lifecycle behavior while
using Rust ownership to remove ambiguity rather than spreading locks through the
tree.

## Method

The study used four evidence classes:

1. **Shipped architecture documentation** from framework and engine owners.
2. **Maintainer-authored issue and migration history** showing where an old
   contract prevented local repair.
3. **Current repository evidence** from FLUI source, ADRs, tests, and public API.
4. **Research literature** for incremental and parallel computation invariants.

Popularity is treated only as an adoption signal. It is not evidence that an
architecture is correct. GitHub metadata below was read on 2026-08-01; counts
will naturally change.

## Ecosystem movement

| Project | Adoption/activity signal | Architectural movement | Lesson for FLUI |
|---|---:|---|---|
| Flutter | ~178k stars; active | Multi-view work is removing implicit global-window assumptions; the main isolate still owns normal UI work | Make presentation identity explicit before public APIs depend on one window or one tree |
| React Native | ~126k stars; active | A ground-up renderer/native-boundary rewrite ran from 2018 until the New Architecture became default | A wrong cross-domain boundary can consume years even when gradual migration is possible |
| Tauri | ~110k stars; active | Strong adoption for webview-based desktop apps | FLUI must compete on native rendering, predictable behavior, and Rust-native composition rather than bundling alone |
| Zed/GPUI | ~88k stars for Zed; active | Product-driven hybrid immediate/retained UI with framework-owned entities | A demanding reference product is excellent validation, but product-specific shortcuts must not become universal toolkit contracts |
| Bevy | ~47k stars; active | Separate main/render worlds; task classes and schedule-based rendering | Extract immutable render data and integrate with a host executor instead of sharing mutable worlds |
| Dioxus | ~38k stars; active | Signals, native wgpu/HTML renderer, hot patching, mobile tooling | Integrated DX is becoming expected; runtime handles trade RAII and direct access for ergonomics |
| Iced | ~31k stars; active | Elm architecture, modular renderer-independent runtime | Explicit messages scale conceptually but can become component boilerplate; keep FLUI callbacks and state local by default |
| egui | ~30k stars; active | Immediate-mode ecosystem remains strong for tools and games | FLUI should embed well in realtime hosts without replacing its retained lifecycle semantics |
| Slint | ~23k stars; active | Expanding from embedded to desktop; current work includes DnD, modality, popups, tray, and accessibility tooling | Desktop readiness is a platform-contract problem, not merely a widget-count problem |
| Compose Multiplatform | ~19k stars; active | Shared declarative model across mobile/desktop/web with phase-scoped invalidation | Track reads by phase eventually, but do not make correctness depend on compiler heuristics |
| Makepad | ~6.5k stars; active | Live design and custom GPU UI | Fast feedback and runtime-editable design can be strategic without changing core lifecycle semantics |
| Xilem/Masonry | ~5.5k stars; active pre-1.0 | Typed transient views synchronized to a retained widget tree; multi-window added in 2025 | Rust UI is converging on typed views plus retained identity, close to FLUI's three-tree direction |

Repository sources: [Flutter](https://github.com/flutter/flutter),
[React Native](https://github.com/facebook/react-native),
[Tauri](https://github.com/tauri-apps/tauri),
[Zed](https://github.com/zed-industries/zed),
[Bevy](https://github.com/bevyengine/bevy),
[Dioxus](https://github.com/DioxusLabs/dioxus),
[Iced](https://github.com/iced-rs/iced),
[egui](https://github.com/emilk/egui),
[Slint](https://github.com/slint-ui/slint),
[Compose Multiplatform](https://github.com/JetBrains/compose-multiplatform),
[Makepad](https://github.com/makepad/makepad), and
[Xilem](https://github.com/linebender/xilem).

## What mature projects had to repair

### Flutter: implicit single-view and isolate boundaries

Flutter's public `window` API was deprecated specifically to prepare for
multi-window support. Maintainers note that replacing it is not always
mechanical because existing code assumes one implicit view. The Android
multi-display proposal likewise states that supporting multiple views requires
examining broad areas that assume one widget tree.

Flutter isolates also have isolated memory and communicate by messages. Normal
Flutter UI work runs in the main isolate; helper isolates cannot use `dart:ui`
and have plugin-message limitations. This is safe but makes shared desktop state
and rich background integration more explicit and costly than ordinary shared
Rust data.

Sources: [Flutter architecture](https://docs.flutter.dev/resources/architectural-overview),
[Flutter concurrency and isolates](https://docs.flutter.dev/perf/isolates),
[global window deprecation](https://github.com/flutter/flutter/issues/143399),
and [Android multi-display proposal](https://github.com/flutter/flutter/issues/134405).

**FLUI response:** never expose an implicit current window/presentation in a
stable API. Keep `WindowId`, `RealmId`, and generational `PresentationId`
distinct. Shared application models belong to `AppRuntime` or the application,
while presentation state remains local.

### React Native: the cost of an architectural bridge

React Native began redesigning its core in 2018 because the asynchronous bridge
and legacy renderer prevented synchronous layout effects, concurrent rendering,
and efficient native interoperation. The New Architecture replaces the bridge
with JSI, introduces typed native interfaces, immutable shadow structures, and
commit-oriented concurrent rendering. It became the default only after years of
incremental migration.

Sources: [New Architecture rationale](https://reactnative.dev/architecture/landing-page)
and [release announcement](https://reactnative.dev/blog/2024/10/23/the-new-architecture-is-here).

**FLUI response:** a generic cross-thread closure executor or opaque bridge is
not an escape hatch. Cross-domain protocols use owned typed messages, versioned
results, bounded delivery, and explicit commit points.

### Compose: precise invalidation is phase-aware

Compose tracks state reads separately during composition, measurement,
placement, and drawing. A write can therefore restart only the affected phase.
Its documentation also recommends immutable UI snapshots and unidirectional
data flow to avoid concurrency inconsistencies.

Sources: [Compose phases](https://developer.android.com/develop/ui/compose/phases),
[Compose architecture](https://developer.android.com/develop/ui/compose/architecture),
and [UI state production](https://developer.android.com/topic/architecture/ui-layer).

**FLUI response:** replace blanket layout-and-paint invalidation with explicit
update impact first. Read tracking can be added later only after an explicit
subscription implementation proves semantics. Correctness must not depend on a
compiler plugin or invisible heuristic.

### SwiftUI/UIKit and Windows: a scene is not necessarily a window

SwiftUI scenes are lifecycle containers. A `WindowGroup` can create multiple
windows from one root definition, and every window retains its own view state.
UIKit scenes share one process and memory while representing independent UI
instances. Windows separately models application process instances and permits
activation routing by application-defined keys.

Sources: [SwiftUI scenes](https://developer.apple.com/documentation/swiftui/scenes),
[SwiftUI windows](https://developer.apple.com/documentation/swiftui/windows),
[UIKit scenes](https://developer.apple.com/documentation/uikit/scenes), and
[Windows app instancing](https://learn.microsoft.com/en-us/windows/apps/develop/launch/multi-instance-apps).

**FLUI response:** do not equate `App`, process, realm, presentation, and native
window. Keep process-instance policy above the widget framework. A default
desktop policy may create one realm per window without making that topology a
core invariant.

### Rust UI: ownership ergonomics remain the central tradeoff

Xilem's architecture was created after Druid exposed recurring problems with a
global `Data: Clone + PartialEq` constraint, difficult lenses, async integration,
and environment invalidation. GPUI uses framework-owned reference-counted
entities and makes developers retain entities explicitly. Dioxus uses
generational, copyable signal handles; its own documentation acknowledges an
extra indirection and lock, lifecycle-managed disposal, and a runtime panic on
read after disposal. Slint keeps UI objects event-loop-affine and marshals
worker results back to that thread.

Sources: [Xilem architecture essay](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html),
[current Xilem architecture](https://docs.rs/crate/xilem/latest/source/ARCHITECTURE.md),
[GPUI overview](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md),
[Dioxus signals](https://dioxuslabs.com/learn/0.7/essentials/basics/signals/),
and [Slint threading](https://docs.slint.dev/latest/docs/rust/slint/).

**FLUI response:** preserve local `State` without global trait-bound creep.
Use framework-owned generational IDs for tree identity, ordinary Rust ownership
for application state, and narrowly scoped capabilities for framework actions.
Do not force all user state through locks to claim multithreading.

### Desktop completeness: platform behavior dominates late cost

Slint's desktop-readiness program shows the breadth that appears after basic
rendering works: cross-window and cross-application DnD, modal ownership, real
popup windows, tooltips, tray lifecycle, and native platform gaps beneath
`winit`. These features require correct event-loop, window, focus, data-transfer,
and shutdown ownership.

Source: [Making Slint Desktop-Ready](https://slint.dev/blog/making-slint-desktop-ready).

**FLUI response:** finish presentation and platform ownership before declaring
desktop readiness. Widget parity alone cannot validate the application shell.

## Browser and game-engine lessons

### Isolated mutable worlds with commit/activation

Chromium's main tree and compositor trees are physically separate. Commit moves
state to a pending tree, activation makes it drawable, and the compositor can
scroll or animate committed content while the main thread is busy. The same
architecture can run with single-threaded and threaded scheduling.

Unreal similarly states that the render thread must never dereference objects
owned by the game thread. It copies render data into render-owned structures and
requires deterministic ordering; its documentation warns that race bugs are
dramatically harder to reproduce and repair.

Bevy extracts data from `MainWorld` into `RenderWorld`. Its task pools distinguish
work required for the current frame, CPU work that may span frames, and I/O.

Sources: [Chromium compositor](https://chromium.googlesource.com/chromium/src.git/+/refs/heads/main/docs/how_cc_works.md),
[RenderingNG](https://developer.chrome.com/docs/chromium/renderingng-architecture),
[Unreal threaded rendering](https://dev.epicgames.com/documentation/unreal-engine/threaded-rendering-in-unreal-engine),
[Bevy extraction](https://docs.rs/bevy/latest/bevy/render/struct.ExtractSchedule.html),
and [Bevy task pools](https://docs.rs/bevy/latest/bevy/tasks/).

**FLUI response:** `SceneSnapshot` is the right boundary, but it must carry the
exact presentation identity and timing metadata. The raster owner must never
reach back into `Element`, `State`, `RenderObject`, or a shared `PipelineOwner`.

### Parallelism follows dependencies, not tree shape alone

Servo layout alternates between parallel traversal and sequential regions where
layout dependencies require ordering. Unreal's task system expresses a DAG of
prerequisites and recommends dependencies instead of worker blocking. Research
on parallel self-adjusting computation similarly tracks sequential and parallel
control dependencies; correctness is defined as producing the same result as a
from-scratch computation.

Sources: [Servo layout](https://book.servo.org/design-documentation/layout.html),
[Unreal tasks](https://dev.epicgames.com/documentation/unreal-engine/tasks-systems-in-unreal-engine),
[Incremental Computation with Names](https://arxiv.org/abs/1503.07792), and
[Efficient Parallel Self-Adjusting Computation](https://arxiv.org/abs/2105.06712).

**FLUI response:** do not revive the old issue that proposes putting
`Arc<RwLock<ElementTree>>` under Rayon. Parallel execution requires frozen
inputs, explicit dependencies, generation-checked outputs, a serial reference
executor, and measured crossover thresholds.

### Frame pacing is per surface, not a public window mode

Modern systems select presentation cadence from display capabilities, OS
policy, thermal/power state, visibility, actual CPU/GPU duration, and requested
frame-rate ranges. Extra queued frames may raise throughput but also add
input-to-display latency. Android's frame-pacing guidance switches between
pipelined and non-pipelined operation and prevents buffer stuffing.

Sources: [Android Frame Pacing](https://developer.android.com/games/sdk/frame-pacing),
[Apple display-link range](https://developer.apple.com/documentation/quartzcore/cadisplaylink/preferredframeraterange),
and [DXGI maximum frame latency](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgidevice1-getmaximumframelatency).

**FLUI response:** there is no public `OnDemand`/`Continuous`/`External` window
mode. Work requests frames; the runtime coalesces demand. Each presentation
tracks its own display deadline internally. The standard is automatic pacing;
advanced users may bound target frame rate and frames in flight.

## Accessibility is an independent output tree

Custom-rendered toolkits must provide an accessibility tree; pixels are not an
accessible interface. AccessKit exists specifically to map a toolkit-owned tree
to platform APIs and is already used by Bevy, egui, Slint, and Xilem. WAI-ARIA
also defines the accessibility tree as a structure parallel to the DOM, with
explicit inclusion, identity, relationships, focus, and actions.

Sources: [AccessKit](https://accesskit.dev/),
[WAI-ARIA accessibility tree](https://www.w3.org/TR/wai-aria-1.3/#accessibility_tree),
and [Apple custom controls](https://developer.apple.com/documentation/appkit/custom-controls).

**FLUI response:** semantics identity, visibility, focus, actions, and geometry
must be presentation-scoped and committed from the same authoritative frame
snapshot. Multi-window support is incomplete until assistive actions route to
the exact live presentation.

## Current FLUI assessment

### Strong foundations already present

- ADR-0027 defines owner-affine `UiRealm`, bounded typed commands, immutable
  handoff, generation checks, and shutdown reliability classes.
- ADR-0037 separates `WindowHost`, owner-local `PresentationState`, and
  `RasterOwner` instead of inventing a shared presentation god object.
- `UiRealm`, `PresentationState`, and owner-local interaction callbacks are
  structurally `!Send + !Sync`.
- `RealmId`, `PresentationId`, `FrameEpoch`, `SurfaceGeneration`, and resource
  generations already exist.
- `RasterOwner` has a capacity-one latest-frame-wins mailbox, typed acks, and a
  separate guaranteed shutdown completion.
- `SceneSnapshot` moves the scene by value instead of sharing an `Arc<Scene>`.
- Platform window events carry `WindowId` rather than an implicit current
  window.
- Frame pacing through FIFO present and `ControlFlow::Wait` has real Wayland
  evidence in ADR-0029.

These should be preserved and completed, not replaced.

### Critical contradictions still in code

1. **A second realm is forbidden.** `REALM_CLAIMED` and
   `UiRealmError::AlreadyExists` preserve a process-wide singleton reality.
2. **Binding ownership is still ambient.** `Scheduler`, rendering, painting,
   and semantics use singleton access; `AppBinding` is a leaked thread-local
   instance.
3. **The presentation is singular.** `UiRealm` stores one `PresentationState`;
   the element tree is not yet the forest required by ADR-0037.
4. **Presentation addressing is incomplete.** `SceneSnapshot` carries
   `RealmId` but not `PresentationId`, despite being documented as a
   per-presentation package.
5. **The render tree is lock-shaped rather than owner-shaped.** `PresentationState`,
   elements, `RenderTree`, and bindings propagate `Arc<RwLock<PipelineOwner>>`.
6. **False thread-safety remains.** `RenderObject` requires `Send + Sync`, and
   `NodePtr` has unsafe `Send`/`Sync` implementations guarded by runtime thread
   checks.
7. **The platform trait combines incompatible authorities.** `Platform` is
   `Send + Sync` while also exposing owner-affine window creation and callbacks.
   ADR-0039 identifies the correction but is still Proposed.
8. **Frame configuration lies.** `AppConfig::target_fps`, `vsync`, fullscreen,
   and debug paint are public while several are advisory or unwired. `Scheduler`
   defaults to 60 FPS independently of the active display.
9. **Worker policy is not implemented.** Every platform constructs a Tokio
   runtime sized to all CPUs; external executor injection is absent; priority
   is explicitly informational.
10. **Public locks leak through exceptions.** `Scheduler::budget` returns a
    `MutexGuard` under a legacy port-check exemption.
11. **Platform behavior is weakly verified.** Win32 and AppKit are type-checked
    but not run in CI; the platform crate test suite is excluded because of an
    unresolved heap-corruption investigation.
12. **Recovery is not transactional.** Some layout panics are caught after
    mutation and recover into degraded partial state. Error containment needs
    a last-known-good commit boundary, not a zero-value substitute.

## Target contracts

### Stable concepts

- `App`: application definition and shared application-level dependencies.
- `Window`: public native presentation request and handle.
- `View`, `State`, `Model`, `Context`, `Service`, `Subscription`, and
  `Transaction`: conventional author-facing vocabulary when those concepts
  have real consumers.
- `run_app`: batteries-included entry point.
- An advanced host-driven runtime entry point after it has two real consumers.

`UiRealm`, `PresentationState`, `WindowHost`, scheduler internals, command
lanes, and raster mailboxes remain implementation vocabulary. They should not
be made public merely to make tests or embedding convenient.

### Ownership and commit

- One logical writer for each mutable UI realm.
- One owner-local presentation nucleus for each presented root.
- One authoritative `WindowId -> (RealmId, PresentationId)` map.
- Worker jobs receive owned immutable input and return owned versioned output.
- Cross-thread results commit only at explicit idle anchors.
- Raster consumes an immutable presentation-addressed snapshot.
- Closing is monotonic, cancellable, and bounded by deadlines; persistence is
  journaled before graceful shutdown begins.

### Scheduling

- `UpdateScheduler` is realm-owned and controls logical UI transactions.
- `FrameClock` is presentation-owned and follows the actual display/surface.
- Raster scheduling owns frames-in-flight and GPU backpressure.
- Frame demand is derived from dirty state, animations, media, or an external
  host; it is not a window mode.
- Work is classified by deadline and behavior, not arbitrary user priority:
  frame-critical compute, asynchronous compute, I/O, and durable services.
- Queues are bounded; invalidations coalesce; stale generation results drop;
  correctness-critical completion has a dedicated reliable path.

### Embedding and future game support

Core remains engine-neutral. The architecture must permit:

- FLUI-owned and host-owned event loops;
- host-provided executor/task pools;
- a host-provided `wgpu::Device` and `wgpu::Queue` where ownership permits;
- rendering to a native surface, texture, or external render target;
- explicit input injection and presentation timing;
- UI as an overlay or texture in a later engine integration.

No ECS, physics, scene graph, fixed simulation timestep, `flui-game`, or
`flui-3d` crate should be added now. Those belong in separate crates after the
host runtime and renderer interop have been validated by real consumers.

## Decisions to reject now

- Shared mutable UI trees under `Arc<RwLock<_>>`.
- Public generic "run this closure on the UI thread" executors.
- A public `StateDomain`, `UiRealm`, or `PresentationRuntime` abstraction.
- One implicit current window, focus manager, scheduler, or GPU service.
- Per-window public execution modes.
- A mandatory Redux, signals, ECS, or actor state model for applications.
- Parallel build/layout based only on subtree shape.
- Unbounded command queues or silent drops of correctness-critical work.
- Stable APIs whose fields are advisory or ignored.
- A game engine inside the widget framework.

## Architecture proof

The milestone should end with one adversarial reference application rather than
several toy counters. The proof application is a multi-window editor shell with:

- shared documents and configuration;
- independent window navigation, focus, selection, and frame cadence;
- a background index/search worker;
- image/text asset work and cancellation;
- an embedded realtime wgpu viewport using host-driven runtime APIs;
- window restoration and graceful close with unsaved work;
- accessibility trees and actions for every window;
- deterministic headless replay of the same input/commit sequence.

This single application exercises the normal desktop, high-scale, background,
multi-window, and future game-embedding requirements without making FLUI itself
a code editor or game engine.

## Conclusion

The market is not converging on "make every UI node multithreaded." It is
converging on explicit ownership, immutable render representations,
transactional commits, phase-aware invalidation, hostable runtimes, adaptive
presentation timing, and tooling that makes behavior observable.

FLUI already selected most of the right concepts. Its risk is allowing the
transitional singleton and lock-based implementation to become the public
contract before the accepted runtime model is real. The companion execution
plan sequences that correction before stable multi-window or embedding APIs are
published.
