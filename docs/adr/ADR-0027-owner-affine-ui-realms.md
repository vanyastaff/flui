# ADR-0027: Owner-affine UI realms — a multi-threaded runtime of single-writer ownership domains

*Scope mutable UI state to an explicit `UiRealm` — a single-owner UI session, structurally `!Send + !Sync` — presented through one or more `PresentationRuntime` surfaces, hosted by one `AppRuntime`; everything crosses threads only as typed `Send + Sync` capabilities, bounded ownership-transfer channels, and immutable snapshots.*

---

- **Status:** Accepted (signed off by @vanyastaff, 2026-07-11; ADR-0002 marked Superseded atomically with this acceptance)
- **Date:** 2026-07-11
- **Deciders:** @vanyastaff
- **Scope:** workspace-wide — `flui-foundation`, `flui-scheduler`, `flui-view`, `flui-interaction`, `flui-rendering`, `flui-layer`, `flui-engine`, `flui-app`, `flui-platform`
- **Supersedes:** [ADR-0002](ADR-0002-engine-wide-threading-architecture.md) — absorbs its Send-boundary table, G1–G4 parallel-layout gates, non-goals, and migration order; completes it with the ownership model and commit protocol it did not decide
- **Related:** ADR-0018 (RebuildHandle seam), ADR-0021 (PostFrameHandle + `drive_frame` contract), ADR-0017 (between-passes fixpoint precedent), ADR-0003 (subtree-arena borrow discipline), ADR-0022/0026 (focus seams)
- **Governance:** requires the companion amendment to the Prime Directive / `STRATEGY.md` naming multi-window ownership, runtime/scheduling topology, concurrency architecture, and presentation architecture as sanctioned leapfrog zones (Flutter stays the behavioral reference for widget-tree semantics, not for process/thread/window topology)

---

## Verdict

> **FLUI is a multi-threaded runtime built from single-writer ownership domains.** Each `UiRealm` has exactly one owner executor and performs its UI transaction serially. Multiple realms may execute concurrently. CPU-intensive pure work, asynchronous I/O, and rasterization execute outside the realm and communicate through bounded ownership-transfer channels and immutable snapshots.

> **FLUI scopes mutable UI state to an explicit `UiRealm`, not to the process and not intrinsically to a native window.** A `UiRealm` is a single-owner UI session containing the element state, lifecycle, reconciliation authority, local identity registry, and update scheduler. Native windows, embedded views, headless targets, and other presentation surfaces are represented by child `PresentationRuntime` instances. The default desktop policy creates one realm per window, while embedders may attach multiple presentations to one realm or create multiple isolated realms.

Not single-threaded FLUI; not a shared-memory multithreaded tree; a multi-threaded actor/snapshot runtime — single writer per UI tree, real parallelism between realms, workers, the compositor, and the GPU. The single-writer transaction is not a language limitation: lifecycle, reconciliation, parent-driven layout, and paint order are causally ordered; per-node parallelization buys scheduler overhead and races, not throughput (ADR-0002's quantified analysis stands, gates G1–G4 unchanged). Whether a dedicated raster/compositor thread ships is deferred to its own ADR behind the stable snapshot seam; the shipping baseline is a synchronous in-process raster owner behind the same mailbox + ack protocol, with a threaded test harness proving the protocol now.

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

Worker pool (threads 4..N): image decode · text shaping · SVG/path · tessellation · resource prep
Async I/O runtime: separate I/O workers
```

## Context

### What ADR-0002 decided and what it left open

ADR-0002 (Proposed, 2026-06-09) drew the Send boundary: control plane `!Send`/thread-affine with plain owned state; data plane (`RenderObject`, `Scene`, `LayerTree`, `ImageCache`, `BackgroundExecutor`) stays `Send`; planes connect by ownership transfer over bounded channels, never `Arc<Mutex<tree>>`; parallel layout gated behind G1–G4. All absorbed here unchanged. It answered *which thread*; it did not answer *which owner object*, did not define a worker-commit protocol, and its `thread_local!` singleton remedy cannot express two realms on one thread or one realm with two presentations.

### Today's reality (audited 2026-07-11; unchanged facts, condensed)

- The entire frame runs inline on one platform event-loop thread; no raster thread; frame core is tokio-free. All trees hang off the `AppBinding` `OnceLock` singleton (`flui-app/src/app/binding.rs:226-232`) with lock-as-ownership (`Arc<RwLock<PipelineOwner>>` shared by 3 holders; `Arc<Mutex<Renderer>>` in four runners).
- The public `Send + Sync` tax traces to `BindingBase: Sized + Send + Sync + 'static` (`flui-foundation/src/binding.rs:106`), forced by `static OnceLock` storage. Zero production code moves views/elements/contexts/callbacks across threads. `docs/FOUNDATIONS.md:110` (C5) already promises the drop; `flui-view/src/context/build_context.rs:49` still carries the bound. The GlobalKey registry is a process-global static (`flui-view/src/key/registry.rs:130`).
- Cross-thread delivery is broken today: `ForegroundExecutor` is an unbounded flume queue with no wake (`flui-platform/src/executor.rs:181`), drained only by the Win32 pump, **never drained on macOS**, thread-per-task under winit; no executor shutdown protocol; no thread-identity enforcement at the platform boundary.
- The correct machinery half-exists and is generalized, not replaced: `PipelineOwnerHandle` (bounded 256, typed `ChannelFull`/`OwnerGone`, wake-on-send, generation-stale drops), `RebuildHandle` set-dedup inbox, `PostFrameHandle`, `AsyncDriver` (one mid-frame poll, `debug_assert_ne!(phase, PersistentCallbacks)`), `Scene: Send` moved by value, `Renderer: Send + !Sync` single-mutator by convention, `RasterBackend` seam.

### Flutter reference and prior art

Flutter's frame body is one uninterruptible unit; external work integrates between frames via dirty-marking (`scheduler/binding.dart:453-459`; `widgets/image.dart:1239-1241`); the raster handoff is an owned immutable scene per view per frame (`rendering/view.dart:347-362`). Flutter keeps **one** `BuildOwner`/GlobalKey registry/`FocusManager` per process (`widgets/binding.dart:473-477`, `widgets/framework.dart:2922-2945`) — a consequence of its single-UI-isolate embedding, and the shape this ADR deliberately does *not* copy (see Alternatives). The realm/scene model matches modern systems: SwiftUI `Scene`/`WindowGroup` are lifecycle containers with independent state storage, not necessarily OS windows; React commits concurrent preparation atomically; Chromium separates the mutable main tree from an isolated compositor snapshot synchronized by commit.

### Forces

- **C5** (single-threaded `BuildContext`, GPUI-lease endgame — this ADR is its enabling shape), **C1** (state carries no bound beyond `'static`), **C8** (async delivers work *to* a frame, never runs *inside* one — the Idle-only commit generalizes it), **C9**/FR-036 (new erased handles join the registry).
- Port-check: SP-6 (no lock types in public API), #7 (single owner of wgpu resources), #22 (frame-capability scope), #3 (no `async fn` on frame verbs).
- The frame-phase machine is load-bearing (forward-only transitions; the pipeline occupies the PersistentCallbacks slot; `drive_async_tasks` phase assert). Commit points sit **outside** `drive_frame`.
- Pre-1.0: breaking changes cheap now, ossified after the catalog ships (Prime Directive #2).

## Decision

### 1. Ownership model — three levels

```text
AppRuntime — process/application host (one per process)
├── platform event loop ownership + presentation→realm demux
├── SharedEngineServices (explicit, constructor-injected — not hidden globals):
│     GPU device/queue · ImageCache · font service (FONT_SYSTEM) · worker pools · async I/O runtime
├── application models / actors (shared business state, passed into realms explicitly)
└── UiRealm 1..N — independent UI session, single-writer owner, !Send + !Sync
    ├── BuildOwner + Element tree/forest
    ├── realm-local GlobalKey registry
    ├── lifecycle / state authority
    ├── UpdateScheduler (logical: priorities, state transactions, rebuild requests)
    ├── navigation
    ├── FocusCoordinator (active-presentation arbitration)
    └── PresentationRuntime 1..N — one per surface
        ├── native window | embedded view | headless surface
        ├── FocusTree + gesture/input state (per presentation)
        ├── FrameClock (vsync, refresh rate, visibility, throttling)
        ├── PipelineOwner + RenderView + RenderTree
        ├── SurfaceGeneration authority
        └── SceneSnapshot producer → compositor/raster seam
```

Instantiation is **policy, not architecture**: desktop default = one realm per window; fully independent windows = N realms × 1 presentation; one UI session on several surfaces (tabs, external display, immersive) = 1 realm × N presentations; headless test = 1 realm × headless presentation. Realm count per owner thread is an **embedder policy**: AppKit may serve several realms on the main thread; Win32/Linux/headless may place realms on distinct owner threads; wasm falls back to sequential. The widget API never names a thread.

`SharedEngineServices` is owned by `AppRuntime` and injected; sharing between realms is a constructor decision, not a global inevitability. Data-plane types stay `Send + Sync` per ADR-0002's table.

Scheduling splits by level: `UiRealm::UpdateScheduler` (logical update priorities and transactions) / `PresentationRuntime::FrameClock` (physical pacing — one window at 60 Hz, another at 144 Hz, a background scene frozen) / compositor-raster scheduling (presentation + GPU backpressure).

### 2. Thread-affinity model — the compiler states the rules

| Type | Contract |
|---|---|
| `UiRealm` | `!Send + !Sync` (raw-pointer `PhantomData` marker) — single writer, structurally |
| `SceneSnapshot` | `Send`, moves by value — the immutable commit artifact |
| `WorkerJob<Input>` / `WorkerResult<Output>` | `Send`, owned immutable payloads |
| `UiCommandSender` | `Clone + Send + Sync` — enqueue-and-wake capability, closed vocabulary |
| `Renderer` | `Send + !Sync` — movable to a raster owner, never shared |

The compiler guarantees: a UI tree cannot be handed to a worker; the renderer cannot be mutated from two threads; workers receive owned immutable inputs and return results instead of writing into the tree; cross-thread interaction exists only through sanctioned capabilities. This is stronger than `Arc<RwLock<App>>`-style "multithreading", which serializes on locks and grows a deadlock graph. Negative bounds are pinned by `compile_fail` doctests (the `flui-foundation/src/id.rs` idiom); positive ones by the `assert_send/assert_sync` test idiom.

Within one realm the UI transaction is serial: lifecycle, state mutation, reconciliation, build, layout, logical paint order, focus/navigation, commit. **Single-writer transaction, not single-threaded framework.** What actually runs in parallel: different realms; raster/compositor vs UI; image decode; font loading/shaping; SVG/path processing; tessellation; shader/pipeline preparation; asset I/O; heavy user computation; later — independent large repaint boundaries (G1–G4).

### 3. Message flow and commit points

- **Commands and worker results commit only while the realm's scheduler phase is Idle** — immediately before entering `drive_frame` and/or after it returns, never inside the frame transaction. One frame observes one committed state. The mid-frame microtasks slot remains reserved for ADR-0018 `AsyncDriver` continuations exclusively; idempotent dirty-mark drains stay at their existing phase-start anchors (marks cannot tear state).
- **Reentrancy gate (normative):** platform callbacks deliver input synchronously in causal order; if a callback re-enters while the realm is mid-transaction (nested Win32/AppKit pump: modal resize, native dialogs), the event is queued into a realm-local **ordered FIFO** and applied at the next permitted anchor. The frame transaction is uninterruptible by construction, not by hope.
- **Wake contract:** enqueue-then-wake is one operation from the sender's perspective; the waker's backend obligation is to reach the owner's event loop without spawning a thread (Win32 `PostMessageW` to a message-only HWND; AppKit `CFRunLoopSource`/`performSelectorOnMainThread`; winit `EventLoopProxy`; headless flag+pump). This retires the never-drained-on-macOS / thread-per-task-on-winit executor pathologies.
- **Self-wake rule:** a drain that dirties the tree must request a frame; a realm never goes idle with a dirty tree.

### 4. Queues — reliability classes, not one FIFO

One FIFO cannot serve messages with different delivery guarantees; lanes are separated so a correctness-critical completion can never be displaced or delayed by optional traffic:

| Lane | Guarantee | Mechanism | Full-behavior |
|---|---|---|---|
| Control / shutdown completion | exactly-once, guaranteed | dedicated one-shot channel per handshake | cannot fill (one-shot) |
| Owner inbox: worker results, framework commands | bounded, typed backpressure | crossbeam bounded (256 default), FIFO per drain batch, **bounded drain pass** (pre-read length — `try_iter` is not a snapshot) | `try_send` → typed `ChannelFull` **returning the rejected command** so the producer retries without rebuilding it |
| Coalesced invalidations (redraw/rebuild/repaint) | idempotent, latest-state | atomic flag / set-dedup inbox (existing `needs_redraw`, `external_inbox`, dirty-channel drain-dedup) | n/a (flag/set) |
| Frame snapshots | latest-frame-wins | single-slot mailbox; replacing an un-started frame acks `Dropped{Superseded}` | never full (slot) |
| Frame telemetry acks | **explicitly lossy/coalesced** | bounded try_send; drops traced | documented lossy — telemetry, not control |
| Input | causal order, never coalesced, never reordered | direct dispatch + reentrancy FIFO (§3) | n/a — pointer-move *sampling* stays per-pointer latest-move inside the gesture binding |

No unbounded channels on runtime paths; no `Arc<Mutex<Vec<_>>>` mailboxes; no public generic "run this closure" executor (§9).

### 5. SceneSnapshot and the raster boundary

Compositing produces an owned, immutable `SceneSnapshot` per presentation per frame (epoch-keyed; the earlier `FrameId` sketch is dropped — one counter, `FrameEpoch`, keys frames and acks):

```rust
pub struct SceneSnapshot {           // Send; moves by value; never Arc<Scene>
    pub realm_id: RealmId,
    pub epoch: FrameEpoch,           // subsumes Scene.frame_number
    pub surface_generation: SurfaceGeneration,
    pub damage: DamageRegion,        // Full today; fine-grained damage is additive
    pub scene: Scene,
}
```

- The **raster owner** solely owns the `Renderer` (Surface/Device/Queue): `Send + !Sync`, never behind a public `Arc<Mutex<_>>` (formalizes trigger #7; deletes the four `Arc<Mutex<Renderer>>` runner sites on migration).
- The raster owner is **bound to one presentation** and is the **SurfaceGeneration authority**: reconfigure/resize is an ordered command that bumps the owner's current generation *before* the next frame is accepted; a pending frame whose `surface_generation` mismatches is rejected **before rendering** with `SurfaceOutdated { epoch, stale, current }`. A handle from presentation A structurally cannot reach presentation B's surface (channel identity).
- Acks: `Presented { epoch }`, `Dropped { epoch, reason }`, `SurfaceOutdated { epoch, stale, current }`, `DeviceLost { epoch }` — every terminal outcome carries the frame identity — on the dedicated lossy telemetry lane. **Shutdown completion travels on its own one-shot channel** (§4 control lane); the raster owner never performs a blocking send.
- **Which thread the raster owner loops on is deferred to its own ADR** (platform-conditional; reverses an ADR-0002 step). The shipping baseline is a synchronous in-process raster owner behind the same mailbox + ack + one-shot seam. **The protocol is tested threaded now**: a test-only raster owner on a real thread (paced by barriers/channels, no sleeps) exercises latest-frame-wins + `Dropped` acks, stale-`SurfaceGeneration` rejection, ack interleaving, and the shutdown handshake — under the synchronous baseline those paths are unreachable and every protocol test would be vacuously green.

### 6. Identity, versioning, cancellation — freshness is per work class

Structural isolation first, version arithmetic second:

- **Channel identity is the lifetime boundary.** A realm's channels are created with the realm and die with it; recreating a realm mints new channels, and senders into the dead realm get `OwnerGone` at send. Cross-incarnation staleness is structurally impossible — no epoch comparison across owner lifetimes exists to get wrong.
- **Freshness by work class** (a blanket `FrameEpoch` check would discard every long-running result during animation — decode from frame N is still valid at N+5):

| Work class | Validity check at commit |
|---|---|
| Asset / decode | `ResourceGeneration` current on its `GenerationGate` |
| Snapshot computation (future) | input revision |
| Raster frame | `FrameEpoch` + `SurfaceGeneration` |
| Lifetime isolation | channel identity (+ generational `RealmId` mismatch drop, traced) |

- **IDs — two concepts, one mapping authority:** `RealmId` (`flui-foundation` `GenId`: slot + generation; a recreated realm never compares equal) identifies the realm incarnation in all protocol types; the platform crate's native `WindowId(u64)` remains the platform-internal native-handle key; `AppRuntime` owns the only native↔realm mapping. `FrameEpoch` is per-realm monotonic; `SurfaceGeneration` is owned by the presentation's raster seam; `ResourceGeneration` follows `GenId` conventions with `GenerationGate` as the canonical commit-time check (the `AsyncSlot` pattern generalized).
- Generational `ElementId`/`RenderId` keep protecting slot reuse (results for removed/recreated slab nodes drain to silent no-ops — existing behavior, existing tests).
- Every worker job carries a cancellation token (`TaskToken` cancel-on-drop precedent). Realm dispose cancels its jobs; racing results hit dead channels or fail their class's freshness check. Race-free disposal without worker-shared locks.

### 7. Shutdown protocol (per realm)

0. `AppRuntime` detaches the realm's platform callbacks — synchronous delivery stops before state teardown.
1. The realm stops accepting frames; the owner inbox flips to drain-and-refuse (`OwnerGone`).
2. Worker jobs are cancelled; in-flight results hit the refused inbox or fail freshness.
3. The snapshot mailbox closes; the raster owner finishes or drops in-flight work and fires the **one-shot shutdown completion**; the realm observes it (baseline: synchronous) — the completion cannot be displaced by telemetry (separate lane, §4).
4. Renderer/Surface teardown happens in the raster owner (single owner ⇒ single drop site) before the surface/window handle is destroyed.
5. The realm drops; surviving handles turn `OwnerGone` — "inert forever" queues are replaced by typed signaling.

### 8. Focus, GlobalKey, and multi-realm semantics

- **FocusCoordinator per realm**: one `FocusTree` per presentation; OS activation selects the active presentation; traversal never crosses presentations accidentally, but the realm knows which of its surfaces is active. Cross-*realm* focus does not exist by construction. Non-build focus call sites (~30 across flui-widgets: lifecycle registration, dispose-time removal, action dispatch) migrate to an owned `FocusHandle` designed in the focus migration step.
- **GlobalKey is realm-scoped**: unique within one `UiRealm`; reparenting inside a realm keeps today's semantics (existing reparent tests and bench are per-tree). Moving a GlobalKey'd subtree across realms is an unmount + fresh mount (`initState` re-fires). Within a 1-realm-N-presentations embedder, keys span all of that realm's presentations — Flutter's cross-view State preservation is recoverable by policy (one realm) rather than forbidden by architecture.
- These are deliberate divergences from Flutter's process-global shape, sanctioned by the governance amendment (leapfrog zones); they are not widget-tree behavioral changes — no C1–C9 contract moves.
- Ticker/animation consequence: the flui-animation scoped `Send + Sync` exception (absorbed from ADR-0002) gains a second retirement obligation — controllers re-home to their realm's `UpdateScheduler`; a subtree remounted in another realm re-registers with the new realm's clock as part of the fresh mount.

### 9. Public API consequences

- `View`, `BuildContext`, `ViewState`, `ElementBase`, the view-family traits, notifications, and the `flui-foundation` callback aliases **lose `Send + Sync`** (C5/C1). The data plane keeps its bounds (ADR-0002 table).
- **Closed command vocabulary**: the cross-thread surface is domain-specific — `request_redraw` (coalesced), `submit_result` (stamped worker results), the raster handle verbs. The generic run-a-closure primitive is crate-private (`pub(crate) invoke`); a public arbitrary-closure executor would bypass the typed commands and is rejected as a standing constraint. Raw channels never appear in public signatures (SP-6).
- **Stability posture**: `UiRealm`, `UiCommandSender`, `SceneSnapshot`, ack/error enums ship `#[non_exhaustive]` and documented **UNSTABLE — internal until the ADR-0027 ownership extraction completes**; they stay out of the prelude, and no embedder-facing stability is implied while the at-most-one transitional guard exists. `AppRuntime` as a public type appears only with the multi-realm instantiation policy (no speculative surface).
- Every public type documents its thread affinity and where callbacks execute. Nothing goes `pub` for tests.
- No new `BuildContext` capability here; if one appears, its token joins `check-frame-capability-scope.sh` in the same change (#22).

### 10. Absorbed from ADR-0002 (unchanged, normative)

The Send-boundary table; parallel-layout gates **G1–G4** and non-goals (no control-plane parallelization, no element/build-tree parallelization, no display-list parallelization except future `RepaintBoundary` subtrees, no shared global Rayon pool, no `check_thread` removal without per-worker-pool redesign + fresh Miri/Loom); the private repaint-boundary extension point (reserved, unimplemented); Phase 1.5 (image decode on the existing idle `BackgroundExecutor` — the first user of §6); the flui-animation scoped exception (retirement extended per §8); migration order Gesture → Widgets → App → Renderer-orchestration → Scheduler last.

## Alternatives considered

| Option | Why rejected |
|---|---|
| **A. Shared-memory concurrent UI tree** (`Arc<RwLock<Tree>>`, `Send + Sync` everywhere, parallel build/layout) | Zero production cross-thread tree access exists — the bounds are storage-forced; the price is real (no `Rc`/`RefCell` in user state, lock graphs, non-deterministic lifecycle). Every production retained-mode Rust GUI is `!Send`-by-default; only Servo parallelized layout and retreated. Formally multithreaded, factually serialized on locks. |
| **B. Flutter's literal shape** — one process-wide runtime, one BuildOwner/FocusManager/GlobalKey registry, per-window only RenderView+PipelineOwner | Copies a historical consequence of Dart's single-UI-isolate embedding into a runtime that has no such constraint; couples all windows' state, keeps process-global identity registries and their test-serialization tax, and makes window isolation impossible to add later without breaking the world. |
| **C. Hard `native window == runtime`** (per-window WindowRuntime owning everything; this ADR's own first draft) | Welds isolation to an OS abstraction: cannot express embedded views, headless UI, offscreen rendering, tabs/document scenes, external displays, XR surfaces, or one session across several surfaces. Realm isolation subsumes it: 1 realm × 1 window is the default *policy*. Rejected on review, 2026-07-11. |
| **D. Fully sequential status quo** | Leaves C5/C1 contradictions, broken foreground dispatch (macOS never runs queued tasks), unbounded queues, singleton test locks; no seam for decode, multi-window, or parallel headless. Cheapest today, most expensive when the catalog ships. |
| Epoch arithmetic across realm recreation | Rests on cross-lifetime monotonicity nothing enforces; channel identity makes the question unaskable. |
| Blanket `FrameEpoch` freshness for all worker results | Discards every long-running result during animation (decode from frame N is valid at N+5). Freshness is per work class (§6). |
| Acks through the owner inbox / one FIFO for all guarantees | Shutdown drain-and-refuse would deadlock the completion handshake; optional telemetry could displace control messages. Lanes by reliability class (§4). |
| `Arc<Scene>` handoff | Single-consumer `Send` value; sharing invites retained references and defeats latest-frame-wins accounting. By-value snapshot is also the Flutter seam shape. |
| Public generic UI-thread executor | Bypasses the closed command vocabulary; arbitrary closures on the owner defeat the typed-command discipline. Crate-private primitive only. |
| One god runtime object | Composition-root rule: `UiRealm`/`AppRuntime` own + wire + vend capabilities; behavior stays in subsystems. Standing constraint. |

## Consequences

**Positive**
- Compiler-enforced single-writer domains; real CPU/GPU parallelism (realm A builds while a worker decodes, raster presents the previous snapshot, realm B handles input, the GPU executes the previous command buffer) with zero locks or bounds in the widget API.
- Widget authors regain `Rc`/`Cell`/`RefCell`; C5/C1 retire; the Send costume (~300 incidental `Arc`s, scheduler lock fields, `unsafe impl`s) is deleted per ADR-0002 Phase 1.
- Multi-window, embedders, headless, tabs, external displays: instantiation policies, not rewrites. Singleton test locks die with the singletons.
- Backpressure, shutdown, device loss, and surface staleness get typed, testable, lane-separated contracts (today they have none).

**Negative / trade-offs**
- A large staged migration (~104 `instance()` call sites, ~40–50 bound edits, 4 runner rewrites) — each landing coherent, no shims left behind.
- Until singleton retirement, the realm shell enforces **at-most-one instance** (typed error) — a per-realm type over process-global internals would be a lying API; the guard and the runner's transitional TLS slot retire with the extraction.
- The baseline raster owner is synchronous — no frame-time win until the raster-thread ADR; stated plainly, with the threaded harness as the non-negotiable companion.
- GlobalKey/focus scoping diverges from Flutter's process-global registries (sanctioned leapfrog zone; recoverable by 1-realm policy where cross-surface identity is wanted).
- Bounded lanes can reject sends; producers handle typed, payload-returning backpressure.

**Follow-ups**
1. Raster/compositor-thread ADR behind the snapshot seam (frame pacing, platform conditions).
2. Parallel repaint boundaries only after G1–G4.
3. Fine-grained `DamageRegion` with the layer-diff work.
4. `flui-scheduler/AGENTS.md` refresh; engine/rendering ARCHITECTURE thread-safety tables cite this ADR; port-check #5/#7 whitelist entries retire with the engine refactors.
5. Event-loop inversion: the **target** is owner-affine platform callbacks (drop the `Send` bound on window-callback storage once callbacks are realm-owned); the runner's TLS slot is the sanctioned transitional form with that retirement condition.

## Migration strategy (dependency-ordered; every step a coherent landing)

1. **Contracts first (this change):** realm shell in `flui-app` with the at-most-one guard and bounded Idle-drained inbox wired into the live desktop frame loop; `RealmId`/`FrameEpoch`/`SurfaceGeneration`/`ResourceGeneration` + `GenerationGate` in foundation; `SceneSnapshot` in flui-layer; raster mailbox + lossy ack lane + one-shot shutdown + synchronous owner + threaded protocol harness in flui-engine; compile-time affinity contracts; invariant tests. No frame-transaction behavior change.
2. **Widgets singleton extraction + bound drop** — *re-sequenced 2026-07-11 after a landing attempt surfaced the real coupling.* The singleton exit and the GlobalKey registry recapture landed alone (they are bound-independent). The bound drop itself cascades past flui-view into three seams that must be designed first, in this order:
   2a. **Post-frame local lane**: `PostFrameHandle::schedule` requires `Send` callbacks (they queue in the process-global Scheduler), but the primary consumer — the ADR-0021 hero machinery — schedules view-capturing callbacks from the owner thread. Needs `schedule_local` (owner-local storage drained by `end_frame`; a realm field after the Scheduler step, a TLS lane transitionally).
   2b. **Gesture-callback token**: gesture callbacks (`Arc<dyn Fn + Send + Sync>`) are stored inside render objects, and `RenderObject: Send + Sync` stays (data plane). Dropping callback bounds therefore requires render objects to hold a `Send` dispatch token routed to realm-owned closures instead of the closures themselves — the step-3a gesture redesign, which must precede or accompany the bound drop.
   2c. **`NavigatorHandle` thread contract**: today documented `Send + Sync`, but it reaches route storage which holds views. Under this ADR navigation is realm-serial; the handle drops the cross-thread promise and cross-thread navigation rides `UiCommandSender`.
   With 2a–2c in place, the bound drop (`View`/`BuildContext`/`ViewState`/callback aliases + the ~24 machinery bounds + widgets-layer cascade: `Route`, `NavigatorObserver`, `WidgetsBindingObserver`, `ViewSeq` container bounds) lands as one wave, together with moving `widgets` into the realm.
3. **Remaining singleton retirement, ADR-0002 order:** Gesture (delete dual-instance) → App (`AppBinding` dissolves into `AppRuntime` + realm construction) → Renderer-orchestration (runners adopt the raster owner; `Arc<Mutex<Renderer>>` deleted) → Scheduler last (ticker-handle design first; `UpdateScheduler`/`FrameClock` split lands here). `FocusManager::global()` retires via `FocusCoordinator` + ambient capability + `FocusHandle`. The at-most-one guard, the TLS runner slot, and the test-lock family retire at the end of this step.
4. **Worker service + Phase 1.5 decode** on `BackgroundExecutor` under §6 freshness classes.
5. **PresentationRuntime reification** (FrameClock, per-presentation FocusTree, 1-realm-N-presentations) when the second presentation type (headless embedder or multi-window) becomes a real consumer.
6. **Raster-thread ADR**; then parallel repaint boundaries per G1–G4. `FONT_SYSTEM` sharding (G3) funded on its own merit; caches adopt `ResourceGeneration`.

## Unresolved questions

1. Scheduler ticker cancellation without `Arc<Scheduler>` vend (blocks the Scheduler step; carried from ADR-0002).
2. Per-backend wake primitives (`CFRunLoopSource` vs `performSelectorOnMainThread`; winit `EventLoopProxy` plumbing) and the two desktop event loops (native Win32 + legacy winit) both satisfying the wake contract during migration.
3. `FocusHandle` shape for the ~30 non-build focus call sites.
4. `CompositionCallback`: owner-routed completion tokens or deletion (no production consumer).
5. `DamageRegion` representation (region vs rect-list) — with the layer-diff work.
6. Reentrancy-FIFO bounds and overflow policy for pathological nested-pump event storms (Win32 modal resize).
7. `AppRuntime` teardown vs late platform callbacks — the §7 step-0 detach verified race-free per backend.
8. Input-revision freshness class for snapshot computations (§6) — designed with its first consumer.

## References

- ADR-0002 (absorbed; its ecosystem survey remains the research record for rejected option A), ADR-0018, ADR-0021, ADR-0017, ADR-0003, ADR-0022/0026.
- Locked contracts: `docs/FOUNDATIONS.md` C1, C5, C8, C9 (implemented, not amended; amendment rule at :285). Governance: Prime Directive / `STRATEGY.md` leapfrog-zone amendment (companion change).
- Flutter reference: `.flutter/.../scheduler/binding.dart:160-199,453-459,1253-1365`; `rendering/binding.dart:329-356,557-560,691-702`; `rendering/view.dart:347-362`; `widgets/binding.dart:473-477,1569-1578`; `widgets/framework.dart:2922-2945`; `gestures/binding.dart:295-353`; `foundation/isolates.dart:75-82`.
- Prior art: SwiftUI `Scene`/`WindowGroup` (scene as lifecycle container, not necessarily a window); React 18 concurrent rendering (commit-after-preparation); Chromium compositor-thread architecture (main tree vs compositor snapshot synchronized by commit).
- Audit evidence (2026-07-11): `flui-platform/src/executor.rs:181,215,227-248`; `platforms/windows/platform.rs:810-812`; `winit/platform.rs:782-788`; `flui-app/src/app/binding.rs:71-138,226-232`; `flui-foundation/src/binding.rs:106,188`; `flui-view/src/key/registry.rs:130`; `flui-rendering/src/pipeline/handle.rs:104-265`; `flui-layer/src/scene.rs:59,104-124`; `flui-engine/src/wgpu/renderer.rs:176-259,1048-1252`; `flui-app/src/app/runner.rs:214-312`.
