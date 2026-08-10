# ADR-0013: Render objects that drive their own pipeline work attach via a tree-lifecycle hook that hands them a self-dirty handle — ONE mechanism serves owned-animation and external-notifier objects alike

*A render object that must mark **itself** dirty out-of-band (a `RenderAnimatedSize` driving its own animation, a `RenderFlow`/`RenderCustomPaint` reacting to a delegate's repaint `Listenable`) receives an attachment-interval-scoped, least-privilege `RenderInvalidationHandle` via the defaulted `attach`/`detach` lifecycle pair on `RenderBox`/`RenderSliver`. It then subscribes to a `dyn Listenable` in `attach` and unsubscribes in `detach`. There is **no** public raw owner/channel capability: `RenderInvalidationHandle` is the sole public invalidation capability, and the owner privately stamps every request with the node's attachment epoch before `drain_pending_dirty` accepts it. `flui-rendering` takes **no** new crate dependency.*

---

- **Status:** Accepted (chief-architect ARCH-GATE: ACCEPTABLE; infra decision only — `RenderAnimatedSize` itself is a separate DEV task and must be DoD-cross-checked against `.flutter/flutter-master/packages/flutter/lib/src/rendering/animated_size.dart` + `object.dart` `attach`/`detach`)
- **Date:** 2026-07-01
- **Deciders:** chief-architect; consult api-design-lead (the two new trait methods + the additive `RenderInvalidationHandle::mark_needs_layout` / rename question), async-systems/scheduler owner (confirming the tick→mark path stays sync and buffered), qa-lead (attach/detach lifecycle + re-attach harness tests)
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

- **Marking *one specific attached node* dirty out-of-band is solved in `flui-rendering`.** `RenderInvalidationHandle` is a `Clone`, least-privilege capability bound to one `RenderId` and one private attachment epoch. Its four public verbs enqueue layout, compositing-bits, paint, or semantics work. The owner drains a request only when both the stable id and the currently attached epoch match; requests sent before detach, while detached, or through an old handle after same-id reattach are inert. Depth is always re-read from the live node.

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
fn attach(&mut self, handle: RenderInvalidationHandle) { let _ = handle; }
fn detach(&mut self) {}
```

- **`attach`** is called by the pipeline immediately after a node's `RenderId` is assigned and its `NodeLinks` are wired. Fresh insertion privately selects the first attachment epoch and constructs the capability from the owner's private dirty sender; `render_invalidation_handle(id)` becomes available only after that interval is attached. The owner then calls `RenderObject::attach(handle)` alongside the canonical initial dirty marks.
- **`detach`** is called for **every** node in a removed or relocated subtree. Relocation preserves the nodes and stable ids but closes each attachment interval, drops the root edge, and evicts all live/mid-phase dirty queues plus layout-poison state. `PipelineOwner<Idle>::detach_render_subtrees` returns one opaque, non-cloneable `DetachedRenderSubtrees` token for a transparent element's disjoint render frontiers. The token must be consumed exactly once: `attach_render_subtrees` opens fresh epochs after destination edges and parent data are established, while `release_detached_render_subtrees_for_finalization` authorizes the ordinary deepest-first element-unmount path without deleting nodes or firing callbacks itself. Both terminal operations preflight ownership, attachment epochs, and topology without mutation and return the token on failure.
- **Non-goal:** `attach` does **not** re-run per frame and is **not** a hot path (insert/remove are structural, between-phase mutations). It therefore does not touch the sync layout/paint/hit-test port-check triggers.

### D2 — How a render object reaches a ticker/Vsync: **it does not, from `flui-rendering`.** The `AnimationController` is injected at construction from the view layer; `attach` hands over only the self-dirty handle.

This is the decisive layering call. `attach` carries **only** a `RenderInvalidationHandle` — never a `Scheduler`, `TickerProvider`, or `Vsync`. The animation itself is constructed by the owning `AnimatedSize` **View/State** (which legitimately reaches a `Vsync`/`Scheduler` in the view layer, exactly where every other animated widget does today) and passed into the render object's **constructor**, mirroring Flutter's `vsync:`-into-the-`RenderObject`-constructor shape. The render object holds an `AnimationController` (an opaque `Listenable` + value source, from `flui-animation`) and treats it purely as a `Listenable`; it never sees a ticker.

Consequences of D2:
- `flui-rendering` stays free of `flui-scheduler`/`flui-animation` and free of any `flui-view`/widget-tree knowledge. Layering stays strictly one-directional.
- The **only** flui-rendering-side capability the render object gains is "mark *my* node dirty," delivered as the least-privilege `RenderInvalidationHandle` — it cannot mark other nodes dirty. `RenderInvalidationHandle` gains one additive method, mirroring `mark_needs_paint`:

  ```rust
  pub fn mark_needs_layout(&self) -> Result<(), SendError> {
      self.sender.request_mark_dirty(self.id, self.attachment_epoch, DirtyKind::Layout)
  }
  ```

- The dependency `flui-objects → flui-animation` (so `RenderAnimatedSize` can hold an `AnimationController`) is acyclic and clean, but it is a **consequence for the `RenderAnimatedSize` DEV task**, not for this infra ADR. This ADR adds nothing to any manifest except `flui-rendering`'s own `handle.rs`/trait.

### D3 — A render-object-driven tick triggers a NEW layout pass through the **existing** dirty channel, re-entrancy-safe

The flow is entirely pre-existing plumbing, wired end-to-end for the first time:

```
controller ticks (Scheduler transient-drain, or Vsync::tick_all)
  → controller notifies its Listener (added in attach)
  → listener calls handle.mark_needs_layout()
  → RenderInvalidationHandle::mark_needs_layout() stamps the private attachment epoch [buffered, wakes platform]
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
- **Render-subtree relocation is a separate owner capability.** It is represented by the linear `DetachedRenderSubtrees` token, not by extending `RenderInvalidationHandle` with tree mutation authority.

---

## Consequences

**Positive**
- Closes the single missing seam with **two defaulted trait methods + one additive handle method**, no new crate dependency, no new sanctioned `dyn`, no lock in public API, no async hot-path, and **zero** change to the `DirtyTracker`/dirty-channel machinery (it is *reused*, not extended).
- One idiom for every "render object drives its own pipeline work" case — owned animation *and* external notifier — instead of two parallel designs. The `RenderFlow`/`RenderCustomPaint` deferrals close for free the moment the seam lands.
- Layering is strictly preserved: the ticking stays in `flui-animation`, the self-mark stays in `flui-rendering`, and `attach` is the thin, least-privilege bridge. Depending on a `dyn Listenable` (already allowlisted) keeps the seam trait-based (DIP at the boundary).
- Attachment epochs make lifecycle safe by construction: old handles and already-queued requests cannot cross a detach/reattach boundary, even when the same stable `RenderId` survives relocation.

**Negative / Trade-offs**
- Two new methods on the widely-implemented `RenderBox`/`RenderSliver` traits. Mitigated: both default to no-op (ISP intact; non-animated objects pay nothing), and this matches the established seven-forwarded-method pattern — api-design-lead sign-off is light.
- A running controller keeps ticking (and self-marking layout) every frame until it settles or the widget's State disposes it — the eager cost Flutter also pays. `Vsync::has_running()` already lets the frame driver quiesce once all controllers settle, so this does not cause perpetual redraw.

**Follow-ups**
- `RenderAnimatedSize` DEV task (owning-animation exemplar of the seam).
- `RenderFlow`/`RenderCustomPaint` DEV tasks: replace their manual `set_*() -> bool` change-detection workarounds with `attach`-time `Listenable` subscriptions (external-notifier exemplars).

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **Two mechanisms** — a bespoke "render object owns + drives a ticker" subsystem for (a), separate from a "subscribe to a `Listenable`" subsystem for (b). | (a)'s ticker driving is already owned by `AnimationController` in `flui-animation`; a second driving subsystem in `flui-rendering` would duplicate it and force a `flui-rendering → flui-scheduler`/`flui-animation` dependency. Once (a) is expressed as "subscribe to the controller (a `Listenable`) and self-mark," (b) is literally the same code with `mark_needs_paint` — one design covers both. Strictly more infrastructure for a strictly worse boundary. |
| **`flui-rendering` depends on `flui-scheduler`; `attach` hands the object a `Scheduler`/`TickerProvider` so it can build its own controller (closest Flutter port).** | Acyclic but unnecessary: it drags ticker/scheduler concepts into the render layer for a capability the view layer already provides at construction. It also invites render objects to reach a *global* scheduler ambiently, which FLUI has deliberately avoided (`Vsync` is non-singleton, handed down explicitly). Injecting the `AnimationController` at construction keeps the render layer ignorant of tickers entirely. |
| **Store a back-pointer to `PipelineOwner` on every render node (Flutter's `attach(owner)` verbatim).** | Violates FLUI's single-owner-mutable model (nothing else holds `&mut PipelineOwner`) and would put a lock/owner handle on every node. The generational `RenderInvalidationHandle` already solves "self-mark from outside a frame" without a back-pointer; handing that (least-privilege) instead of the whole owner is the FLUI-native shape. |
| **A new dedicated per-frame "animation tick" callback list on the `PipelineOwner`, and objects register/unregister there.** | Re-invents `Scheduler`'s transient/persistent frame-callback lists (which already tick `AnimationController`s) inside the render layer, and re-invents the dirty-channel wake. The private sender behind `RenderInvalidationHandle` plus `drain_pending_dirty` already turns an out-of-band signal into next-frame layout, re-entrancy-safe. No new list needed. |
| **Extend `RenderInvalidationHandle` implicitly / add layout marking without a lifecycle hook** (e.g. hand the handle at construction). | Impossible: the `RenderId` does not exist until `insert`. The lifecycle hook is the irreducible core of the problem, not an optional convenience. |

---

## Ordered implementation plan

All changes are confined to `flui-rendering` (trait + handle + owner insert/remove); no manifest edits.

**Slice A — the self-dirty handle verb (independent, landable first):**
1. Add `RenderInvalidationHandle::mark_needs_layout(&self) -> Result<(), SendError>` (`pipeline/handle.rs`), delegating privately to an attachment-epoch-stamped layout request — mirror `mark_needs_paint` exactly. Unit test alongside the existing handle tests: a layout request round-trips through the private receiver as a layout request.

**Slice B — the lifecycle hook (depends on A for a useful `attach` payload):**
2. Add defaulted `attach(&mut self, RenderInvalidationHandle)` / `detach(&mut self)` to `RenderBox` and `RenderSliver`; forward them from the blanket `RenderObject<P>` impls (mirror `reassemble` wiring precisely). No change to the erased `perform_layout_raw`/`paint_raw`/`hit_test_raw` surface.
3. Call `RenderObject::attach` in `insert` / `insert_child_render_object` / `insert_render_node` (`owner/accessors.rs`) right after the id is assigned, privately minting the first attachment epoch and capability. Call `RenderObject::detach` for each subtree id in `remove_render_object` **before** `scheduler.evict`.
4. **★ MILESTONE — lifecycle harness proof.** In `render_object_harness.rs`, a probe render object records `attach`/`detach` invocations and captures its handle. Assert: inserting a node fires exactly one `attach` with a live handle whose `id()` matches; the captured handle's `mark_needs_layout()` marks *that* node (observable via the next frame re-laying it out); removing the node fires `detach`; and a `mark_needs_layout()` on the handle *after* removal is a silent `Ok`/no-op (generational staleness). Re-parent (remove+insert) fires `detach` then `attach` with a fresh handle. These are **red** before steps 2–3.

**Slice C — first real consumer (separate DEV task, out of this ADR's scope but named for sequencing):** `RenderAnimatedSize` in `flui-objects` (adds `flui-objects → flui-animation`), holding an injected `AnimationController`, subscribing in `attach` → `mark_needs_layout`, with retarget/clip logic DoD-checked against `animated_size.dart`.

---

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reuses the two capabilities that already exist in sibling crates — `AnimationController` owning-and-driving its `Ticker` while *being* a `Listenable` (`flui-animation`), and attachment-epoch-scoped `RenderInvalidationHandle` requests drained by `drain_pending_dirty` into a re-entrancy-safe next-frame mark (`flui-rendering`) — rather than inventing a parallel ticker or dirty channel inside the render layer. It adds **no** new crate dependency to `flui-rendering`, **no** new sanctioned `dyn` (`Listenable` is already allowlisted), **no** lock in public API, and **no** async on the layout/paint/hit-test hot path (the tick→mark is a buffered channel send, and `attach`/`detach` fire only on structural insert/remove, never mid-phase). Boundaries stay acyclic and one-directional: ticking in `flui-animation`, self-mark in `flui-rendering`, `attach` the least-privilege bridge; the render object never sees a scheduler/ticker (injected `AnimationController` at construction, Flutter-`vsync`-parity). One fact, one place: `RenderInvalidationHandle` is the sole public invalidation capability and the raw sender/request/kind transport remains private. Boundary-type check: `attach`/`detach` are defaulted forwarded methods matching the established `reassemble` pattern (ISP: no-op default costs non-animated objects nothing) rather than a new sealed sub-trait or typestate, which would over-encode a two-method no-op contract. Forward view (2 years / 3 extensions): the same seam serves `RenderAnimatedSize` (owned animation → layout), `RenderFlow`/`RenderCustomPaint` (external delegate/painter `Listenable` → paint), and a future `RenderAnimatedOpacity` self-optimization or render-level `ListenableBuilder` (external value-notifier → paint/layout) with no further infrastructure. The pinned Flutter oracle (`animated_size.dart`, `object.dart` `attach`/`detach`) remains the behavioral reference; this ADR's own milestone is the lifecycle harness proof in Slice B.
