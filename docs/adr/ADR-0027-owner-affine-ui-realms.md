# ADR-0027: Owner-affine UI realms — a multi-threaded runtime of single-writer ownership domains

- **Status:** Accepted
- **Date:** 2026-07-11
- **Absorbs:** ADR-0027 (engine-wide threading architecture)
- **Refined by:** ADR-0037 (presentation ownership domains), ADR-0043 (per-presentation trees,
  realm `GlobalKeyScope`), ADR-0045 (the raster lane)
- **Amended by:** [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the runner's
  thread-local `AppRuntime` slot is a permanent `trampoline`, the host's only one; every other
  process-global is listed and gated by `cargo xtask globals`)

Mutable UI state is scoped to an explicit `UiRealm` — a single-owner UI session, structurally
`!Send + !Sync` — presented through one or more presentations and hosted by one `AppRuntime`.
Everything crosses threads only as typed `Send` capabilities, bounded ownership-transfer
channels, and immutable snapshots.

## Verdict

> **FLUI is a multi-threaded runtime built from single-writer ownership domains.** Each
> `UiRealm` has exactly one owner executor and performs its UI transaction serially. Multiple
> realms may execute concurrently. CPU-intensive pure work, asynchronous I/O, and
> rasterization execute outside the realm and communicate through bounded ownership-transfer
> channels and immutable snapshots.

Not a single-threaded framework and not a shared-memory multithreaded tree: single writer per
UI tree, real parallelism between realms, workers, the compositor and the GPU. The
single-writer transaction is not a language limitation: lifecycle, reconciliation,
parent-driven layout and paint order are causally ordered, and per-node parallelism buys
scheduler overhead and races, not throughput.

```text
Platform/Event-loop thread
        │ events (AppRuntime demux: presentation → realm)
        ▼
┌────────────────────────┐
│ UiRealm A owner        │ owner executor 1
│ build/layout/paint     │
└──────────┬─────────────┘
           │ SceneSnapshot (owned, immutable)
           ▼
┌────────────────────────┐   ┌────────────────────────┐
│ Compositor / Raster    │   │ UiRealm B owner        │ executor 2 (platform policy)
│ surfaces / GPU submit  │   │ independent UI tree    │
└────────────────────────┘   └────────────────────────┘

Worker pool: image decode · text shaping · SVG/path · tessellation · resource prep
Async I/O runtime: separate I/O workers
```

**Leapfrog zones.** Multi-window ownership, runtime/scheduling topology, concurrency
architecture and presentation architecture are not bound by Flutter. Flutter stays the
behavioral reference for widget-tree semantics, not for process, thread or window topology.

## Context

When this was decided, the whole frame ran inline on one platform event-loop thread behind
process-global singletons (app binding, scheduler, gesture and focus managers) wrapped in
`Arc<RwLock<_>>`, and most framework traits carried `Send + Sync` although no production code
moved a view, element, context or callback across threads — the bounds were forced by storage,
not by use. Cross-thread delivery was broken: the public foreground executor was an unbounded,
wake-less queue drained only by the Win32 pump. The earlier threading decision (ADR-0027) had
drawn the control-plane/data-plane boundary but answered only *which thread*, not *which owner
object*, and its `thread_local!` remedy could not express two realms on one thread or one realm
with two presentations.

Flutter keeps one `BuildOwner`, one GlobalKey registry and one `FocusManager` per process — a
consequence of its single-UI-isolate embedding. SwiftUI's `Scene`/`WindowGroup` are lifecycle
containers with independent state, React commits concurrent preparation atomically, and
Chromium separates the mutable main tree from a compositor snapshot synchronized by commit.

## Decision

### 1. Ownership model — three levels

```text
AppRuntime — process/application host (one per process)
├── platform event loop ownership + presentation→realm demux
├── SharedEngineServices (explicit, constructor-injected — not hidden globals):
│     GPU device/queue · ImageCache · font service · worker pools · async I/O runtime
├── application models / actors (shared business state, passed into realms explicitly)
└── UiRealm 1..N — independent UI session, single-writer owner, !Send + !Sync
    ├── update scheduler, post-frame lane, interaction dispatch, async driving
    ├── GlobalKey uniqueness scope (ADR-0043)
    ├── focus coordination across its presentations
    └── presentation 1..N — one per surface (ADR-0037)
        ├── native window | embedded view | headless surface
        ├── element tree + BuildOwner, PipelineOwner + render tree
        ├── focus tree + gesture/input state
        ├── FrameClock (vsync, refresh rate, visibility, throttling)
        └── SceneSnapshot producer → raster owner (SurfaceGeneration authority)
```

Instantiation is **policy, not architecture**: desktop default is one realm per window; fully
independent windows are N realms × 1 presentation; one session on several surfaces is 1 realm ×
N presentations; a headless test is 1 realm × a headless presentation. Realm count per owner
thread is an embedder policy (AppKit may serve several realms on the main thread; Win32, Linux
and headless may use distinct owner threads; wasm is sequential). The widget API never names a
thread.

`SharedEngineServices` is owned by `AppRuntime` and injected; sharing between realms is a
constructor decision. Scheduling splits by level: the realm's update scheduler (priorities,
transactions), each presentation's `FrameClock` (physical pacing — one window at 60 Hz,
another at 144 Hz, a background one frozen), and raster scheduling (GPU backpressure).

### 2. Thread-affinity model — the compiler states the rules

| Type | Contract |
|---|---|
| `UiRealm`, element and render trees, `PipelineCell`, views, contexts, UI callbacks | `!Send + !Sync` — single writer, structurally |
| `SceneSnapshot`, `Scene`, `LayerTree` | `Send`, moves by value — the immutable commit artifact |
| `WorkerJob<Input>` / `WorkerResult<Output>` | `Send`, owned immutable payloads |
| `UiCommandSender` | `Clone + Send + Sync` — enqueue-and-wake capability, closed vocabulary |
| `Renderer` | owned by exactly one raster owner, never shared |

A UI tree cannot be handed to a worker, workers receive owned inputs and return results
instead of writing into the tree, and cross-thread interaction exists only through sanctioned
capabilities. Negative bounds are pinned by `assert_not_impl_any!` or `compile_fail` tests;
positive ones by `assert_send`/`assert_sync` tests. A render object reaches its owner only
through an attachment-scoped `RenderInvalidationHandle`, never a stored `PipelineCell`, which
would close an `Rc` cycle.

Within one realm the transaction is serial: lifecycle, state mutation, reconciliation, build,
layout, paint order, focus/navigation, commit. What runs in parallel: different realms; raster
versus UI; image decode; font loading and shaping; path processing; tessellation; shader
preparation; asset I/O; heavy user computation.

### 3. Message flow and commit points

- **Commands and worker results commit only while the realm's scheduler phase is Idle** —
  before entering `drive_frame` or after it returns, never inside the frame transaction. One
  frame observes one committed state. The mid-frame microtask slot is reserved for ADR-0018
  `AsyncDriver` continuations; idempotent dirty-mark drains stay at their phase-start anchors.
- **Reentrancy gate:** platform callbacks deliver input synchronously in causal order; a
  callback that re-enters while the realm is mid-transaction (nested Win32/AppKit pump: modal
  resize, native dialogs) is queued into a realm-local ordered FIFO and applied at the next
  permitted anchor.
- **Wake contract:** enqueue-then-wake is one operation for the sender; the waker reaches the
  owner's event loop without spawning a thread (Win32 `PostMessageW` to a message-only HWND,
  AppKit run-loop source, winit `EventLoopProxy`, headless flag + pump).
- **Self-wake rule:** a drain that dirties the tree requests a frame; a realm never goes idle
  with a dirty tree.

### 4. Queues — reliability classes, not one FIFO

| Lane | Guarantee | Mechanism | When full |
|---|---|---|---|
| Control / shutdown completion | exactly once | one-shot channel per handshake | cannot fill |
| Owner inbox: worker results, framework commands | bounded, typed backpressure | bounded channel (256 default), bounded drain pass per batch | `try_send` → typed `ChannelFull` returning the rejected command |
| Coalesced invalidations (redraw/rebuild/repaint) | idempotent, latest state | atomic flag / set-dedup inbox | n/a |
| Frame snapshots | latest frame wins | single-slot mailbox; replacing an unstarted frame acks `Dropped{Superseded}` | never full |
| Frame telemetry acks | explicitly lossy | bounded `try_send`; drops traced | lossy by contract |
| Input | causal order, never coalesced or reordered | direct dispatch + the reentrancy FIFO (§3) | n/a |

No unbounded channels on runtime paths, no `Arc<Mutex<Vec<_>>>` mailboxes, no public generic
"run this closure" executor (§9).

### 5. SceneSnapshot and the raster boundary

Compositing produces an owned, immutable `SceneSnapshot` per presentation per frame:

```rust
pub struct SceneSnapshot {           // Send; moves by value; never Arc<Scene>
    pub realm_id: RealmId,
    pub epoch: FrameEpoch,
    pub surface_generation: SurfaceGeneration,
    pub damage: DamageRegion,
    pub scene: Scene,
}
```

- The raster owner solely owns the renderer (surface, device, queue).
- It is bound to one presentation and is the `SurfaceGeneration` authority: a
  reconfigure/resize bumps the generation before the next frame is accepted, and a frame whose
  generation mismatches is rejected before rendering with `SurfaceOutdated`.
- Acks (`Presented`, `Dropped`, `SurfaceOutdated`, `DeviceLost`) carry the frame identity and
  travel on the lossy telemetry lane; shutdown completion has its own one-shot channel.
- Which thread the raster owner runs on is ADR-0045's decision. The protocol is exercised by a
  threaded test harness, because under a synchronous raster owner its drop, stale-generation and
  shutdown paths are unreachable.

### 6. Identity, versioning, cancellation — freshness is per work class

- **Channel identity is the lifetime boundary.** A realm's channels are created with it and die
  with it; senders into a dead realm get `OwnerGone`. No epoch comparison across owner
  lifetimes exists to get wrong.
- **Freshness by work class** (a blanket `FrameEpoch` check would discard every long-running
  result during animation):

| Work class | Validity check at commit |
|---|---|
| Asset / decode | `ResourceGeneration` current on its `GenerationGate` |
| Snapshot computation (future) | input revision |
| Raster frame | `FrameEpoch` + `SurfaceGeneration` |
| Lifetime isolation | channel identity (+ generational `RealmId`) |

- `RealmId` is a generational id (a recreated realm never compares equal); the native window
  id stays platform-internal and `AppRuntime` owns the only native↔realm mapping. `FrameEpoch`
  is per realm; generational `ElementId`/`RenderId` keep protecting slot reuse.
- Every worker job carries a cancel-on-drop token. Realm disposal cancels its jobs; racing
  results hit dead channels or fail their freshness check.

### 7. Shutdown protocol (per realm)

0. `AppRuntime` detaches the realm's platform callbacks — delivery stops before teardown.
1. The realm stops accepting frames; the owner inbox flips to drain-and-refuse (`OwnerGone`).
2. Worker jobs are cancelled; in-flight results hit the refused inbox or fail freshness.
3. The snapshot mailbox closes; the raster owner finishes or drops in-flight work and fires the
   one-shot shutdown completion.
4. Renderer and surface teardown happen in the raster owner before the window handle is
   destroyed.
5. The realm drops; surviving handles turn `OwnerGone`.

The web runner cannot yet detach: `WebPlatform::run` installs its animation-frame loop and
returns, and the platform has no detach hook enclosing that registration, so web keeps its
owner host for the page lifetime.

### 8. Focus, GlobalKey, and multi-realm semantics

- **Focus is per realm**: one focus tree per presentation; OS activation selects the active
  presentation; cross-realm focus does not exist by construction.
- **GlobalKey is realm-scoped**: unique within one realm. Within one presentation tree, a
  retake preserves element, state and render identity. Every presentation owns its own element
  tree (ADR-0043), so a keyed subtree moving between presentations or realms is an unmount plus
  a fresh mount (`init_state` re-fires). Cross-presentation state continuity would be a
  dedicated checkpoint/restore primitive, not a reparent.
- Tickers and animation controllers belong to their realm's scheduler; a subtree remounted in
  another realm re-registers with that realm's clock.

These are deliberate divergences from Flutter's process-global shape.

### 9. Public API consequences

- `View`, `BuildContext`, `ViewState`, `ElementBase`, the view-family traits, notifications and
  the `flui-foundation` callback aliases are not `Send + Sync`; widget authors use `Rc`,
  `Cell` and `RefCell` freely. `RenderObject`, `RenderSliver` and `ParentData` are not
  `Send + Sync` either: a `PipelineOwner` belongs to one presentation on one thread.
- **Closed command vocabulary**: the cross-thread surface is domain-specific — coalesced redraw
  requests, owner-queued hot reload, stamped worker results, navigation commands, and the raster
  handle verbs. The run-a-closure primitive is crate-private; a public arbitrary-closure
  executor is rejected as a standing constraint. Raw channels never appear in public
  signatures.
- `UiRealm`, its dispatcher and command protocol are `pub(crate)` in `flui-app`, not an
  embedder API. A public realm/runtime surface is designed separately; it is not obtained by
  making these types public.
- Every public type documents its thread affinity and where its callbacks run. Nothing goes
  `pub` for tests. Capability acquisition follows ADR-0078.

### 10. Parallel layout and repaint (absorbed from ADR-0027)

Parallel layout and parallel repaint are not part of this design. They become worth
revisiting only when all of the following hold, and then as their own ADR: a render object can
opt into laying out children it has proven independent (the fork point is user code, so the
engine cannot parallelize it unasked); relayout boundaries can prove a subtree independent;
text shaping has no process-wide lock; and a committed benchmark on a deep tree, a wide list
and realistic text shows the win. A typical frame's layout is about 1% of a 60 Hz budget,
where parallelism regresses it; even on list and grid shapes the ceiling is well under 15% of
the pipeline.

Non-goals: no parallel build or element tree, no control-plane parallelism, no shared global
thread pool, no display-list parallelism except future repaint-boundary subtrees, and no
reintroducing `Send` on the layout arena's node pointers without a fresh soundness argument.

## Alternatives rejected

| Option | Why rejected |
|---|---|
| **Shared-memory concurrent UI tree** (`Arc<RwLock<Tree>>`, `Send + Sync` everywhere) | No production code needs cross-thread tree access; the price is real (no `Rc`/`RefCell` in user state, lock graphs, non-deterministic lifecycle). Formally multithreaded, factually serialized on locks. |
| **Flutter's literal shape** — one process-wide runtime and registries, per-window render view only | Couples every window's state, keeps process-global registries and their test-serialization tax, and makes window isolation impossible to add later. |
| **Native window == runtime** | Cannot express embedded views, headless UI, offscreen rendering, tabs, external displays or one session across surfaces. 1 realm × 1 window is the default *policy*. |
| **Fully sequential status quo** | Leaves broken foreground dispatch, unbounded queues and singleton test locks, and no seam for decode or multi-window. |
| Epoch arithmetic across realm recreation | Rests on cross-lifetime monotonicity nothing enforces; channel identity makes the question unaskable. |
| One FIFO for all delivery guarantees | Shutdown drain-and-refuse would deadlock the completion handshake; telemetry could displace control messages. |
| `Arc<Scene>` handoff | Invites retained references and defeats latest-frame-wins accounting. |
| One god runtime object | `UiRealm`/`AppRuntime` own, wire and vend capabilities; behavior stays in subsystems. |
| Bevy's `NonSend`-in-a-`Send`-world | A natively `!Send` owner holding thread-affine state directly is simpler, and Bevy is moving away from the pattern. |

## Consequences

- Compiler-enforced single-writer domains, with real CPU/GPU parallelism around them and no
  locks or bounds in the widget API.
- Multi-window, embedders, headless, tabs and external displays are instantiation policies, not
  rewrites; the process-global singletons and their test locks are gone.
- Backpressure, shutdown, device loss and surface staleness have typed, lane-separated
  contracts.
- Bounded lanes can reject sends; producers handle typed, payload-returning backpressure.
- GlobalKey and focus scoping diverge from Flutter's process-global registries.

## Open questions

- Owner-affine platform callbacks (event-loop inversion): the runner's thread-local
  `AppRuntime` slot is the one host trampoline that OS callbacks reach (ADR-0097).
- Reentrancy-FIFO bounds and overflow policy for nested-pump event storms (Win32 modal resize).
- `AppRuntime` teardown versus late platform callbacks — step 0 of §7 verified race-free per
  backend.
- `DamageRegion` representation, and the input-revision freshness class, each designed with its
  first consumer.
