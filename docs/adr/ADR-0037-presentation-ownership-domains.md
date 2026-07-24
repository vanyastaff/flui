# ADR-0037: Presentation ownership domains — three physical owners without a mediation layer

*A presentation is one identity coordinated by three physical owners: an event-loop-owned `WindowHost`, an owner-thread `PresentationState` inside its `UiRealm`, and the existing engine `RasterOwner`. There is no fourth runtime object proxying between them.*

---

- **Status:** Accepted
- **Date:** 2026-07-23
- **Deciders:** @vanyastaff
- **Scope:** `flui-app` composition; presentation identity and routing protocols; `flui-platform` window delivery; per-presentation input, frame, render, and semantics ownership; the `flui-engine` raster boundary
- **Clarifies:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) — `PresentationRuntime` is the logical presentation contract, not a new shared object or crate; this ADR fixes its physical ownership, identity, lifecycle, and one-to-many gate
- **Supersedes in part:** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §§3–4 and §6 — removes the process/thread-local registry, `OpaqueWindowHandle`, downcast, and application IME intermediary while retaining the `ImeEvent`, stale-token, and `PlatformTextInput` behavioral contracts
- **Supersedes in part:** [ADR-0032](ADR-0032-ime-cursor-area-single-rect-reduction.md) only where it names `AppBinding`/`TextInputPlatformBridge` ownership and routing — its global-rect reduction, per-attach loop, geometry, and lifecycle behavior remain accepted
- **Related:** ADR-0003 (owner-local tree borrowing), ADR-0021 (post-frame capability), ADR-0029 (frame pacing), ADR-0033 (composing-region rendering), ADR-0035 (application lifecycle)

---

## Verdict

> **ACCEPTABLE after reshape.** The direction in ADR-0027 remains sound, but implementation is accepted only after presentation ownership is reshaped into the three explicit physical owners below. No `Bridge`, `Adapter`, opaque `Any` payload, callback bundle, service locator, or generic UI executor may survive as the way those owners communicate.

This is a breaking pre-1.0 correction. Compatibility with the transitional `AppBinding`, global interaction registries, and IME intermediary is explicitly not a goal. The clean ownership path replaces them atomically; there is no deprecated forwarding layer.

This ADR accepts an architecture, not an implementation-complete claim. The repository does not satisfy all of these invariants at the time of acceptance.

## Context

ADR-0027 correctly separated a realm's serial UI transaction from platform and raster work, but its name `PresentationRuntime` left two materially different interpretations open:

1. a logical presentation whose state is physically owned where each operation must run; or
2. one cross-thread object containing window, UI, and raster handles and forwarding calls among them.

The second interpretation recreates the current problems under a cleaner name. It becomes a handle bag, encourages `Arc<Mutex<_>>`, makes shutdown an ordering convention, and needs intermediary objects whenever two sibling crates cannot depend on each other. The transitional IME path demonstrates the failure mode: a global registry stores an opaque window, application code downcasts it, and a separate object synchronizes two independently selected "current" owners. It cannot make window replacement, multi-window routing, or teardown correct by construction.

The Kimi K3 architecture audit also surfaced the same broader ownership fault in focus, mouse tracking, gestures, semantics action routing, scheduling, and the render pipeline: process/thread-local selection and erased handles hide which presentation owns mutable state. The corrective principle is not to add another facade. It is to make the owner and address part of every state transition.

FLUI's three-tree semantics remain Flutter-loyal. Presentation, window, process/thread, and concurrency topology are sanctioned leapfrog zones under ADR-0027 and the Prime Directive.

## Decision

### 1. One logical presentation, three physical owners

```text
Platform/event-loop lane
┌──────────────────────────────┐
│ AppRuntime                   │
│ WindowId → (RealmId,         │
│             PresentationId)  │
│ ┌──────────────────────────┐ │
│ │ WindowHost               │ │  native window, event delivery,
│ └──────────────────────────┘ │  cancellable registrations
└──────────────┬───────────────┘
               │ closed, addressed events
               ▼
UiRealm owner lane
┌──────────────────────────────┐
│ UiRealm (!Send + !Sync)      │
│ ┌──────────────────────────┐ │
│ │ PresentationState       │ │  element root, pipeline, frame clock,
│ └──────────────┬───────────┘ │  input, focus, semantics, text input
└────────────────┼─────────────┘
                 │ owned snapshots / closed commands
                 ▼
Raster owner lane
┌──────────────────────────────┐
│ RasterOwner                  │  surface, renderer,
│ SurfaceGeneration authority  │  GPU submission
└──────────────────────────────┘
```

The three owners are:

| Owner | Physical location | Sole mutable authority |
|---|---|---|
| `WindowHost` | platform/event-loop lane | native window lifetime, platform event delivery, OS callback/registration lifetime, and redraw requests |
| `PresentationState` | realm owner thread, stored inside `UiRealm`, structurally `!Send + !Sync` | per-presentation UI root and pipeline, logical frame/input state, focus, gestures, mouse tracking, text-input session, and semantics |
| `RasterOwner` | the existing `flui-engine` owner lane | renderer, GPU surface, configure/present ordering, and `SurfaceGeneration` |

`PresentationRuntime` remains useful only as the architecture term for the aggregate contract formed by those owners. It does **not** become:

- a public struct;
- a new crate;
- an `Arc` shared among lanes;
- a collection of closures that stands in for those owners; or
- a fourth object through which all operations are forwarded.

The composition that creates and coordinates a presentation stays private to `flui-app`, the highest layer that already sees platform, interaction, rendering, and engine contracts. Lower crates expose deep owner-local primitives and closed protocol data, not an application composition root.

### 2. Presentation identity is explicit and generational

Add `PresentationId`, using the workspace's generational ID convention. A recycled slot never compares equal to the previous presentation incarnation.

`AppRuntime` is the only authority for:

```text
WindowId → (RealmId, PresentationId)
```

No second native-window map may live in `AppBinding`, `UiRealm`, an input registry, or a platform callback. `WindowId` is consumed at the platform demultiplexing boundary. Owner-lane and raster protocols carry `RealmId` plus `PresentationId`; they do not rediscover a presentation from a "current window".

Late events are harmless by construction:

- removal from the `AppRuntime` map stops new native events from routing;
- already-queued events carry the old generational `PresentationId`;
- the receiving realm drops an event whose realm/presentation incarnation is no longer live;
- raster channels are lifetime-specific, so a sender to a dead owner returns `OwnerGone`.

### 3. Cross-thread traffic has a closed vocabulary

When two physical owners run on different threads, they exchange owned `Send` data through bounded channels or dedicated one-shot completion channels. They never exchange UI closures or a generic "run this on the UI thread" command.

The protocol families are closed enums with presentation addressing, for example:

```text
PlatformToUi
  Input
  FocusChanged
  Resized
  ScaleFactorChanged
  RedrawRequested
  CloseRequested
  Suspended
  Resumed

UiToPlatform
  RequestRedraw
  SetCursor
  BeginClose

UiToRaster
  AttachSurface
  ConfigureSurface
  SubmitFrame
  Suspend
  Resume
  Close

RasterToUi
  SurfaceAttached
  SurfaceConfigured
  Presented
  Dropped
  DeviceLost
  Closed
```

Exact Rust variant payloads are settled with their implementing change, but these protocol constraints are fixed:

- every routable message contains or is structurally bound to the exact `RealmId` and `PresentationId`;
- every queue is bounded according to ADR-0027's reliability classes;
- shutdown completion uses a dedicated one-shot, never a possibly-full telemetry lane;
- no payload contains `dyn Any`, a native window hidden as an opaque value, `Box<dyn FnOnce()>`, or an arbitrary executor job;
- owner-local event dispatch may be a direct function call when lanes are co-located, but it must consume the same typed event and preserve the same ordering.

`winit::event_loop::EventLoop` is `!Send + !Sync`; cross-thread wake goes through its typed `EventLoopProxy`. That is an ownership constraint, not an inconvenience to erase behind locks.

### 4. `PresentationState` is the owner-local presentation nucleus

`PresentationState` is private to `flui-app`, stored by value inside its `UiRealm`, and structurally `!Send + !Sync` through its owner-local fields. It owns exactly one presentation's:

- element-forest root entry;
- `PipelineOwner`, render root, and presentation-specific layout constraints;
- frame clock, redraw coalescing state, visibility, and pacing state;
- `FocusManager`/focus tree;
- root gesture binding and arena;
- `MouseTracker` and hover state;
- a weak lifetime capability to the exact `PlatformWindow` for direct
  window-scoped commands while the platform and UI lanes are co-located; it
  cannot keep the native window alive;
- text-input session state plus the direct `Arc<dyn PlatformTextInput>` capability, when the window provides one;
- semantics owner, stable semantics-to-render identity mapping, and pending accessibility updates;
- cached raster acknowledgements, including the last `SurfaceGeneration` issued by `RasterOwner`, but not authority to mint that generation.

These fields are not fetched from TLS and are not selected through a process-global "current" object. Code running for a presentation receives an owner-local concrete capability derived from that `PresentationState`.

`FocusNode` attachment records a `Weak` reference to the exact presentation's focus owner, following Flutter's manager-attached-node behavior without copying Flutter's process-global binding topology. Gesture recognizers, mouse annotations, text fields, and semantics nodes follow the same rule: identity and owner are established on attachment, not discovered later from ambient state.

### 5. Text input is direct ownership plus a concrete weak capability

The accepted text-input behavior remains:

- one active client per presentation;
- attach replaces the previous client;
- a monotonic/non-zero connection token identifies the attach;
- detach is effective only when its token is still current;
- IME events are delivered only to the current client;
- every attach is a fresh cursor/composing session.

The ownership changes completely:

1. Composition receives the exact `Arc<dyn PlatformWindow>` once.
   `PresentationState` derives the direct
   `Option<Arc<dyn PlatformTextInput>>` and retains only a
   `Weak<dyn PlatformWindow>` for co-located window commands. It never stores a
   strong window owner for IME and never downcasts an opaque window.
2. `flui-interaction` provides a concrete owner-local `TextInputOwner` and `TextInputHandle { owner: Weak<TextInputOwner> }`. The handle is a narrow lifetime capability, not a closure bundle.
3. `TextInputOwner` holds the active client/token and a revisioned desired state: enabled/disabled, cursor-area rect, and the data required to route the current IME session.
4. At the end of an input transaction and at the permitted after-frame anchor, `PresentationState` compares the revision and applies the desired state directly to its owned `PlatformTextInput`. This occurs before control is yielded back to the event loop. There is no independently selected window and no intermediary object.
5. A dead weak owner produces a typed `OwnerGone`; a stale detach is a successful no-op; a replaced session cannot disable the current one.

The application-level `ImeBackend`, `TextInputPlatformBridge`, `OpaqueWindowHandle`, global `TextInputRegistry`, and `as_any`/downcast path are deleted. Tests observe a constructor-injected recording `PlatformTextInput`; they do not recover a fake with `Any`.

This owner-boundary synchronization is not an adapter: it is a method on the sole state owner applying its own revisioned state to a capability it directly owns. There is no extra identity, lifetime, registry, or forwarding object.

### 6. Focus, gestures, mouse tracking, and keyboard routing are presentation-local

Each `PresentationState` owns one complete input domain:

- keyboard events enter through `(RealmId, PresentationId)` and traverse that presentation's focused leaf to root;
- focus requests use the manager recorded when the node was attached;
- pointer events enter the presentation's one root gesture binding and arena;
- hover diffing reads and updates only that presentation's `MouseTracker`;
- cursor changes use `cursor_icon::CursorIcon` end to end and target the weak
  handle to that exact window; a closed window drops the update rather than
  falling back to a process-global cursor;
- deactivation/suspension clears or freezes state according to the subsystem contract without affecting another presentation.

The current event-loop and UI owner lanes are co-located, so cursor application
is a direct call on the exact window. If those lanes split, the same operation
becomes the closed, presentation-addressed `UiToPlatform::SetCursor` command
defined above; no forwarding object is introduced in either topology.

There is no fallback that silently creates an isolated gesture arena, focus manager, mouse tracker, or text-input registry when attachment is missing. Missing ownership is an invariant error on framework-owned paths and a typed construction/attachment error on public embedder paths.

### 7. Semantics commands route to the exact pipeline owner

Platform accessibility actions become closed, presentation-addressed commands:

```text
SemanticsCommand {
    realm_id,
    presentation_id,
    semantics_node_id,
    action,
}
```

The realm routes the command to the exact `PresentationState`, which resolves the stable semantics ID through that presentation's exact `PipelineOwner`. Resolution and invocation use two borrow phases:

1. borrow the pipeline/tree only long enough to validate the generational identity and obtain the owner-local action target;
2. release that borrow;
3. invoke the action through the presentation's normal action/input path.

No action invocation runs while the render tree is borrowed. No process-global pipeline owner, `SemanticsActionBridge`, `Any` payload, or fallback scan participates. A semantics ID from another presentation or a recycled generation is rejected and traced.

### 8. `RasterOwner` is the sole `SurfaceGeneration` authority

`WindowHost` reports native size/scale/lifecycle changes; it does not mutate a GPU surface. `PresentationState` requests surface operations; it does not increment `SurfaceGeneration`. Only `RasterOwner`, in the ordered command stream that actually attaches or reconfigures the surface, can mint the next generation.

The sequence is:

1. an addressed attach/configure command reaches the presentation's `RasterOwner`;
2. the raster owner mutates the surface and advances `SurfaceGeneration`;
3. its acknowledgement returns the issued generation;
4. `PresentationState` caches that acknowledged value for subsequent `SceneSnapshot`s;
5. a snapshot with a stale generation is rejected before render and acknowledged as `SurfaceOutdated`.

There is one counter and one authority. Surface generation is never predicted by the UI lane and never duplicated in `WindowHost`.

### 9. Lifecycle is explicit and monotonic

Each presentation has exactly this lifecycle:

```text
Created ──surface ack──▶ SurfaceAttached ──suspend──▶ Suspended
   │                         ▲                         │
   │                         └────────resume/ack──────┘
   └──────────────────────┐
                          ▼
SurfaceAttached/Suspended ──close──▶ Closing ──teardown ack──▶ Closed
```

The normative states are:

| State | Allowed work |
|---|---|
| `Created` | identity registration and surface-attach request; no input dispatch or frame submission |
| `SurfaceAttached` | input, build/layout/paint, semantics, redraw, and frame submission |
| `Suspended` | owner-local state updates may queue according to policy; no surface frame submission; platform/raster resources may be reduced |
| `Closing` | no new input or frames; mapping removed, callbacks cancelled, IME disabled, owner jobs cancelled, raster shutdown awaited |
| `Closed` | terminal; all capabilities fail with `OwnerGone`; ID slot may be recycled only with a new generation |

Resume does not claim `SurfaceAttached` until the raster owner acknowledges a valid surface generation. Surface replacement is an ordered configure operation, not a second hidden lifetime.

### 10. Platform callback lifetime is owned and cancellable

Install-only callbacks are forbidden. A backend must use one of two shapes:

- the platform event loop owns delivery and yields typed events while the `WindowHost` exists; or
- registration returns a cancellation/registration token owned by `WindowHost`, and teardown cancels it before the mapping and target owners can be destroyed.

A callback may capture only the typed sender and generational addresses needed for delivery. It may not capture `UiRealm`, `PresentationState`, `PipelineOwner`, or an application binding. `Closing` cancels registrations and prevents new delivery; queued old-generation events are then safely dropped.

Web RAF, native resize observers, accessibility callbacks, and platform text-input callbacks are subject to the same rule. "Installed until process exit" is not a lifecycle contract.

### 11. One realm with multiple presentations is gated by an element forest

The architecture permits `UiRealm 1 → N PresentationState`, but current single-root storage must not be relabeled as multi-presentation support.

Before FLUI claims or exposes one-realm/multi-presentation support, the element layer must provide and verify an element forest with:

- one presentation-keyed element root per live `PresentationId`;
- one presentation-specific render root and `PipelineOwner` per forest root;
- realm-local GlobalKey rules defined across the entire forest;
- lifecycle/reconciliation scheduling that can dirty one root without accidentally rebuilding another;
- root removal that disposes only its presentation subtree;
- tests with different constraints, focus, semantics, and frame cadence on two simultaneous presentations.

Until those invariants land, `UiRealm` enforces at most one live `PresentationState` and rejects a second attachment explicitly. Cloning one element tree into two pipelines, sharing one pipeline across surfaces, or quietly rebuilding two independent roots under a one-root API does not satisfy this gate.

### 12. No new `flui-presentation` crate

A new `flui-presentation` crate is rejected now. The proposed object would either:

- be an anemic bag of handles because the real state must remain in platform, realm, and engine owners;
- depend upward on application policy and downward on nearly every subsystem;
- force lateral dependency edges or trait indirection merely to compile; or
- become the forbidden fourth owner.

`flui-app` is already the composition root and may privately coordinate the three owners without changing the public layer graph. `flui-binding` and headless tests compose the same lower-level owner primitives for deterministic testing; a test harness is not evidence that application composition belongs in a new shared crate.

A future extraction requires a deep, policy-free abstraction with at least two production consumers and no new dependency inversion. It must receive its own ADR; this decision does not pre-authorize it.

## Consequences

### Positive

- Window, UI, and GPU lifetimes are independently correct and jointly addressable.
- Multi-window input cannot accidentally reach a process-global "current" owner.
- Rust's type/ownership system carries the single-writer invariant instead of lock discipline and comments.
- Surface staleness has one measurable authority.
- Focus, IME, semantics, gesture, and hover state can be tested for cross-presentation isolation.
- Shutdown becomes a finite protocol rather than a cascade of weak assumptions.
- The architecture supports dedicated owner threads where platforms allow them and same-thread owners where they require them, without changing widget semantics.

### Costs

- This is a wide breaking migration across `flui-app`, interaction ownership, platform delivery, and tests.
- Existing singleton-based convenience APIs and tests must be rewritten, not forwarded.
- Multi-presentation remains intentionally unavailable until element-forest storage is real.
- The private `flui-app` composition must resist accumulating policy-free domain logic; such logic stays in its lower owner crate.

## Rejected alternatives

| Alternative | Why rejected |
|---|---|
| New `flui-presentation` crate | Premature and anemic; either forms dependency cycles or becomes a handle bag/fourth owner |
| Put presentation composition in `flui-platform` | Layer inversion: platform delivery would own widget/render/input policy and could not remain an embedder substrate |
| One monolithic cross-thread runtime | Requires locks, erased handles, or forwarding for owner-affine state; teardown and reentrancy become convention-driven |
| Preserve the current intermediaries behind deprecated APIs | Rejected by the maintainer: it keeps dual ownership and allows new code to continue selecting the wrong current window/realm |
| Process/thread-local focus, gesture, mouse, or IME state | Cannot represent two realms on one thread, leaks state across tests/presentations, and hides the required owner relationship |
| Generic UI executor carrying closures | Open-ended authority, unreviewable ordering/reentrancy, no protocol backpressure classification, and accidental capture of owner-local state |
| `Arc<RwLock<PresentationRuntime>>` | Makes the compiler permit ownership violations and replaces explicit sequencing with a deadlock-prone lock graph |

## Implementation order and acceptance gates

The migration is intentionally breaking and does not install compatibility shims:

1. Introduce generational `PresentationId`, the sole `AppRuntime` window map, lifecycle state, and closed event addresses.
2. Materialize private owner-local `PresentationState` inside `UiRealm`; move focus, gesture, mouse, text-input, semantics, frame, and pipeline state into it.
3. Replace callback installation with event-loop-owned delivery or cancellable registrations.
4. Delete global/TLS interaction selection and every IME/semantics intermediary and opaque/downcast path.
5. Make `RasterOwner` the mechanically unique `SurfaceGeneration` writer and route configure/present acknowledgements by presentation.
6. Add teardown, stale-ID, wrong-presentation, reentrancy, and two-independent-realm tests.
7. Build and verify the element forest before enabling more than one presentation in a realm.

The reshape is complete only when all of the following hold:

- compile-time assertions prove `PresentationState`, its input owners, and UI tree are `!Send + !Sync`;
- cross-thread protocol payloads are `Send` and contain no owner-local references or executable closures;
- no production input path calls a global/TLS `FocusManager`, gesture arena, `MouseTracker`, or text-input registry;
- replacing a text-input token cannot disable or receive events for the replacement;
- an event for presentation A cannot mutate focus, hover, semantics, pipeline, or IME state in presentation B;
- semantics action resolution releases the pipeline/tree borrow before invocation;
- late platform callbacks after `Closing` are cancelled or rejected by generation;
- stale-surface frames are rejected against the generation issued by `RasterOwner`;
- a second presentation is rejected until the element-forest gate passes;
- focused crate tests, render harnesses, `just port-check`, workspace clippy, and the full non-platform test gate are green.

## Compatibility and reference verification

- **Rust:** verified against the repository-pinned `rustc 1.96.1`; owner affinity uses ordinary `Rc`/`Weak`/`RefCell` structure plus compile-time trait assertions, not unstable language features. The workspace's declared MSRV remains Rust 1.96.
- **winit:** verified against the workspace's 0.30.x contract (manifest requirement 0.30.12; lockfile resolution 0.30.13). `EventLoop` is `!Send + !Sync`; `EventLoopProxy` is the supported cross-thread wake path; `Window::request_redraw` coalesces redraw requests. These contracts support an event-loop-owned `WindowHost` and typed wake/control traffic rather than a shared event-loop object. See the official [`EventLoop`](https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoop.html), [`EventLoopProxy`](https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoopProxy.html), and [`Window`](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html) documentation.
- **Flutter behavioral reference:** Flutter's focus nodes retain their owning manager and key events traverse the focused node's ancestor chain; its text-input connection also has a single current connection with replacement/close identity semantics. FLUI keeps those observable behaviors while realm/presentation ownership deliberately diverges. See Flutter 3.44's [`focus_manager.dart`](https://github.com/flutter/flutter/blob/3.44.0/packages/flutter/lib/src/widgets/focus_manager.dart) and [`text_input.dart`](https://github.com/flutter/flutter/blob/3.44.0/packages/flutter/lib/src/services/text_input.dart).

No implementation parity is claimed by these reference checks. Each migrated widget-tree behavior still requires `.flutter/` source verification and a regression test under the repository's Definition of Done.
