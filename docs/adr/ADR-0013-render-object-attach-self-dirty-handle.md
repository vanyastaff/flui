# ADR-0013: Render objects that drive their own pipeline work attach via a tree-lifecycle hook that hands them a self-dirty handle — ONE mechanism serves owned-animation and external-notifier objects alike

*A render object that must mark **itself** dirty out-of-band (a `RenderAnimatedSize` driving its own animation, a `RenderFlow`/`RenderCustomPaint` reacting to a delegate's repaint `Listenable`) receives a generational, least-privilege self-dirty handle at tree-attach time via a new defaulted `attach`/`detach` lifecycle pair on `RenderBox`/`RenderSliver`. It then subscribes to a `dyn Listenable` in `attach` and unsubscribes in `detach`. There is **no** new "render object owns a ticker" infrastructure: the ticking is already owned by `flui-animation`'s `AnimationController` (which owns its `Ticker` and **is** a `Listenable`), and the out-of-band mark is already carried by the existing `PipelineOwnerHandle`/`drain_pending_dirty` channel. `flui-rendering` takes **no** new crate dependency.*

---

- **Status:** Accepted (chief-architect ARCH-GATE: ACCEPTABLE; infra decision only — `RenderAnimatedSize` itself is a separate DEV task and must be DoD-cross-checked against `.flutter/flutter-master/packages/flutter/lib/src/rendering/animated_size.dart` + `object.dart` `attach`/`detach`)
- **Date:** 2026-07-01
- **Deciders:** chief-architect; consult api-design-lead (the two new trait methods + the additive `RepaintHandle::mark_needs_layout` / rename question), async-systems/scheduler owner (confirming the tick→mark path stays sync and buffered), qa-lead (attach/detach lifecycle + re-attach harness tests)
- **Relates to:** unblocks the `AnimatedSize` widget epic (`RenderAnimatedSize`); retroactively closes the documented deferrals in `RenderFlow` (`flui-objects/src/layout/flow.rs` module doc — `FlowDelegate`'s `Listenable? repaint`) and `RenderCustomPaint` (`flui-objects/src/proxy/custom_paint.rs` module doc — `CustomPainter.addListener`/`removeListener` driving `markNeedsPaint`). Sibling in spirit to ADR-0011/0012: close a gap by **reusing existing machinery** rather than inventing a parallel channel.
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs.

---

## Context

### What `RenderAnimatedSize` needs (oracle: `rendering/animated_size.dart`)

`RenderAnimatedSize` is structurally unlike every render object FLUI ships today: it **owns and drives its own animation**, decoupled from any widget rebuild. In Flutter it holds `_controller: AnimationController`, `_animation: CurvedAnimation`, `_sizeTween: SizeTween`; a `vsync: TickerProvider` is passed **once at construction**; `attach(PipelineOwner)`/`detach()` create/dispose the controller's ticker connection; the controller's `addListener` calls `markNeedsLayout()` on every tick, so the render object drives its **own** repeated layout passes over time; during `performLayout` it lays out the child, compares the child's measured size to `_sizeTween.end`, and on a mismatch retargets (`begin = size` — the *current interpolated* size — then `forward(from: 0.0)`); the reported size is `constraints.constrain(_sizeTween.evaluate(_animation))`, clipping overflow.

### The four verified gaps in FLUI

1. No `AnimatedSize` widget/render object exists in any form.
2. `RenderObject<P>` (`crates/flui-rendering/src/traits/render_object.rs`) has **no** attach/detach tree-lifecycle hook. It carries seven *defaulted* forwarded methods today (`reassemble` at `:529` — note `&mut self` — `paint_alpha`, `paint_transform`, …), but nothing fires when a node enters/leaves the tree.
3. No render object holds a back-reference to its owner or receives per-frame ticks directly. Every "animated" widget today (`AnimatedOpacity`, `FadeTransition`, …) works by having a **`State`** (view layer) register a controller with a `Vsync` and push a freshly-computed value into a *plain* render object on rebuild. That is architecturally insufficient here: the render object itself — not a rebuild — must detect the size change and decide when to animate.
4. Crate graph is clean for this: `flui-rendering`'s `Cargo.toml` lists `flui-animation`/`flui-scheduler` only as `[dev-dependencies]`; `flui-animation` does **not** depend back on `flui-rendering`. A production edge `flui-rendering → flui-scheduler` would be acyclic — but this ADR shows we do not need one.

### The reframe: 90% of this already exists — in two other crates

The naive read is "we must build a way for a render object to own-and-drive a ticker and re-trigger layout." That capability is **already built**, split across two crates that this ADR deliberately does **not** pull into `flui-rendering`:

- **Owning + driving a ticker is solved in `flui-animation`.** `AnimationController` (`crates/flui-animation/src/controller.rs`) is constructed with an `Arc<Scheduler>`, **owns its own `Ticker`** (`Ticker::new_with_scheduler`), drives itself every frame off the scheduler's transient-callback drain (or, for headless determinism, off `Vsync::tick_all` with *virtual* time — `crates/flui-animation/src/vsync.rs`), and **implements `Listenable`** (`add_listener(ListenerCallback) -> ListenerId` / `remove_listener`, from `flui-foundation/src/notifier.rs`). `SizeTween`, `Tween`, and `CurvedAnimation` already exist (`flui-animation/src/tween_types.rs`, `curved.rs`). So "the render object drives its own layout over time, decoupled from rebuild" needs **no new ticking machinery** — it needs the controller (a `Listenable`) to be able to mark **its** node dirty on notify.

- **Marking *one specific node* dirty out-of-band is solved in `flui-rendering`.** `RepaintHandle` (`crates/flui-rendering/src/pipeline/handle.rs:213`) is a `Clone`, **generational**, least-privilege wrapper over `PipelineOwnerHandle` bound to one `RenderId + depth`; a stale handle (node removed → generation bumped) is a silent no-op, "never a repaint of an unrelated reused slot." It currently exposes only `mark_needs_paint()` (`:243`) → `request_mark_dirty(id, depth, DirtyKind::Paint)`. But the underlying `PipelineOwnerHandle::request_mark_dirty` (`:171`) already accepts **all four** `DirtyKind`s including `Layout`, fires the visual-update wake on enqueue, and is drained at the top of every frame by `drain_pending_dirty` (`owner/accessors.rs:107`), which **replays through the same mark paths local callers use, re-reading the live node's authoritative depth and dropping stale ids silently.** So "a tick triggers a new layout pass next frame" is an **existing, tested, re-entrancy-safe path** — `RepaintHandle` simply doesn't expose the layout verb yet.

- **`dyn Listenable` is already a sanctioned `dyn` boundary.** Port-check trigger #9's FR-036 allowlist (`scripts/port-check.sh`) already contains `Listenable` (and `Animation`, `FlowDelegate`, `CustomPainter`). No new sanctioned `dyn` boundary is introduced. (`flui-objects`, where the concrete objects live, is not even in the FR-036 enforcement scope.)

### What is genuinely missing

Exactly one seam: **a render object cannot receive its own self-dirty handle, because its `RenderId` is assigned by `insert` and there is no lifecycle hook where the owner can hand the node that handle (and where the node can later tear its subscription down).** That is the whole decision.

---

## Decision

**We add one mechanism: a defaulted `attach`/`detach` tree-lifecycle pair, called by the pipeline's insert/remove paths, that hands a render object a generational, least-privilege self-dirty handle. Both `RenderAnimatedSize` and the `RenderFlow`/`RenderCustomPaint` deferrals are then expressed as "subscribe to a `dyn Listenable` in `attach`, mark self dirty on notify, unsubscribe in `detach`."** `flui-rendering` gains **no** new crate dependency, **no** new sanctioned `dyn`, **no** lock in public API, and **no** async on a hot path.

### D1 — `attach`/`detach` lifecycle pair (mirrors the existing forwarded-defaulted-method pattern)

Add two defaulted methods on `RenderBox` and `RenderSliver` (the traits users implement), forwarded from the blanket `RenderObject<P>` impls exactly like the existing seven (`reassemble` is the closest precedent — same `&mut self`, same "most objects want the no-op default" shape):

```rust
// Default no-op: a non-animated object pays nothing (ISP preserved).
fn attach(&mut self, handle: RepaintHandle) { let _ = handle; }
fn detach(&mut self) {}
```

- **`attach`** is called by the pipeline immediately after a node's `RenderId` is assigned and its `NodeLinks` are wired — i.e. inside `PipelineOwner::insert` / `insert_child_render_object` / `insert_render_node` (`owner/accessors.rs`), after the id exists and alongside the initial `mark_needs_layout`/`mark_needs_paint` those methods already issue. The owner mints the handle with the **existing** `PipelineOwner::repaint_handle(id)` constructor and calls `RenderObject::attach(handle)` on the freshly-inserted entry.
- **`detach`** is called by `remove_render_object` (`owner/accessors.rs:248`, "THE dispose site") for **every** id in the collected subtree, **before** `scheduler.evict` and `remove_recursive`. Re-parent = remove + insert = `detach` then `attach` with a **fresh** handle carrying the new depth (and depth staleness is corrected at drain time anyway — `drain_pending_dirty` re-reads the live node's depth).
- **Non-goal:** `attach` does **not** re-run per frame and is **not** a hot path (insert/remove are structural, between-phase mutations). It therefore does not touch the sync layout/paint/hit-test port-check triggers.

### D2 — How a render object reaches a ticker/Vsync: **it does not, from `flui-rendering`.** The `AnimationController` is injected at construction from the view layer; `attach` hands over only the self-dirty handle.

This is the decisive layering call. `attach` carries **only** a `RepaintHandle` — never a `Scheduler`, `TickerProvider`, or `Vsync`. The animation itself is constructed by the owning `AnimatedSize` **View/State** (which legitimately reaches a `Vsync`/`Scheduler` in the view layer, exactly where every other animated widget does today) and passed into the render object's **constructor**, mirroring Flutter's `vsync:`-into-the-`RenderObject`-constructor shape. The render object holds an `AnimationController` (an opaque `Listenable` + value source, from `flui-animation`) and treats it purely as a `Listenable`; it never sees a ticker.

Consequences of D2:
- `flui-rendering` stays free of `flui-scheduler`/`flui-animation` and free of any `flui-view`/widget-tree knowledge. Layering stays strictly one-directional.
- The **only** flui-rendering-side capability the render object gains is "mark *my* node dirty," delivered as the least-privilege `RepaintHandle` — it cannot mark other nodes dirty. `RepaintHandle` gains one additive method, mirroring `mark_needs_paint`:

  ```rust
  pub fn mark_needs_layout(&self) -> Result<(), SendError> {
      self.handle.request_mark_dirty(self.id, self.depth, DirtyKind::Layout)
  }
  ```

  The type's name (`RepaintHandle`) becomes slightly narrow once it also marks layout; because its constructor is `pub(super)` and it is only now being vended to render objects, a rename to a neutral `NodeDirtyHandle` is low-risk. **Recommendation:** add the method now; defer the rename to api-design-lead at the DEV-GATE (naming convention not yet implied by an existing consumer — the one strategic fork here).
- The dependency `flui-objects → flui-animation` (so `RenderAnimatedSize` can hold an `AnimationController`) is acyclic and clean, but it is a **consequence for the `RenderAnimatedSize` DEV task**, not for this infra ADR. This ADR adds nothing to any manifest except `flui-rendering`'s own `handle.rs`/trait.

### D3 — A render-object-driven tick triggers a NEW layout pass through the **existing** dirty channel, re-entrancy-safe

The flow is entirely pre-existing plumbing, wired end-to-end for the first time:

```
controller ticks (Scheduler transient-drain, or Vsync::tick_all)
  → controller notifies its Listener (added in attach)
  → listener calls handle.mark_needs_layout()
  → PipelineOwnerHandle::request_mark_dirty(id, depth, DirtyKind::Layout)   [buffered, wakes platform]
  → next run_frame: drain_pending_dirty() replays → mark_needs_layout(id)    [boundary walk enqueues]
  → run_layout re-lays-out the subtree → RenderAnimatedSize::perform_layout reads controller.value()
```

Re-entrancy is structurally excluded: the tick fires during the scheduler's transient-callback drain (`handle_begin_frame`, which the app runner drives **before** `run_frame`), and the mark is **buffered onto a bounded channel**, not pushed synchronously into the `DirtyTracker`. Even a tick that somehow fired mid-layout routes to the channel (drained at the *next* frame's top), never into the mid-phase side-queue directly — so the "tick firing mid-layout" hazard the brief flags cannot corrupt an in-flight walk. Backpressure is already surfaced (`SendError::ChannelFull`). No new dirty-tracking infrastructure; the `DirtyTracker` (`pipeline/scheduler.rs`) is untouched.

### D4 — Scope: **ONE mechanism.** The external-notifier case (b) is a strict subset of the owned-animation case (a).

- **(a) owned + self-driven** (`RenderAnimatedSize`): the render object *owns* an `AnimationController`, subscribes to it in `attach` with `move || handle.mark_needs_layout()`, and unsubscribes in `detach`. The retarget-mid-flight logic (`begin = current interpolated size`, `forward(from: 0)`, clip overflow) lives in the object's `perform_layout` — object-specific, not infra.
- **(b) subscribe to an externally-owned notifier** (`RenderFlow`/`RenderCustomPaint`, and a hypothetical `RenderAnimatedOpacity` self-optimization): the render object holds a `dyn Listenable` it does **not** own (the `FlowDelegate`/`CustomPainter` repaint `Listenable`), subscribes in `attach` with `move || handle.mark_needs_paint()`, unsubscribes in `detach`.

Both reduce to the identical shape — *hold a `Listenable`, `add_listener` in `attach`, self-mark on notify, `remove_listener` in `detach`* — differing only in (i) who owns the `Listenable` and (ii) whether the self-mark is `mark_needs_layout` (a) or `mark_needs_paint` (b). **(b) uses a strict subset of (a)'s plumbing.** Crucially, "owning and *driving* a ticker" is **not** extra flui-rendering infrastructure for (a): the driving is fully absorbed by `flui-animation`'s `AnimationController` (which owns its `Ticker` and is itself the `Listenable`). So the two cases do **not** need separate designs; the single `attach`/`detach` + self-dirty-handle seam serves both. Building it correctly for (a) closes (b) for free.

### D5 — Explicitly OUT of scope

- **`RenderAnimatedSize` itself** — its `AnimationController`/`CurvedAnimation`/`SizeTween` construction, the retarget-from-current-size logic, `constraints.constrain(evaluate(...))`, and hard-edge clip-on-overflow. Separate DEV task; DoD-cross-check against `animated_size.dart`.
- **The `AnimatedSize` widget/View/State** — creating the controller, reaching the `Vsync`/`Scheduler`, passing it into the render object. View-layer work.
- **Ticker/animation machinery** — `Ticker` lifecycle (`flui-scheduler`), `AnimationController`, `Vsync` virtual-time driving, `Tween`/`SizeTween`/`CurvedAnimation`. All exist; untouched.
- **The frame pump and vsync-to-display-refresh** — the app runner + `Scheduler::handle_begin_frame`/`handle_draw_frame` already own this (`flui-app`). Untouched.
- **Semantics-tree ticking**, and any `RenderObject::dispose` epic beyond `detach`'s subscription teardown (controller *disposal* is the owning State's responsibility, as in Flutter; `detach` only stops the self-mark subscription).
- **Renaming `RepaintHandle`** — recommended follow-up, ratified by api-design-lead; not blocking.

---

## Consequences

**Positive**
- Closes the single missing seam with **two defaulted trait methods + one additive handle method**, no new crate dependency, no new sanctioned `dyn`, no lock in public API, no async hot-path, and **zero** change to the `DirtyTracker`/dirty-channel machinery (it is *reused*, not extended).
- One idiom for every "render object drives its own pipeline work" case — owned animation *and* external notifier — instead of two parallel designs. The `RenderFlow`/`RenderCustomPaint` deferrals close for free the moment the seam lands.
- Layering is strictly preserved: the ticking stays in `flui-animation`, the self-mark stays in `flui-rendering`, and `attach` is the thin, least-privilege bridge. Depending on a `dyn Listenable` (already allowlisted) keeps the seam trait-based (DIP at the boundary).
- Generational handle makes lifecycle safe by construction: a controller that ticks after its node was removed self-marks into the void (stale id dropped at drain) — `detach` is a clean stop, not a correctness prerequisite.

**Negative / Trade-offs**
- Two new methods on the widely-implemented `RenderBox`/`RenderSliver` traits. Mitigated: both default to no-op (ISP intact; non-animated objects pay nothing), and this matches the established seven-forwarded-method pattern — api-design-lead sign-off is light.
- A running controller keeps ticking (and self-marking layout) every frame until it settles or the widget's State disposes it — the eager cost Flutter also pays. `Vsync::has_running()` already lets the frame driver quiesce once all controllers settle, so this does not cause perpetual redraw.
- `RepaintHandle`'s name is now marginally misleading (marks layout too). Accepted as a deferred rename rather than churning a public type mid-ADR.

**Follow-ups**
- `RenderAnimatedSize` DEV task (owning-animation exemplar of the seam).
- `RenderFlow`/`RenderCustomPaint` DEV tasks: replace their manual `set_*() -> bool` change-detection workarounds with `attach`-time `Listenable` subscriptions (external-notifier exemplars).
- api-design-lead: ratify `RepaintHandle::mark_needs_layout` and the optional rename to `NodeDirtyHandle`.

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **Two mechanisms** — a bespoke "render object owns + drives a ticker" subsystem for (a), separate from a "subscribe to a `Listenable`" subsystem for (b). | (a)'s ticker driving is already owned by `AnimationController` in `flui-animation`; a second driving subsystem in `flui-rendering` would duplicate it and force a `flui-rendering → flui-scheduler`/`flui-animation` dependency. Once (a) is expressed as "subscribe to the controller (a `Listenable`) and self-mark," (b) is literally the same code with `mark_needs_paint` — one design covers both. Strictly more infrastructure for a strictly worse boundary. |
| **`flui-rendering` depends on `flui-scheduler`; `attach` hands the object a `Scheduler`/`TickerProvider` so it can build its own controller (closest Flutter port).** | Acyclic but unnecessary: it drags ticker/scheduler concepts into the render layer for a capability the view layer already provides at construction. It also invites render objects to reach a *global* scheduler ambiently, which FLUI has deliberately avoided (`Vsync` is non-singleton, handed down explicitly). Injecting the `AnimationController` at construction keeps the render layer ignorant of tickers entirely. |
| **Store a back-pointer to `PipelineOwner` on every render node (Flutter's `attach(owner)` verbatim).** | Violates FLUI's single-owner-mutable model (nothing else holds `&mut PipelineOwner`) and would put a lock/owner handle on every node. The generational `RepaintHandle` already solves "self-mark from outside a frame" without a back-pointer; handing that (least-privilege) instead of the whole owner is the FLUI-native shape. |
| **A new dedicated per-frame "animation tick" callback list on the `PipelineOwner`, and objects register/unregister there.** | Re-invents `Scheduler`'s transient/persistent frame-callback lists (which already tick `AnimationController`s) inside the render layer, and re-invents the dirty-channel wake. The existing `PipelineOwnerHandle` → `drain_pending_dirty` path already turns an out-of-band signal into next-frame layout, re-entrancy-safe. No new list needed. |
| **Extend `RepaintHandle` implicitly / add layout marking without a lifecycle hook** (e.g. hand the handle at construction). | Impossible: the `RenderId` does not exist until `insert`. The lifecycle hook is the irreducible core of the problem, not an optional convenience. |

---

## Ordered implementation plan

All changes are confined to `flui-rendering` (trait + handle + owner insert/remove); no manifest edits.

**Slice A — the self-dirty handle verb (independent, landable first):**
1. Add `RepaintHandle::mark_needs_layout(&self) -> Result<(), SendError>` (`pipeline/handle.rs:222`), delegating to `request_mark_dirty(.., DirtyKind::Layout)` — mirror `mark_needs_paint` exactly. Unit test alongside the existing `handle_*` tests: a layout request round-trips through the receiver as `DirtyKind::Layout`.

**Slice B — the lifecycle hook (depends on A for a useful `attach` payload):**
2. Add defaulted `attach(&mut self, RepaintHandle)` / `detach(&mut self)` to `RenderBox` and `RenderSliver`; forward them from the blanket `RenderObject<P>` impls (mirror `reassemble` wiring precisely). No change to the erased `perform_layout_raw`/`paint_raw`/`hit_test_raw` surface.
3. Call `RenderObject::attach` in `insert` / `insert_child_render_object` / `insert_render_node` (`owner/accessors.rs`) right after the id is assigned, minting the handle via the existing `PipelineOwner::repaint_handle(id)`. Call `RenderObject::detach` for each subtree id in `remove_render_object` (`:248`) **before** `scheduler.evict`.
4. **★ MILESTONE — lifecycle harness proof.** In `render_object_harness.rs`, a probe render object records `attach`/`detach` invocations and captures its handle. Assert: inserting a node fires exactly one `attach` with a live handle whose `id()` matches; the captured handle's `mark_needs_layout()` marks *that* node (observable via the next frame re-laying it out); removing the node fires `detach`; and a `mark_needs_layout()` on the handle *after* removal is a silent `Ok`/no-op (generational staleness). Re-parent (remove+insert) fires `detach` then `attach` with a fresh handle. These are **red** before steps 2–3.

**Slice C — first real consumer (separate DEV task, out of this ADR's scope but named for sequencing):** `RenderAnimatedSize` in `flui-objects` (adds `flui-objects → flui-animation`), holding an injected `AnimationController`, subscribing in `attach` → `mark_needs_layout`, with retarget/clip logic DoD-checked against `animated_size.dart`.

---

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reuses the two capabilities that already exist in sibling crates — `AnimationController` owning-and-driving its `Ticker` while *being* a `Listenable` (`flui-animation`), and `RepaintHandle`/`PipelineOwnerHandle`/`drain_pending_dirty` turning an out-of-band signal into a re-entrancy-safe next-frame mark (`flui-rendering`) — rather than inventing a parallel ticker or dirty channel inside the render layer. It adds **no** new crate dependency to `flui-rendering`, **no** new sanctioned `dyn` (`Listenable` is already allowlisted), **no** lock in public API, and **no** async on the layout/paint/hit-test hot path (the tick→mark is a buffered channel send, and `attach`/`detach` fire only on structural insert/remove, never mid-phase). Boundaries stay acyclic and one-directional: ticking in `flui-animation`, self-mark in `flui-rendering`, `attach` the least-privilege bridge; the render object never sees a scheduler/ticker (injected `AnimationController` at construction, Flutter-`vsync`-parity). One fact, one place: the self-dirty verb is the existing `request_mark_dirty`, exposed once via `RepaintHandle`; no duplicate notifier. Boundary-type check: `attach`/`detach` are defaulted forwarded methods matching the established `reassemble` pattern (ISP: no-op default costs non-animated objects nothing) rather than a new sealed sub-trait or typestate, which would over-encode a two-method no-op contract. Forward view (2 years / 3 extensions): the same seam serves `RenderAnimatedSize` (owned animation → layout), `RenderFlow`/`RenderCustomPaint` (external delegate/painter `Listenable` → paint), and a future `RenderAnimatedOpacity` self-optimization or render-level `ListenableBuilder` (external value-notifier → paint/layout) with no further infrastructure. The `.flutter/flutter-master/` oracle (`animated_size.dart`, `object.dart` `attach`/`detach`) is available for the Slice-C DoD cross-check; this ADR's own milestone is the lifecycle harness proof in Slice B.
