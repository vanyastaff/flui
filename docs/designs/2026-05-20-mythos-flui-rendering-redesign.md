---
title: "Mythos design verdict — flui-rendering redesign"
status: design
date: 2026-05-20
author: Claude Mythos
applies-to: crates/flui-rendering
---

# Mythos Design Verdict

## What `flui-rendering` should be

A **single-owner render-tree storage and synchronous frame-pipeline driver** with a tight public surface: insert a render object, mark dirty, run a frame, get back a layer tree. Nothing else.

## What it must not become

A god-shaped library that owns its own dispatch story, its own observability story, its own plugin contract, its own concurrency posture, and its own debugger UI. That is the shape the Dart original has because Dart classes ARE the shape; in Rust, every responsibility above belongs in a different crate or doesn't exist.

## Main state owner

`PipelineOwner` owns the render tree and dirty-state vectors. There is exactly one mutable owner at any time. No `Arc<RwLock<PipelineOwner>>`. No "child pipelines". The hierarchical-pipeline pattern Flutter uses is a Dart-flavoured workaround for not having Rust's borrow checker; we have the borrow checker — we don't need the workaround.

## Main trust boundary

The trait `RenderObject<P>` is the plugin boundary. Anything implementing it is third-party code. The crate must assume nothing about its behaviour beyond what the trait contract states. Today the trait says `Send + Sync` — which costs every implementor and buys nothing because the pipeline is single-threaded. Drop it.

## Main async risk

Almost zero. The pipeline is synchronous by strategy. Async lives in `flui-scheduler`, `flui-assets`, `flui-build` — not here. Any `async fn` that creeps into this crate's hot path is a refusal-trigger violation (`docs/PORT.md` Trigger 3).

## Main simplification principle

**Every dyn, every Arc, every RwLock in this crate must defend its existence in writing.** Today, of the 35 public traits, at least 7 have ≤1 impl and exist as scaffolding rather than abstraction. Of the 29 `RwLock` sites, the U2 refactor removed the canonical hot-path violator; the rest are infrastructure carve-outs but should be audited as a class, not individually. The trait surface needs to halve. The state-module needs to break into 4 files keyed by concern. `PipelineOwner` needs to shed two of its three callback channels.

This is not architecture. It is fear wearing a generic parameter. Time to cut.

---

## 1. Problem Definition

**Responsibility.** Store and mutate a tree of render objects; drive the four-phase frame pipeline (layout → compositing-bits → paint → semantics); emit a `LayerTree` for downstream compositing.

**Non-responsibility.**
- Element-tree reconciliation (lives in `flui-view`).
- Widget identity, keys, build phase (lives in `flui-view`).
- Concrete layer compositing (lives in `flui-layer`).
- GPU surface management (lives in `flui-engine`).
- Hit-test target dispatch beyond the render tree (lives in `flui-interaction`).
- Async scheduling (lives in `flui-scheduler`).
- Hot-reload state preservation (lives in `flui-hot-reload`).
- Any persistent storage. Render state is in-memory only.

**Callers.** `flui-view` (mounts render objects into the tree as elements reconcile), `flui-app` (drives frame-tick), `flui-interaction` (hit-tests against the painted tree), `flui-engine` (renders the emitted `LayerTree`). Three callers, each well-defined.

**Lifecycle.** `PipelineOwner` is created at app startup, lives until shutdown. Render objects enter via `insert`, get repositioned via parent-child mutation, exit via `remove`. No detach-without-replace transient state.

**Key invariants.**
1. **Single-writer-per-frame.** During any phase, exactly one mutation path is active. Enforced by `&mut PipelineOwner` at phase boundaries.
2. **ID stability.** A `RenderId` issued once is valid until explicit removal. Slab-based storage enforces this.
3. **Phase ordering.** Layout → compositing-bits → paint → semantics. Phases are pure functions of dirty sets and constraints. Phases never run concurrently.
4. **No mutation across `.await`.** Render path is sync. There are no `.await` points to corrupt.
5. **Parent → child layout dispatch.** A parent's `perform_layout` calls `child.layout(constraints)`; the child writes its own size; the parent reads it. Re-entrant via `&mut RenderTree` disjoint borrows.
6. **Repaint-boundary invariant.** Paint changes below a repaint boundary do not require repainting ancestors. Tracked via `RenderFlags::IS_REPAINT_BOUNDARY` + `WAS_REPAINT_BOUNDARY`.

**Failure modes — normal, not exceptional.**
- Layout returns invalid geometry (NaN size, negative dimensions) — surface as `RenderError::InvalidGeometry`, do not panic.
- Parent gets `unconstrained` constraint where it expected bounded — `RenderError::UnboundedConstraint`.
- User-defined render object panics inside `perform_layout` — pipeline catches at frame boundary, marks tree as poisoned, surfaces error, drops frame.
- Render object marks itself dirty during paint — phase invariant violation, return `RenderError::PhaseViolation`.

---

## 2. Architecture Overview

```text
flui-view (element reconciler)
  │  insert / mark_dirty / remove
  ▼
PipelineOwner<Phase>         ◄── single mutable owner, phase-typed
  │  &mut RenderTree
  ▼
RenderTree (Slab<RenderNode>)
  │  RenderNode = enum { Box(Entry<Box>), Sliver(Entry<Sliver>) }
  │  Entry<P> = { Box<dyn RenderObject<P>>, RenderState<P>, NodeLinks }
  ▼
RenderObject<P>              ◄── plugin boundary; third-party code
  │
  ├─▶ (layout) → writes RenderState<P>::geometry
  ├─▶ (paint)  → writes LayerTree via PaintContext
  └─▶ (hit-test) → returns HitTestResult to flui-interaction

LayerTree (output) ─▶ flui-layer/flui-engine for compositing
```

No `Arc<RwLock<…>>` on the diagram. No hierarchical pipelines. No `Box<dyn Fn() + Send + Sync>` callbacks. No traits in the hot path that don't appear on the diagram.

What goes away from current code:
- `binding/` module — Flutter mixin-translation noise. Folded into a thin `RendererBinding` trait that lives in `flui-view` or `flui-app`, not here. The current 4-trait stack (`PipelineManifold`, `HitTestDispatcher`, `ViewHitTestable`, `RendererBinding`) collapses to one purpose-specific trait, or to a concrete adapter struct, depending on whether `flui-view` and `flui-app` are competing implementations (they are not).
- `children_access.rs` + `child_handle.rs` — the closure-based iterator was a workaround for fighting the borrow checker. Once `RenderTree` exposes `&mut self` properly at phase boundaries, plain `for child_id in node.children() { tree.layout_child(child_id, constraints) }` works. Both files: delete.
- `arity.rs` — 48 LOC of re-exports. Delete; let callers import from `flui_tree::Arity` directly.

---

## 3. Core Types

```rust
// IDs — NonZeroUsize-backed, +1/-1 offset pattern.
// Already correct in flui-foundation; not changing.
pub struct RenderId(NonZeroUsize);

// The pipeline owner — phase-typed via type-state.
// Replaces the current PipelineOwner with its three mutually-exclusive
// debug_doing_* bools and four parallel dirty Vecs.
pub struct PipelineOwner<Phase: PipelinePhase = Idle> {
    tree: RenderTree,
    root: Option<RenderId>,
    dirty: DirtySets,           // one struct, four Vecs co-located
    layer_tree: Option<LayerTree>,
    on_visual_update: VisualUpdateNotifier,  // ONE channel, not three Box<dyn Fn>
    _phase: PhantomData<Phase>,
}

pub trait PipelinePhase: sealed::Sealed {}
pub struct Idle;
pub struct Layout;
pub struct Compositing;
pub struct Paint;
pub struct Semantics;
impl PipelinePhase for Idle {}
impl PipelinePhase for Layout {}
impl PipelinePhase for Compositing {}
impl PipelinePhase for Paint {}
impl PipelinePhase for Semantics {}

// Phase transitions consume the owner — single-writer-per-frame enforced
// at compile time, not via runtime bool.
impl PipelineOwner<Idle> {
    pub fn into_layout(self) -> PipelineOwner<Layout> { /* ... */ }
}

impl PipelineOwner<Layout> {
    pub fn run_layout(&mut self) -> RenderResult<()> { /* ... */ }
    pub fn into_compositing(self) -> PipelineOwner<Compositing> { /* ... */ }
}

// And so on through Paint, Semantics, back to Idle.
// You CANNOT call paint() on a tree that has not had layout run.
// The type system enforces it.

// Co-located dirty sets — one cache line of pointers, not four heap
// allocations scattered across the struct.
pub struct DirtySets {
    pub needs_layout: Vec<DirtyNode>,        // sorted shallow-first
    pub needs_compositing: Vec<DirtyNode>,   // sorted shallow-first
    pub needs_paint: Vec<DirtyNode>,         // sorted deep-first
    pub needs_semantics: Vec<DirtyNode>,     // sorted shallow-first
}

// Render tree — Slab arena. No change from U2 baseline.
pub struct RenderTree {
    nodes: Slab<RenderNode>,
    owner_back: Option<Weak<PipelineOwnerHandle>>,  // not Weak<RwLock<...>>
}

// Wrap the owner for the back-reference. The wrapper is the only place
// that owns the &mut serialization. Children render objects hold a Weak
// pointer to it for "mark dirty" calls, not the owner directly.
pub struct PipelineOwnerHandle {
    // Send "I am dirty" requests; the owner consumes them at frame boundary.
    dirty_tx: crossbeam_channel::Sender<DirtyRequest>,
}

// Single-producer-single-consumer-ish: many render objects produce, one
// frame loop consumes. Bounded channel; backpressure surfaces as an error,
// not silent drop.

// State on a node — split by concern.
// Today's state.rs (1738 LOC) breaks into four files:
//   state/flags.rs       (atomic bitset, ~300 LOC after doc trim)
//   state/geometry.rs    (OnceCell<Geometry>, ~200 LOC)
//   state/constraints.rs (OnceCell<Constraints>, ~150 LOC)
//   state/offset.rs      (AtomicOffset, ~100 LOC)
// All wired through a thin facade RenderState<P> that exposes the
// composed API; each file owns its own invariants. The current
// 30+-method god-impl block evaporates.

pub struct RenderState<P: Protocol> {
    pub flags: AtomicRenderFlags,        // imported from state/flags
    pub geometry: GeometryCell<P>,       // imported from state/geometry
    pub constraints: ConstraintsCell<P>, // imported from state/constraints
    pub offset: AtomicOffset,            // imported from state/offset
}

// Node — unchanged from U2.
pub enum RenderNode {
    Box(RenderEntry<BoxProtocol>),
    Sliver(RenderEntry<SliverProtocol>),
}

pub struct RenderEntry<P: Protocol> {
    render_object: Box<dyn RenderObject<P>>,  // U2 baseline; no lock
    state: RenderState<P>,
    links: NodeLinks,
}

// Trait — slimmed.
// Current trait has 18 methods spanning paint, hit-test, semantics, parent-data,
// pipeline-integration. Many of those methods could move to extension traits
// keyed on capability (`HitTestCapability`, `SemanticsCapability`). Concrete
// proposal:
pub trait RenderObject<P: Protocol>: 'static {
    // Layout — &mut self because Flutter render objects mutate internal state
    // during layout (RenderFlex stores child positions on self).
    fn perform_layout(&mut self, ctx: &mut LayoutContext<P>) -> ProtocolGeometry<P>;

    // Paint — &self only. Mutation belongs to the framework (RenderState flags).
    fn paint(&self, ctx: &mut PaintContext<P>);

    // Hit-test — &self only.
    fn hit_test(&self, ctx: &mut HitTestContext<P>, position: Point) -> bool;

    // Optional capability hooks via default methods.
    fn paint_bounds(&self) -> Rect { Rect::ZERO }
    fn is_repaint_boundary(&self) -> bool { false }
    fn is_relayout_boundary(&self) -> bool { false }
    fn sized_by_parent(&self) -> bool { false }

    // Diagnostics for `RenderObject::Debug` parity with flui-foundation.
    fn debug_name(&self) -> &'static str { core::any::type_name::<Self>() }
}

// `Send + Sync` is dropped from the bound. If any future consumer needs to
// move a RenderObject across threads (e.g. for async asset loading on a
// `flui-assets` boundary), they wrap it themselves. The pipeline doesn't pay
// the tax.

// Errors — narrow, structured.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid geometry from {render_object}: {reason}")]
    InvalidGeometry { render_object: &'static str, reason: &'static str },
    #[error("unbounded constraint at {render_object}; parent must provide bounds")]
    UnboundedConstraint { render_object: &'static str },
    #[error("phase violation: cannot {operation} during {phase} phase")]
    PhaseViolation { operation: &'static str, phase: &'static str },
    #[error("render object {render_object} panicked during {phase}")]
    Poisoned { render_object: &'static str, phase: &'static str },
    #[error("dirty channel closed; pipeline owner dropped")]
    DirtyChannelClosed,
}
```

---

## 4. State Machine

Two state machines: one for the **pipeline phase**, one for the **render-object lifecycle**.

### Pipeline phase

```text
Idle
  │ into_layout()
  ▼
Layout ──run_layout()──┐
  │ into_compositing() │
  ▼                    │ on poison → Idle (with PoisonError)
Compositing ───────────┤
  │ into_paint()       │
  ▼                    │
Paint ─────────────────┤
  │ into_semantics()   │
  ▼                    │
Semantics              │
  │ finish()           │
  ▼                    │
Idle ←─────────────────┘
```

Every transition consumes `self` and returns the new-typed owner. **You cannot call `paint()` without having run `layout()` first** — the compiler enforces it. `flush_all` becomes a convenience method that composes the transitions:

```rust
impl PipelineOwner<Idle> {
    pub fn run_frame(self) -> Result<(PipelineOwner<Idle>, LayerTree), RenderError> {
        let owner = self.into_layout();
        owner.run_layout()?;
        let owner = owner.into_compositing();
        owner.run_compositing()?;
        let owner = owner.into_paint();
        owner.run_paint()?;
        let owner = owner.into_semantics();
        owner.run_semantics()?;
        Ok((owner.finish(), layer_tree))
    }
}
```

Today's runtime `debug_doing_layout: bool`/`debug_doing_paint: bool`/`debug_doing_semantics: bool` triple goes away. So does the question "what if two of those are true at once?" — it cannot happen.

### Render-object lifecycle

```text
Unattached
  │ insert_into_pipeline()
  ▼
Attached(needs_layout=true, needs_paint=true)
  │ layout phase
  ▼
LaidOut(needs_layout=false, needs_paint=true)
  │ paint phase
  ▼
Painted(needs_layout=false, needs_paint=false)
  │ mark_dirty() → back to Attached
  │ remove() → Unattached → Dropped
```

This stays as runtime state on `RenderState::flags` (atomic bitset). Reifying it as a Rust enum on each entry would bloat the entry; the bits are cheap, the atomics are cheap, and the validity envelope is small enough that "what happens if I paint an unlaid-out object" can be tested with debug asserts plus a state-machine table.

---

## 5. Public API

The crate's public surface — every other type is implementation detail and lives behind `pub(crate)` or further.

```rust
// Construction
PipelineOwner::<Idle>::new() -> PipelineOwner<Idle>

// Mutation (Idle only)
PipelineOwner<Idle>::insert<P>(node: ProtocolNode<P>) -> RenderId
PipelineOwner<Idle>::insert_child<P>(parent: RenderId, node: ProtocolNode<P>) -> Result<RenderId, RenderError>
PipelineOwner<Idle>::remove(id: RenderId) -> Result<(), RenderError>
PipelineOwner<Idle>::mark_needs_layout(id: RenderId)
PipelineOwner<Idle>::mark_needs_paint(id: RenderId)
PipelineOwner<Idle>::set_root(id: Option<RenderId>)

// Phase execution — typestate-driven, see Phase 4
PipelineOwner<Idle>::run_frame() -> Result<(PipelineOwner<Idle>, LayerTree), RenderError>

// Inspection (any phase)
PipelineOwner<_>::tree() -> &RenderTree
PipelineOwner<_>::root() -> Option<RenderId>
PipelineOwner<_>::dirty_count() -> usize

// Handle for cross-thread mark-dirty (when flui-view's element tree runs on
// the same thread but spawns work, or when async asset loaders complete and
// need to mark the receiving render object dirty)
PipelineOwner<_>::handle() -> PipelineOwnerHandle  // Send+Sync, Clone

// PipelineOwnerHandle API:
PipelineOwnerHandle::request_mark_dirty(id: RenderId, kind: DirtyKind)
  -> Result<(), RenderError::DirtyChannelClosed>
```

That is the entire public API. Eleven methods. Today's `PipelineOwner` exposes thirty-plus. The methods that go away:

| Today | Replacement |
|---|---|
| `flush_layout`, `flush_paint`, `flush_compositing_bits`, `flush_semantics`, `flush_all` | typestate transitions + `run_frame()` |
| `nodes_needing_layout`, `nodes_needing_paint`, `…compositing…`, `…semantics` (getters) | `dirty_count()` + phase-internal inspection |
| `add_node_needing_layout`, `add_node_needing_paint`, `…compositing…`, `…semantics` | `mark_needs_layout`/`mark_needs_paint` (typestate-gated) |
| `debug_doing_layout/paint/semantics/any_phase` | typestate parameter; reads on `PipelineOwner<Layout>` etc. |
| `adopt_child`, `drop_child`, `child_count`, `children` (hierarchical pipelines) | **removed** — see Rejected Designs |
| `set_on_need_visual_update`, `set_on_semantics_owner_created`, `set_on_semantics_owner_disposed` | one `VisualUpdateNotifier` (channel-based, single sink) |
| `with_callbacks` constructor variant | removed in favour of fluent setter on the one notifier |
| `clear_all_dirty_nodes`, `has_dirty_nodes`, `dirty_node_count` | `dirty_count()` only |

Every method is hot-path or cold-path-tagged in rustdoc. The hot path: `run_layout`, `run_paint`, `run_compositing`, internal `layout_node_with_children`, `paint_node_recursive`. The cold path: insertion, removal, mark-dirty, handle issuance.

---

## 6. Internal Modules

```text
crates/flui-rendering/src/
  lib.rs                — re-exports, prelude
  error.rs              — RenderError + RenderResult
  protocol/             — Box vs Sliver protocol seam
    mod.rs
    box_protocol.rs
    sliver_protocol.rs
  storage/              — Slab arena + RenderNode/RenderEntry
    mod.rs
    tree.rs             — RenderTree
    node.rs             — RenderNode enum
    entry.rs            — RenderEntry<P> (already U2-clean)
    links.rs            — NodeLinks (parent/children/depth)
  state/                — split from today's monolith
    mod.rs              — RenderState<P> facade
    flags.rs            — AtomicRenderFlags (kept; trim doc)
    geometry.rs         — GeometryCell<P>
    constraints.rs      — ConstraintsCell<P>
    offset.rs           — AtomicOffset
    propagation.rs      — RenderDirtyPropagation trait + impl (extracted)
  pipeline/             — phase-typed owner
    mod.rs
    owner.rs            — PipelineOwner<Phase> + transitions
    phase.rs            — sealed Phase types
    dirty.rs            — DirtyNode, DirtySets
    layout_phase.rs     — run_layout, layout_node_with_children
    paint_phase.rs      — run_paint, paint_node_recursive, layer emission
    compositing_phase.rs
    semantics_phase.rs
    handle.rs           — PipelineOwnerHandle + channel
  traits/               — RenderObject<P> + RenderBox<A> + RenderSliver
    mod.rs
    render_object.rs    — RenderObject<P> trait (slimmed)
    render_box.rs       — RenderBox<A: Arity> ergonomic adapter
    render_sliver.rs    — RenderSliver ergonomic adapter
  context/              — phase contexts callers consume
    mod.rs
    layout.rs           — LayoutContext<P>
    paint.rs            — PaintContext<P>
    hit_test.rs         — HitTestContext<P>
    canvas.rs           — CanvasContext (paint primitive surface)
  objects/              — concrete render objects (Padding, Flex, …)
  constraints/          — BoxConstraints, SliverConstraints (essentially unchanged)
  parent_data/          — ParentData trait + box/sliver/container variants
  hit_testing/          — HitTestEntry, HitTestResult, HitTestTarget
  input/                — MouseTracker (out-of-scope-for-render question; see "Open questions" below)
  view/                 — RenderView (root render object representing the window)
  delegates/            — CustomPainter, CustomClipper, FlowDelegate, etc.
```

**What earned its place vs. what didn't.**

Earned existence (justify in one sentence each):
- `storage/` — slab arena + node enum; without it, no tree.
- `state/` — atomic flags + write-once cells; without it, U2's lock-free refactor unravels.
- `pipeline/` — phase typestate + dirty bookkeeping + traversal; without it, no frame.
- `traits/` — `RenderObject<P>` is the third-party plugin boundary.
- `context/` — phase contexts let render objects call back into the pipeline without grabbing global state.
- `protocol/` — the Box vs Sliver split is real polymorphism (different `Constraints`/`Geometry` types).
- `objects/` — concrete render objects ship with the crate; eight files for eight objects, fine.
- `constraints/`, `parent_data/` — types every render object consumes.
- `hit_testing/` — hit-test primitives shared with `flui-interaction`.
- `view/` — `RenderView` is the root render object representing the OS window; without it, no root.
- `delegates/` — user-overridable paint/layout/clip delegates; six files for six legitimate extension points.

Did not earn its place — proposed deletions:
- `arity.rs` — 48 LOC re-exports. Delete; let `flui_tree::Arity` flow through unchanged.
- `child_handle.rs` + `children_access.rs` — 828 LOC for a closure-based iterator that exists to dodge the borrow checker. Once `RenderTree` exposes proper `&mut self` at phase boundaries (see Phase 7), `for child_id in node.children() { tree.layout_child(child_id, …) }` works without these abstractions. Delete both; inline what's needed.
- `binding/` — 4 traits, mixin transliteration. Move the one trait that's actually load-bearing (`RendererBinding`) to `flui-view` if `flui-view` is the only caller, or to a new `crates/flui-app-protocol` crate if `flui-app` also needs it. The other three traits (`PipelineManifold`, `HitTestDispatcher`, `ViewHitTestable`) get folded into concrete adapter structs.

**What `lib.rs` re-exports.** Today 188 lines of `pub use`. After the cut: `PipelineOwner`, `PipelineOwnerHandle`, `RenderId`, `RenderTree`, `RenderNode`, `RenderObject<P>`, `RenderBox`, `RenderSliver`, `BoxConstraints`/`SliverConstraints`/`Constraints`, `Offset`/`Size`/`Rect` re-exports from `flui-types`, the four context types, the error enum. Maybe 30 names total.

---

## 7. Async & Failure Semantics

**Task ownership.** Zero owned tasks. The crate runs synchronously inside the frame-tick driven by `flui-app`.

**Cancellation.** Not applicable. The frame either completes or returns an error; there is no in-progress state to cancel.

**Retry.** Not applicable at the pipeline level. A frame that fails (poisoned render object, etc.) is reported to the caller, which decides whether to drop the frame, retry next tick, or abort.

**Idempotency.** The pipeline is naturally idempotent — running `run_frame()` twice on a clean tree produces the same `LayerTree`. Mark-dirty operations are idempotent via the bitset.

**Backpressure.** The `PipelineOwnerHandle::request_mark_dirty` channel is **bounded**. If the consumer (frame loop) is too slow, the producer (background asset loader, etc.) gets a backpressure signal — `RenderError::DirtyChannelClosed` if the loop has died, or a blocking send if the channel is full. Default capacity: 256 pending dirty requests per pipeline. Tunable at construction.

**Shutdown.** `PipelineOwner` drop: drops the `RenderTree`, closes the dirty channel, drops the `VisualUpdateNotifier`. Any outstanding `PipelineOwnerHandle` receives `Err(DirtyChannelClosed)` on its next send. No hidden tasks to join.

**Partial failure recovery.** A render object that panics inside `perform_layout` or `paint` poisons that node only. The pipeline catches via `std::panic::catch_unwind`, marks the node as `RenderError::Poisoned`, drops the in-flight frame, and lets the caller decide. The render tree itself is not corrupted; the poisoned node's `state` is left in `needs_layout = true`; remediation is to remove the node or trust the next layout to recover.

**Two-phase commits.** Not needed; render state is in-memory. The closest analog is "constraints are written before geometry" — enforced by the layout phase's traversal order.

---

## 8. Security Model

`flui-rendering` is a library, not a service. It does not handle credentials, secrets, or network input. Its trust boundary is the `RenderObject<P>` plugin trait.

**Trusted inputs.**
- Constraints from parents (computed by other render objects inside the same trust boundary).
- IDs from the slab (issued by the trait itself).

**Untrusted inputs.**
- Concrete `impl RenderObject<P>` from third-party widgets. The render object can:
  - Panic — caught by `catch_unwind`.
  - Return invalid geometry — validated, surfaced as `InvalidGeometry`.
  - Spin in `perform_layout` — not detected. There is no per-call timeout in this crate. If `flui-app` wants a frame budget, it enforces it.
  - Allocate unbounded memory — not detected. Resource limits live higher up.
  - Recurse infinitely via parent → child → parent — detected indirectly by stack overflow; we should add a depth limit to the layout traversal (constant in `RenderTree`, e.g. 1024 levels).

**Capabilities.** None. The crate does not mediate authority. The render-object plugin runs with the privileges of the host process.

**Secret handling.** Not applicable. If a third-party render object embeds a secret in its config (`RenderText` displaying a password), that is the third party's bug; we provide nothing to mitigate it. Document this explicitly: "do not embed secrets in render object configuration; they will appear in `Debug` impls."

**Logging rules.** No render-object configuration is logged at info-level. Diagnostic dumps (`RenderTree::diagnose`) at debug-level only. `tracing` spans for phases use `RenderId`, not render-object debug output.

**Serialization.** `RenderTree` is not serializable in this crate. If `flui-devtools` wants to dump a tree for the inspector, that crate provides the serializer with explicit redaction.

**Plugin/user input rules.** `RenderObject` impls trust their own configuration but do not trust their parent's constraints (which could be NaN, infinite, etc.). The crate validates constraints at the boundary in debug builds.

---

## 9. Data-Oriented Notes

**Hot data.** Touched every frame for every node:
- `RenderState::flags` (4 bytes atomic) — read on every traversal, written on every mark-dirty.
- `RenderState::offset` (16 bytes, atomic-or-Cell) — written during layout, read during paint.
- `RenderState::geometry.size` (16 bytes for Box) — written during layout, read during paint.
- `NodeLinks::children` (24 bytes — `Vec` header).

Cold data:
- `RenderState::constraints` — written during layout, occasionally read during dirty propagation.
- `RenderObject` itself — vtable + config; accessed via `box_render_object()`.
- `parent_data` — written occasionally during reconciliation, read during layout context setup.

**Allocation strategy.**
- Slab arena (`storage/tree.rs`): O(1) insert/delete, dense reuse, ID stability. Already in place.
- `Box<dyn RenderObject<P>>`: one allocation per render object at insertion. Acceptable — long-lived. NOT allocated per frame.
- `Vec<DirtyNode>` in `DirtySets`: amortized; grows once at startup to typical tree size, never shrinks. Empty between frames.
- `LayerTree` (output): single allocation per paint phase, reused via `take_layer_tree`.

**Forbidden allocations.**
- No `Arc::clone` inside `paint_node_recursive` or `layout_node_with_children` (refusal trigger #5).
- No `HashMap<RenderId, _>` lookups in the paint hot path. Render objects find their state through the slab's direct index, not via a map.
- No `String::new` inside any hot path. Error formatting uses `&'static str` constants where possible.

**Cache locality.**
- `RenderEntry<P>` is ~120 bytes today. Splitting `RenderState` into four files does not change memory layout; `RenderState<P>` remains one struct, just composed from sub-types.
- `RenderNode` enum union is dominated by the larger of `RenderEntry<BoxProtocol>` and `RenderEntry<SliverProtocol>`. Box and Sliver have different `Geometry`/`Constraints` types, but the size difference is bounded — within one cache line of each other.
- Co-locating the four `DirtySets` Vecs in one struct (vs. four scattered fields on PipelineOwner) puts them on one cache line of pointers, which is occasionally relevant during frame setup but rarely a hot point.

**Where `Arc`/`Mutex`/`HashMap`/`Box`/`dyn Trait` are acceptable.**
- `Arc<PipelineOwnerHandle>` (per process; one allocation) — for cross-thread mark-dirty notifications.
- `HashMap<TypeId, Box<dyn ViewBuilder>>` in InheritedView lookup (lives in `flui-view`, not here).
- `Box<dyn RenderObject<P>>` in `RenderEntry` — the open-set plugin boundary; one alloc per node, long-lived.
- `dyn ParentData` — same story for variable parent-data shapes per render object; allocated at adoption time, lives until detach.

**Where they are forbidden.**
- `Arc<RwLock<RenderTree>>` — never. The tree has one owner.
- `Arc<RwLock<PipelineOwner>>` — never. The pipeline owner has one owner. Cross-thread access goes through `PipelineOwnerHandle`'s channel, not a shared lock.
- `Mutex<HashMap<RenderId, _>>` — never. State lives on the entry, not in side tables.
- `Box<dyn Fn() + Send + Sync>` callback fields on hot data — fold into one observer pattern with a typed event enum.

---

## 10. Error Model

```rust
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    // ── Recoverable / retryable ──
    #[error("dirty channel full ({pending} pending, cap {capacity})")]
    DirtyChannelBackpressure { pending: usize, capacity: usize },

    // ── Terminal-for-this-frame, retry next frame ──
    #[error("invalid geometry from {render_object}: {reason}")]
    InvalidGeometry { render_object: &'static str, reason: &'static str },

    #[error("unbounded constraint at {render_object}; parent must provide bounds")]
    UnboundedConstraint { render_object: &'static str },

    #[error("layout depth limit exceeded ({limit}); infinite recursion suspected")]
    LayoutDepthExceeded { limit: usize },

    // ── Programmer error / structural ──
    #[error("phase violation: cannot {operation} during {phase} phase")]
    PhaseViolation { operation: &'static str, phase: &'static str },

    #[error("render id {id:?} not found in tree")]
    UnknownRenderId { id: RenderId },

    #[error("render id {id:?} has wrong protocol; expected {expected}, got {actual}")]
    ProtocolMismatch { id: RenderId, expected: &'static str, actual: &'static str },

    // ── Poison / panic ──
    #[error("render object {render_object} panicked during {phase}")]
    Poisoned { render_object: &'static str, phase: &'static str },

    // ── Shutdown / lifecycle ──
    #[error("pipeline owner dropped; handle no longer valid")]
    DirtyChannelClosed,
}
```

**Retryable** — `DirtyChannelBackpressure`. The producer should back off and try again.

**Terminal for this frame, retry next frame** — `InvalidGeometry`, `UnboundedConstraint`, `LayoutDepthExceeded`. The frame is dropped; the caller (frame loop) decides whether to retry.

**User-facing** — `InvalidGeometry`, `UnboundedConstraint`. These can be surfaced to the developer building widgets so they fix their layout logic.

**Internal only** — `PhaseViolation`, `UnknownRenderId`, `ProtocolMismatch`. Programmer-error signal; should never reach an end-user.

**Security-sensitive** — none. The errors do not embed render-object config in their messages; they carry `&'static str` debug names. If a render object's `debug_name` happens to embed secrets, that is again the render object's bug.

`anyhow::Error` is **never** returned from this crate's public API. Internally, `anyhow::Context` may wrap diagnostics inside test code, but the public surface is `RenderError` only.

---

## 11. Tests Required

Each test must prove a design guarantee.

**Invariants on `RenderTree`.**
- `tree.insert(...).get(...)` round-trips; the inserted node is found at the returned ID.
- IDs are 1-based and never zero; converting via `RenderId::new(0)` is a compile error or panics.
- Removing a node removes its slot but leaves other IDs untouched.

**Invariants on `RenderState`.**
- `mark_needs_layout()` sets the bit; `clear_needs_layout()` clears it.
- `set_was_repaint_boundary(true)` followed by `was_repaint_boundary()` returns `true`.
- `flags.is_dirty()` matches `flags.contains(dirty_flags())`.
- After `mark_needs_paint`, `needs_paint()` is `true` AND ancestors' `needs_paint` is `true` (propagation).

**Phase-typestate.**
- A test that attempts to call `run_paint()` on `PipelineOwner<Idle>` is a compile error. (Use a `compile_fail` doc-test.)
- `into_layout().run_layout().into_paint().run_paint()` succeeds and returns a `LayerTree`.
- `run_frame()` composes the transitions and returns to `Idle`.

**Cancellation.** Not applicable (sync pipeline). Replace with: "frame errors leave the tree in a consistent state."
- A render object that panics in `perform_layout` poisons that node; the next `run_frame()` either skips that node (if removed) or returns `Poisoned` again.
- An `InvalidGeometry` return preserves the previous geometry — the broken layout phase does not zero out the size.

**Retry / idempotency.**
- Running `run_frame()` on a clean tree produces an empty dirty set.
- Running `run_frame()` twice in a row on the same tree produces the same `LayerTree` byte-for-byte.

**Authorization.** Not applicable.

**Malformed input.**
- `NaN` constraint values propagate as `RenderError::InvalidGeometry`, do not panic.
- `f32::INFINITY` constraints are accepted (legitimate for unbounded scrolls) but yield `UnboundedConstraint` if the render object cannot handle them.
- A render object claiming a `Size` larger than `f32::MAX / 2` is rejected.

**Concurrency.**
- `PipelineOwnerHandle::request_mark_dirty` from another thread enqueues into the channel; the next `run_frame()` observes it.
- Dropping the `PipelineOwner` returns `DirtyChannelClosed` to outstanding handles.

**Property tests.**
- For any sequence of `(insert, mark_dirty, run_frame)` operations, the tree is consistent: every reachable `RenderId` has a state, every dirty `RenderId` is in the dirty set, no orphans.
- For any tree of depth ≤ depth limit, `run_layout` terminates.

**Loom tests.**
- The `PipelineOwnerHandle` channel send/recv sequencing is sound under concurrent senders.
- `AtomicRenderFlags` set/clear/read interleaving preserves the dirty bit's "set" guarantee under concurrent mark-dirty.

**Miri tests.**
- Add `cargo +nightly miri test -p flui-rendering` as a CI gate. The slab access patterns, atomic memory orderings, and any unsafe `get_many_mut` re-entrancy primitive are exactly the surface that benefits from Miri.

**Integration tests.**
- End-to-end: build a 3-deep render tree with one root, one row, three leaves; run a frame; assert the emitted `LayerTree` has the expected shape.
- Mark a leaf dirty, run a frame, assert only the changed subtree is in `nodes_needing_paint` (repaint boundary effectiveness).

---

## 12. Rejected Designs

For each rejected design: what it was, why it was tempting, why it is wrong here.

### Hierarchical pipeline owners (Flutter parity)

**What:** `PipelineOwner` adopts child `Arc<RwLock<PipelineOwner>>` and recursively flushes each phase across the tree of owners.

**Why tempting:** Flutter has this. It is convenient for multi-window scenarios where each window has its own root render object.

**Why wrong:** Multi-window does not require nested pipelines. It requires multiple `PipelineOwner` instances. `flui-app` (the windowing crate) owns the multiplicity; this crate owns one pipeline. The current `Arc<RwLock<PipelineOwner>>` for "children" creates `Arc<RwLock<_>>` cycles on tree structures — exactly the anti-pattern the constitution forbids.

### `Arc<Mutex<State>>` god-object

**What:** Replace `PipelineOwner`'s typestate with `Arc<Mutex<PipelineState>>` and pass it everywhere.

**Why tempting:** "Easier" than threading typestate through every method. Familiar to developers from less-strict languages.

**Why wrong:** Loses the compile-time phase invariant. The runtime check is strictly worse than the type system check. Also enables lock contention on the hot path, the U2 refactor specifically rejected.

### One giant `RenderContext` parameter

**What:** Pass a single `&mut RenderContext` to every method on `RenderObject` containing tree, owner, layer tree, semantics — everything.

**Why tempting:** Reduces the parameter list to one. Mirrors React's "context" pattern.

**Why wrong:** "Everything" is the wrong granularity. Layout phase has different needs than paint phase has different needs than hit-test. The four separate `LayoutContext` / `PaintContext` / `HitTestContext` / `MountContext` types let each phase pass exactly what's needed and statically forbid each other's operations.

### Raw `String` or `Uuid` for `RenderId`

**What:** Use string identifiers from the widget's `key` field for `RenderId`.

**Why tempting:** Decouples render-tree storage from the slab.

**Why wrong:** Hashed lookups in the hot path. Loses the NonZeroUsize niche optimisation. Doesn't compose with parent-data slabs.

### Dynamic `serde_json::Value` for `ParentData`

**What:** Make `ParentData` a JSON-blob type so any render object can attach arbitrary data without defining a Rust type.

**Why tempting:** Dart's dynamic typing makes this look like the natural translation.

**Why wrong:** Throws away type safety, allocates a JSON tree per node, requires every reader to validate. The current `Box<dyn ParentData>` with `Downcast` is the right shape — type-safe, one allocation, downcast on access.

### Unbounded channel for `mark_dirty` requests

**What:** Use `tokio::sync::mpsc::UnboundedSender` or `crossbeam::unbounded`.

**Why tempting:** "Just absorb backpressure."

**Why wrong:** Backpressure has to land somewhere. An unbounded channel hides it inside heap growth; the system OOMs instead of returning an error. Bounded channels surface the problem at the boundary where it can be handled.

### Panic-based invariants

**What:** Use `unreachable!()` and `assert!` at runtime to enforce phase ordering, ID validity, etc.

**Why tempting:** Easy to write. Catches violations immediately.

**Why wrong:** Defers compile-time checks to runtime. Crashes user processes on framework bugs. Use typestate + `Result` where it can encode the constraint at compile time; reserve `debug_assert` for invariants that genuinely cannot be type-encoded.

### Trait-heavy render-object plugin architecture

**What:** Define a forest of capability traits (`HasChildren`, `HasPaint`, `HasHitTest`, `HasSemantics`, …) and let plugin authors mix and match.

**Why tempting:** Maximally flexible. Looks like good OO design.

**Why wrong:** Render objects always have layout, almost always have paint, often have hit-test, sometimes have semantics. The mixin lattice is small enough that one trait with optional methods (defaults) is simpler than a graph of traits. The current trait is over-decomposed *in a different direction* (lots of methods, one trait) but the cure is to slim the methods, not to spawn more traits.

### Generic `Pipeline<R: RenderObject>` (monomorphic per object type)

**What:** Make the pipeline owner generic on the root render object's concrete type, monomorphising the tree.

**Why tempting:** Eliminates the `Box<dyn>` on the root.

**Why wrong:** Render trees are heterogeneous; you cannot pick one concrete `R`. The dyn dispatch on the trait is the actual cost of heterogeneity, and it's bounded at one vtable lookup per node per phase. Trying to remove it pushes complexity elsewhere.

### "Helper" submodules: `pipeline/helpers.rs`, `storage/helpers.rs`

**What:** Group utility functions in `helpers.rs`.

**Why tempting:** Avoids cluttering the main module.

**Why wrong:** "Helper" is a naming smell. If a function is genuinely shared, it belongs on the type it manipulates or in a named submodule about its concern. Reject.

---

## 13. Implementation Plan

Ordered. Each step lands as a reviewable PR. Each step compiles and passes tests independently.

### Step 1 — Phase typestate skeleton

- Add `pipeline/phase.rs` with sealed `Idle`/`Layout`/`Compositing`/`Paint`/`Semantics` markers.
- Add `PhantomData<Phase>` to `PipelineOwner`.
- Add transition methods (`into_layout`, etc.) — no behavior change yet, they just rebind the phase.
- Mark the existing `flush_*` methods as deprecated, route them through the new transitions.

**Verifies:** typestate compiles; existing code still works through deprecation aliases.

### Step 2 — Co-locate dirty sets

- Introduce `pipeline/dirty.rs` with `DirtySets { needs_layout, needs_paint, needs_compositing, needs_semantics }`.
- Replace the four `Vec<DirtyNode>` fields on `PipelineOwner` with `DirtySets`.
- Update all add/get methods.

**Verifies:** behaviour unchanged; one less moving part.

### Step 3 — Single notifier; remove three-callback constructor

- Introduce `VisualUpdateNotifier` (channel-based or observer struct).
- Remove `on_need_visual_update`, `on_semantics_owner_created`, `on_semantics_owner_disposed` callback fields.
- Remove `with_callbacks` constructor variant.
- Provide one `pub fn on_visual_update(&self) -> VisualUpdateSubscription` API instead.

**Verifies:** `flui-view` and `flui-app` (the only callers) migrate cleanly. Three `Box<dyn Fn() + Send + Sync>` go away.

### Step 4 — Delete dead and single-impl traits

- Delete `HitTestDispatcher` (0 production impls).
- Inline `PipelineManifold` into the one caller; delete the trait.
- Inline `ViewHitTestable` into the one caller; delete the trait.
- Re-evaluate `RendererBinding` — if it has multiple non-test impls it stays; if not, fold into a concrete adapter struct in `flui-view`.

**Verifies:** trait count drops by 3-4. No behaviour change.

### Step 5 — Delete `arity.rs`, `child_handle.rs`, `children_access.rs`

- Move callers to import `flui_tree::Arity` directly.
- Replace closure-based child iteration with `for child_id in node.children() { ... }` patterns. Where the borrow checker fights, use the disjoint-borrow primitive (U2 Outstanding) instead of fighting it via closures.

**Verifies:** ~880 LOC deleted, no functionality lost.

### Step 6 — Split `state.rs` into `state/{flags,geometry,constraints,offset,propagation}.rs`

- Create `state/` directory with five files.
- Move types and impls from the 1738-LOC `state.rs` into their new homes.
- `RenderState<P>` becomes a thin facade in `state/mod.rs` exposing the composed API.

**Verifies:** behaviour unchanged; each new file has a focused invariant and is ~150-300 LOC.

### Step 7 — Phase consuming transitions

- Make `into_layout` etc. consume `self` and return the new-typed owner (currently they could just be conversions).
- Update `flush_layout` / `flush_paint` callers to use the new transitions.
- Remove the deprecated aliases from Step 1.

**Verifies:** invalid phase calls (e.g. `paint()` on `Idle`) become compile errors.

### Step 8 — `PipelineOwnerHandle` + bounded dirty channel

- Add `pipeline/handle.rs` with `PipelineOwnerHandle::request_mark_dirty`.
- Wire up the bounded channel; default capacity 256.
- Provide `PipelineOwner<_>::handle()` accessor.
- Replace any cross-thread mark-dirty patterns in `flui-view`/`flui-app` with handles.

**Verifies:** the existing "shared pipeline owner across threads" use cases work without `Arc<RwLock<PipelineOwner>>`.

### Step 9 — Drop hierarchical pipelines

- Delete `adopt_child`, `drop_child`, `child_count`, `children` from `PipelineOwner`.
- Delete the `children: Vec<Arc<RwLock<PipelineOwner>>>` field.
- Migrate any multi-window callers in `flui-app` to own multiple `PipelineOwner` instances directly.

**Verifies:** `Arc<RwLock<PipelineOwner>>` count drops to zero in the workspace.

### Step 10 — Disjoint-borrow primitive in `RenderTree`

- Add `RenderTree::get_two_mut(parent, child)` and `get_many_mut(parent, &[children])`.
- Implementation uses `split_at_mut` on the slab's underlying vec, or a local `unsafe` block with disjoint-keys invariant and unit tests.
- Wire `layout_node_with_children` to use it, filling in `propagate_constraints_to_child` and `sync_child_size_to_parent` (today both are empty stubs).

**Verifies:** layout actually runs through children; `RenderEntry::layout` finally has production callers.

### Step 11 — Slim `RenderObject<P>` trait

- Audit the 18 methods on the trait.
- Move capability-specific methods to extension traits (`HitTestCapability`, `SemanticsCapability`).
- Drop `Send + Sync + 'static` from the bound to `'static` only. Add `Send + Sync` only on the types that genuinely cross thread boundaries (which is none in the render path).

**Verifies:** the trait surface shrinks; implementors that hold `Rc<T>` for shared widget state become valid.

### Step 12 — Error model

- Replace `Result<..., String>` / `panic!` / `unimplemented!` paths with `RenderError` variants.
- Wrap layout/paint calls in `catch_unwind` and surface `Poisoned`.
- Add depth-limit check to `layout_node_with_children`.

**Verifies:** poisoning is recoverable; depth limits prevent infinite recursion.

### Step 13 — Tests pass

- Property tests for tree consistency.
- Loom tests for `AtomicRenderFlags` and the dirty channel.
- Miri test for the disjoint-borrow primitive.
- `compile_fail` doctests for phase typestate.

**Verifies:** the design guarantees from Phase 4 are actual guarantees.

### Step 14 — Performance pass

- Profile a 1000-node frame; verify no `Arc::clone` in the paint loop, no `HashMap` lookups.
- Profile a 10,000-node frame; verify cache layout of `RenderEntry<P>` (size estimate from Phase 9).
- Bench against pre-refactor numbers if any.

**Verifies:** the data-oriented invariants hold.

---

## Self-check

- **Did I start from data, not traits?** Yes. `RenderTree`/`RenderNode`/`RenderEntry`/`RenderState` are the spine. `RenderObject` is the plugin boundary that hangs off the data.
- **Did every module earn its existence?** Three modules (`arity`, `child_handle`, `children_access`) and four traits (`PipelineManifold`, `HitTestDispatcher`, `ViewHitTestable`, `RendererBinding`-maybe) flagged for deletion. `state.rs` flagged for split because its responsibilities are distinct.
- **Did I identify the state owner?** Yes. `PipelineOwner<Phase>`. Exactly one mutable instance per render pipeline; cross-thread access via `PipelineOwnerHandle` channel.
- **Did I define cancellation behavior?** Yes. The pipeline is sync; cancellation is not applicable. The frame either completes or returns an error.
- **Did I define trust boundaries?** Yes. `RenderObject<P>` is the third-party boundary; `catch_unwind` + validated constraints + bounded channel form the guard layer.
- **Did I avoid fake extensibility?** Yes. The capability traits and delegates that survive are the ones with ≥2 real implementations or a documented stable extension point. The rest are flagged for deletion.
- **Did I avoid Quick Win architecture?** The U2 refactor was a quick-win-shaped change (refactor one site). This redesign goes further: split god modules, kill dead traits, remove hierarchical pipelines, replace runtime phase bools with typestate.
- **Did I encode invariants in types where possible?** Yes. Phase typestate forbids invalid phase calls. `NonZeroUsize` IDs forbid zero. `OnceCell` enforces write-once on geometry. The disjoint-borrow primitive's `unsafe` block has a local, unit-testable safety invariant.
- **Did I reject bad alternatives?** Eight rejected designs documented in Section 12.
- **Could a Rust developer implement this design without guessing?** Yes, given the implementation plan in Section 13 and the type sketches in Section 3.
