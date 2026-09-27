# ADR-0037: Presentation ownership domains — three physical owners, no mediation layer

- **Status:** Accepted
- **Date:** 2026-07-23
- **Supersedes:** ADR-0030 in part — its original text-input ownership (process-global
  registry, opaque window handle, application IME bridge), since removed from that record
- **Superseded in part by:** ADR-0078 (capability acquisition)
- **Amended by:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (the
  `interaction -> platform` edge becomes `interaction -> platform-api`);
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) in part (§1 and §4:
  `PresentationState` and the realm that composes it live in `flui-runtime`, not `flui-app`;
  the ownership domains themselves are unchanged)

## Context

ADR-0027 separated a realm's serial UI transaction from platform and raster
work, but its term `PresentationRuntime` allowed two readings:

1. a logical presentation whose state is physically owned where each operation
   must run; or
2. one cross-thread object holding window, UI and raster handles and
   forwarding calls between them.

The second reading rebuilds the old problems under a cleaner name: a handle
bag that invites `Arc<Mutex<_>>`, makes shutdown an ordering convention, and
needs an intermediary whenever two sibling crates cannot depend on each other.
The original IME path showed the failure: a global registry stored an opaque
window, application code downcast it, and a bridge synchronized two
independently chosen "current" owners. Focus, mouse tracking, gestures,
semantics actions and the render pipeline had the same fault — thread-local
selection and erased handles hid which presentation owned mutable state.

The fix is not another facade. It is making the owner and the address part of
every state transition. Presentation, window, thread and concurrency topology
are leapfrog zones under ADR-0027; the three-tree semantics stay Flutter's.

## Decision

### 1. One logical presentation, three physical owners

```text
event-loop lane     AppRuntime ── WindowRegistry: WindowId → PresentationAddress
                    backend-owned PlatformWindow (native window, event delivery)
                         │ closed, addressed events (PlatformToUi)
                         ▼
realm owner lane    UiRealm (!Send + !Sync)
                    └─ PresentationState: element root, pipeline, frame clock,
                       input, focus, semantics, text input
                         │ owned snapshots / closed commands
                         ▼
raster owner lane   RasterOwner: surface, renderer, GPU submission,
                    SurfaceGeneration authority
```

| Owner | Where | Sole mutable authority |
|---|---|---|
| Event-loop side: the backend's `PlatformWindow` plus `AppRuntime`'s `WindowRegistry` | platform/event-loop lane | native window lifetime, event delivery, OS callback registration, redraw requests, the native-window → presentation map |
| `PresentationState` | stored by value in its `UiRealm`; `!Send + !Sync` (statically asserted) | the presentation's UI root and pipeline, frame/input state, focus, gestures, mouse tracking, text-input session, semantics |
| `RasterOwner` (`flui-engine`) | raster owner lane | renderer, GPU surface, configure/present ordering, `SurfaceGeneration` |

`PresentationRuntime` survives only as the name of the contract those owners
form. It is not a public struct, a crate, an `Arc` shared among lanes, a bundle
of closures, or a fourth object that operations are forwarded through. The
composition that creates a presentation is private to `flui-app`, the lowest
layer that already sees platform, interaction, rendering and engine.

### 2. Presentation identity is explicit and generational

`PresentationId` is generational; a recycled slot never equals the previous
incarnation. `PresentationAddress { realm_id, presentation_id }`
(`flui-foundation`) is the address every routable message carries.

`AppRuntime`'s `WindowRegistry` is the only `WindowId → PresentationAddress`
map. `WindowId` is consumed at the platform demultiplexing boundary; realm-
facing code addresses a presentation only by `PresentationAddress`. No second
map lives in `UiRealm`, an input registry or a platform callback.

Late events are harmless by construction: removing the mapping stops new
routing; queued events carry the old generational address and are dropped by
the realm; raster channels are lifetime-specific, so a send to a dead owner
returns `OwnerGone`.

### 3. Cross-thread traffic has a closed vocabulary

Owners on different threads exchange owned `Send` data through bounded
channels or dedicated one-shot completions — never UI closures or a generic
"run this on the UI thread" job. `PlatformToUi` (platform → realm) and
`RasterAck` (raster → UI) are the shipped instances. The rules:

- every routable message carries, or is structurally bound to, its exact
  `PresentationAddress`;
- every queue is bounded per ADR-0027's reliability classes;
- shutdown completion uses a dedicated one-shot, never a lane that can be full;
- no payload contains `dyn Any`, an opaque native window, `Box<dyn FnOnce()>`
  or an executor job;
- when lanes are co-located, dispatch may be a direct call, but it consumes the
  same typed event in the same order.

winit's `EventLoop` is `!Send + !Sync`; cross-thread wake goes through its
typed `EventLoopProxy`. That is an ownership constraint, not something to hide
behind locks.

### 4. `PresentationState` is the owner-local nucleus

Private to `flui-app`, it owns exactly one presentation's element-forest root
entry; `PipelineOwner`, render root and root constraints; frame clock, redraw
coalescing, visibility and pacing state; focus manager; root gesture arena;
`MouseTracker`; a `Weak` reference to its exact `PlatformWindow` (it cannot
keep the native window alive); its text-input owner with the direct
`Arc<dyn PlatformTextInput>`; its semantics host; and the last acknowledged
`SurfaceGeneration`, cached but never minted.

None of this is read from TLS or selected through a process-global "current"
object. Code running for a presentation holds a concrete owner-local
capability derived from that `PresentationState`; where such capabilities may
be acquired is ADR-0078. A `FocusNode` records a `Weak` reference to its
presentation's focus owner on attach, keeping Flutter's manager-attached-node
behavior without Flutter's process-global binding; recognizers, mouse
annotations, text fields and semantics nodes follow the same rule.

### 5. Text input: direct ownership plus a concrete weak handle

The behavior of ADR-0030 §3 holds per presentation. Its ownership:

1. `PresentationState` derives `Option<Arc<dyn PlatformTextInput>>` from its
   window once, and keeps only a `Weak` to the window.
2. `flui-interaction` provides `TextInputOwner` and
   `TextInputHandle { owner: Weak<TextInputOwner> }` — a narrow lifetime
   capability, not a closure bundle.
3. The owner holds the active client, its token, and the desired
   enabled/cursor-area state, and applies it directly to the platform
   capability it owns.
4. A dead owner yields a typed error; a stale detach is a successful no-op; a
   replaced session cannot disable the current one.

There is no global registry, no `as_any` downcast and no application bridge.
Tests inject a recording `PlatformTextInput` through the constructor.

### 6. Input is presentation-local

Keyboard events enter by address and walk that presentation's focused leaf to
root; focus requests use the manager recorded at attach; pointer events enter
the presentation's one gesture arena; hover diffing touches only its
`MouseTracker`; cursor changes use `cursor_icon::CursorIcon` end to end and
target the weak handle to that exact window — a closed window drops the update
rather than falling back to a global cursor.

Missing ownership is never papered over by silently creating an isolated
arena, focus manager, tracker or text-input owner: it is an invariant error on
framework paths and a typed error on embedder paths.

### 7. Semantics actions route to the exact pipeline owner

A platform accessibility action arrives as a `SemanticsActionRequest`
addressed to the presentation whose snapshot the adapter exposes. The
presentation resolves the stable node identity against its own pipeline in two
borrow phases: borrow only long enough to validate identity and find the
target, release, then invoke through the normal action path. No action runs
while the render tree is borrowed. An identity from another presentation or a
recycled generation is rejected and traced.

### 8. `RasterOwner` is the sole `SurfaceGeneration` authority

The event-loop side reports native size, scale and lifecycle changes; it never
touches a GPU surface. `PresentationState` requests surface operations; it
never increments `SurfaceGeneration`. Only `RasterOwner`, in the command stream
that actually attaches or reconfigures the surface, mints the next generation
and returns it in its acknowledgement. A snapshot carrying a stale generation
is rejected before render as `SurfaceOutdated`.

### 9. Lifecycle is explicit and monotonic

```text
Created ──surface ack──▶ SurfaceAttached ⇄ Suspended
SurfaceAttached/Suspended ──close──▶ Closing ──teardown ack──▶ Closed
```

| `PresentationLifecycle` | Allowed work |
|---|---|
| `Created` | identity registration, surface-attach request; no input dispatch or frames |
| `SurfaceAttached` | input, build/layout/paint, semantics, redraw, frame submission |
| `Suspended` | owner-local updates per policy; no frame submission |
| `Closing` | no new input or frames; mapping removed, callbacks cancelled, IME disabled, owner jobs cancelled, raster shutdown awaited |
| `Closed` | terminal; capabilities fail with `OwnerGone`; the slot is recycled only with a new generation |

Resume does not claim `SurfaceAttached` until the raster owner acknowledges a
valid generation.

### 10. Platform callbacks are owned and cancellable

Install-only callbacks are forbidden. Either the event loop owns delivery and
yields typed events while the window exists, or registration returns a token
the event-loop side owns and cancels at teardown, before the mapping and the
target owners go away. A callback may capture only the typed sender and the
generational address — never `UiRealm`, `PresentationState`, `PipelineOwner` or
an application binding. Web RAF, resize observers, accessibility and text-input
callbacks follow the same rule; "installed until process exit" is not a
lifecycle.

### 11. Several presentations in one realm need an element forest

`UiRealm 1 → N PresentationState` is allowed only with a real element forest:
one element root, render root and `PipelineOwner` per live presentation;
realm-local `GlobalKey` rules across the forest; scheduling that can dirty one
root without rebuilding another; root removal that disposes only its subtree.
Cloning one element tree into two pipelines, or sharing one pipeline across
surfaces, does not qualify. The forest is ADR-0043's `PresentationForest`.

### 12. No `flui-presentation` crate

Such a crate would be an anemic handle bag (the real state stays in the
platform, realm and engine owners), would depend upward on application policy
and downward on almost everything, or would become the forbidden fourth owner.
`flui-app` is already the composition root. A future extraction needs a deep,
policy-free abstraction with two production consumers and its own ADR.

## Consequences

- Window, UI and GPU lifetimes are each correct on their own and jointly
  addressable; multi-window input cannot reach a global "current" owner.
- The single-writer invariant is carried by types (`!Send` owners, `Weak`
  handles), not by lock discipline.
- Surface staleness has one authority; shutdown is a finite protocol.
- Focus, IME, semantics, gesture and hover state are testable for
  cross-presentation isolation.
- The migration was breaking: singleton-based APIs and tests were rewritten,
  not forwarded. The private `flui-app` composition must resist accumulating
  domain logic that belongs in a lower owner crate.

## Alternatives rejected

| Alternative | Why |
|---|---|
| A `flui-presentation` crate | premature and anemic; a dependency cycle or a fourth owner |
| Composition in `flui-platform` | layer inversion: platform delivery would own widget/render/input policy |
| One cross-thread runtime object | needs locks, erased handles or forwarding for owner-affine state |
| Keep the old intermediaries behind deprecated APIs | keeps dual ownership and lets new code pick the wrong current window |
| Thread-local focus/gesture/mouse/IME state | cannot represent two realms on one thread; leaks across tests and presentations |
| A generic UI executor carrying closures | open-ended authority, unreviewable ordering, no backpressure class |
| `Arc<RwLock<PresentationRuntime>>` | lets the compiler permit ownership violations; a deadlock-prone lock graph |

Flutter reference points: focus nodes retain their owning manager and key
events walk the focused chain; text input has one current connection with
replacement semantics (`focus_manager.dart`, `text_input.dart`, Flutter 3.44).
FLUI keeps those behaviors; the ownership topology is its own.
