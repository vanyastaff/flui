# flui-rendering Architecture

This document is the per-crate template instance for `flui-rendering` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the Flutter → Rust mapping for this crate, the divergence decisions taken so far, the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper architectural write-ups for individual subsystems (protocol, layout, paint, hit-test) live alongside this file under [`docs/`](docs/) and migration plans under [`migration/`](migration/). The Flutter class hierarchy walk lives in [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) as a sibling appendix and is referenced from `## Flutter source mapping` below.

---

## Flutter source mapping

| Flutter source | FLUI module | Notes |
|---|---|---|
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` | [`src/storage/entry.rs`](src/storage/entry.rs), [`src/storage/state.rs`](src/storage/state.rs), [`src/storage/flags.rs`](src/storage/flags.rs), [`src/traits/render_object.rs`](src/traits/render_object.rs) | The `RenderObject` base class is split: trait surface in `traits/render_object.rs`, owned storage in `storage/entry.rs`, mutable per-frame state in `storage/state.rs`, atomic flags in `storage/flags.rs`. The Flutter `AbstractNode` parent-linkage role is in [`src/storage/links.rs`](src/storage/links.rs). |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` `PipelineOwner` (line 1019+) | [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | Single-threaded phase serialisation. Flutter's `flushLayout` / `flushCompositingBits` / `flushPaint` / `flushSemantics` map to FLUI's `run_layout` / `run_compositing` / `run_paint` / `run_semantics`, each living on the matching `PipelineOwner<Phase>` impl block (typestate-enforced ordering, Mythos Step 7). Holds the root node and dirty lists. The `debug_doing_layout` / `debug_doing_paint` flags on the owner are the FLUI runtime analog of Flutter's `_debugActiveLayout` / `_debugDoingThisPaint` static asserts (kept as a debug-build cross-check; the type system is the load-bearing enforcement). |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/box.dart` | [`src/protocol/box_protocol.rs`](src/protocol/box_protocol.rs), [`src/parent_data/box_parent_data.rs`](src/parent_data/box_parent_data.rs) | `BoxConstraints`, `BoxParentData`, `Size`-based geometry. |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver.dart` | [`src/protocol/sliver_protocol.rs`](src/protocol/sliver_protocol.rs), [`src/parent_data/sliver_parent_data.rs`](src/parent_data/sliver_parent_data.rs) | Sliver protocol for scrollable layout. |
| `RenderObjectWithChildMixin`, `ContainerRenderObjectMixin` (`object.dart` lines 4160-4400+) | [`src/storage/links.rs`](src/storage/links.rs), [`src/parent_data/container_mixin.rs`](src/parent_data/container_mixin.rs) | Single-child + variable-children storage. Flutter uses Dart linked lists; FLUI stores `Vec<RenderId>` on the parent. |
| `proxy_box.dart`, `shifted_box.dart`, `flex.dart` | [`flui-objects`](../flui-objects/src/) crate | Concrete render objects (`Padding`, `Center`, `ColoredBox`, `Flex`, `Opacity`, `SizedBox`, `Transform`, …), extracted to the sibling `flui-objects` crate; this crate keeps only the protocol/pipeline machinery. |
| Layer-related (`layer.dart`, container layers) | `flui-layer` crate | Compositing layers live in a sibling crate per the layered DAG ([`docs/architecture.md`](../../docs/architecture.md)). |

The full Flutter class hierarchy is enumerated in the sibling appendix [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) (1352 LOC, generated from a class-name sweep of `.flutter/flutter-master/packages/flutter/lib/src/rendering/`). That file is kept as a search index; it is not part of the template proper.

Render-subtree relocation is deliberately narrower than Flutter's ambient
owner mutation: only `PipelineOwner<Idle>` can detach, attach, or release a
batch. Detach returns an opaque, non-cloneable `DetachedRenderSubtrees` token
bound to the originating owner by private `Rc` identity. Reattach and
finalization release consume that token, returning it inside a typed failure
after mutation-free preflight when owner, epoch, live-node, or topology checks
fail. Finalization release does not delete nodes; it authorizes the existing
deepest-first element unmount so view lifecycle hooks remain canonical.

---

## Mapping decisions

This section records places where the Rust shape diverges from the Dart shape and why. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md): state the rule (or absence of rule), the choice, the alternatives considered, the trade-off accepted.

### A lazy sliver's items get their set position from parent data, not a wrapper

**Rule:** a screen reader announces "item 12 of 100" from the platform's set-position concept.
Flutter produces the "12" by having each lazy delegate wrap every materialised item in an
`IndexedSemantics` (`addSemanticIndexes`, on by default), which is a
`SingleChildRenderObjectWidget` whose `RenderIndexedSemantics` sets
`SemanticsConfiguration.indexInParent`.

**Choice:** the semantics assembler reads the index the sliver *already* stamped in
`SliverMultiBoxAdaptorParentData` and sets `index_in_parent` from it, when nothing above already
set one. `IndexedSemantics` exists here too, as a public widget for content a sliver does not
own, and an explicit one wins — it runs through `describe_semantics_configuration` first, so a
caller who set an index deliberately is not overwritten.

**Alternatives considered:** transcribing the wrapper — rejected on two counts. It costs an
element and a render object per materialised item, measured directly (a three-item `ListView`
went from 8 render nodes to 11 while the wrapper version was in the tree). And the wrapper's
index is captured when the item is *built*, while the sliver's is maintained by the band walk, so
the two can disagree about where a row actually sits; reading the maintained one cannot drift by
construction.

**Trade-off accepted:** the announced position is the item's logical index, so a delegate cannot
declare an item to be *outside* the set — Flutter's `semanticIndexCallback` returning `null`, the
case a separator uses. FLUI has no separated-list constructor yet; when one lands it needs a way
to say "not a member", and that is when this gets revisited rather than now, on speculation.

**Improvement at the platform boundary too:** Flutter carries `indexInParent` and
`scrollChildCount` as separate fields and leaves each platform bridge to reconcile them into that
platform's set-position concept. AccessKit has the concept directly, so `accesskit_translation`
emits the pair — `position_in_set` (one-based, converted there from the framework's zero-based
index) and `size_of_set` — and drops a negative index rather than publishing a nonsensical
position.

**Replacement test:** `lazy_list_items_carry_their_set_position_without_a_wrapper`
(`crates/flui-widgets/tests/semantics.rs`), plus
`harness_indexed_semantics_reports_its_index_and_only_republishes_on_change`
(`crates/flui-objects/tests/render_object_harness.rs`) for the public widget's render object,
including that an unchanged index requests no semantics update.

### Layout marks semantics once per walk, at the dirty root

**Rule:** Flutter pairs `performLayout()` with `markNeedsSemanticsUpdate()` in *both* of
`RenderObject`'s layout entry points (`rendering/object.dart`, `layoutWithoutResize` and
`layout`), per object. Every node that lays out re-publishes its semantics geometry, which is
what makes a scroll update the accessibility tree at all: a viewport's offset listener requests
layout and nothing else.

**Choice:** the same guarantee, marked **once per layout walk on the dirty root**
(`layout_dirty_root`) rather than once per laid-out node.

**Alternatives considered:** recording every laid-out node in the arena and marking each, which
is the literal transcription — rejected on two counts. It is redundant: the arena walks the
subtree of the dirty root, so every node that laid out is already under it, and `try_graft_pass`
re-assembles a marked node's whole subtree. And it is expensive in a way the transcription hides:
each `add_node_needing_semantics` fires `fire_need_visual_update`, whose production callback asks
the platform to redraw, and the graft resolves every marked node by walking its ancestor chain —
so an N-node relayout would cost N redraw requests and O(N·depth) graft work.

**Trade-off accepted:** one unconditional call per layout walk. `mark_needs_semantics` is a
no-op while semantics is disabled, so a session with no accessibility client attached pays one
predictable branch. A relayout of a subtree re-assembles that subtree even where a node's own
geometry did not move — the graft's granularity is the anchor, not the node, which is the same
bargain the existing graft already makes.

**Replacement test:** `scrolling_republishes_the_semantics_rects`
(`crates/flui-widgets/tests/semantics.rs`). It scrolls a viewport whose rows are *all* inside the
cache band, so the frame materialises nothing new, and asserts on a build counter that no row
rebuilt — without that assertion the test measures a newly-built row's own semantics mark and
passes with the change reverted, which the first draft did.

### The hit-test path is driver-owned; the protocol carries no result accumulator

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — a contract may be improved, and an
improvement owes a record plus a replacement test. This is that record.

**Choice:** `HitTestCapability::Result` and `::Entry` are vocabulary only. There is no
`ctx.result()`, `result_mut()`, `add_hit(entry)` or `add_self(id)`: the driver
(`PipelineOwner`'s hit-test walk) owns the path and builds each entry from the node's own
`RenderId`. A render object says it was hit by returning `true`, or calls
`ctx.register_self_hit_entry()` to appear in the path without blocking what is behind it.

**The reference's shape:** Flutter's `hitTest` takes a `HitTestResult` and each render object
calls `result.add(BoxHitTestEntry(this, position))`. The accumulator is the protocol.

**Why the divergence is better here, in checkable terms:**

1. **A render object cannot get the id wrong**, because it never supplies one. Flutter's
   `add(BoxHitTestEntry(this, …))` takes the node as an argument; passing the wrong one, or
   adding twice, is expressible and silent.
2. **There is one writer, not N.** The driver knows the node, its transform and its position in
   the walk, so the entry is assembled once from state that cannot disagree with itself. FLUI's
   accumulator was the second writer, and — this is the finding that produced the deletion — it
   was *unread*: `add_self` compiled, ran, and did nothing, because nothing downstream consumed
   the protocol-level result (issue #844).
3. **The trap is gone rather than documented.** The broken call was the discoverable one: it took
   the id you were holding and read like the box-side API. Deleting it makes the wrong call
   impossible instead of warned against.

**Alternatives:**
- *Wire the accumulator so the driver bridge reads it* — the honest alternative, and the one to
  take if a consumer ever appears (a sliver assembling its own path). Rejected now for having no
  consumer: wiring a second writer into the hit path to serve nothing would add exactly the
  disagreement point item 2 removes.
- *Keep the API and document the trap* — rejected; the deleted method's own module already
  documented it in passing ("dead in production") and that stopped nobody.

**Replacement coverage:** `register_self_hit_entry` is exercised end-to-end by the widget-level
hit-test ports that dispatch through a real pipeline — the `Transform`, `ClipPath`, `ClipRect`,
`Wrap` and viewport-order cases in `crates/flui-widgets/tests/parity/`, each asserting a tap
reaches or misses a specific child. The deleted tests asserted a write landed in a structure
nobody read, so they were removed rather than adapted: they could not fail for a reason a user
would notice. `crates/flui-widgets/tests/parity/render_viewport_test.rs` carries the debug trail
of how the dead path was found.

### Lazy-sliver scroll correction keeps the first visible item stationary

**Rule:** Prime Directive rule 1 ("improve where a Flutter contract can be improved, record it, replace the oracle"); [ADR-0051](../../docs/adr/ADR-0051-anchor-stationary-scroll-correction.md).

**Choice:** `Virtualizer::set_measured` / `adapt_default_estimate` report the offset delta of the anchor (the first visible item) whenever an extent above it changes; the consumer sliver accumulates the deltas and emits them as `SliverGeometry::scroll_offset_correction` at the end of the pass, in either scroll direction. The viewport applies the correction and re-runs layout in the same pass, so the anchor never moves on screen.

**Alternatives:** Flutter's `RenderSliverList` retains each resident child's stale `layoutOffset`, walks forward from the first retained child with current sizes, and corrects only at a boundary — growth of a retained-but-invisible child shifts visible content. ADR-0003's original consumer note additionally withheld corrections during a backward scroll; measured on the oracle scene it changed nothing and, where it can act, it is a one-frame anchor drift.

**Accepted trade-off:** the `slivers_test.dart` 'inaccurate scroll offset' windows differ from the oracle's by exactly the growth Flutter shows as a jump (192 px in that scene); the pinned oracle stays `#[ignore]`d as the statement of the declined behaviour and a FLUI oracle stands beside it. Items above a jump that were never resident stay hinted until they enter the band (O(band) layout, ADR-0003), where Flutter's O(distance) walk would be exact.

### Render-tree storage uses a `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys

**Rule:** strategy clause "Behavior as floor, everything else designed for Rust"; constitution Anti-Patterns list ("`Arc<Mutex<>>` for tree structures — use arena/slotmap"); the ID-offset pattern documented in [`docs/architecture.md`](../../docs/architecture.md).

**Choice:** `RenderTree` stores `Slab<RenderNode>`. `RenderId` is a `NonZeroUsize` newtype that adds `+1` to the slab index, so `Option<RenderId>` niche-optimises to 8 bytes for parent / child references. The slab is reached from one strong root (`PipelineOwner::root_id`) and every other node is reached by walking child IDs in `NodeLinks`.

**Alternatives:** Flutter holds the tree as a graph of Dart references with direct child pointers on every render object. Direct translation would require `Arc<RwLock<RenderObject>>` or `Rc<RefCell<RenderObject>>` for parent/child cycles, which the constitution forbids for tree structures. `typed-arena::Arena` was considered but cannot delete individual entries, which the element reconciler needs.

**Accepted trade-off:** one extra indirection (slab lookup) on the tree-walk hot path, paid back by O(1) insert/delete, deterministic ID stability across mutations, and elimination of `Arc<Mutex<>>` cycles. The same pattern is used by `flui-view`'s `ElementTree`.

### `RenderEntry<P>` owns the render object by value (no lock, no interior mutability)

**Rule:** strategy clause "sync hot path, async на краях" (lock contention on the hot path is functionally async-flavoured); [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (`RwLock<Box<dyn RenderObject<P>>>` in `perform_layout` / `paint`).

**Choice:** `RenderEntry<P>::render_object` is a plain `Box<dyn RenderObject<P>>` (see [`src/storage/entry.rs`](src/storage/entry.rs)). Mutable access goes through `&mut self`, which the pipeline obtains via `PipelineOwner::render_tree_mut() -> &mut RenderTree` at phase boundaries. Re-entrant access from a parent to a child during layout uses disjoint-borrow primitives on `RenderTree` (`get_two_mut`, `get_many_mut`; the underlying `unsafe` is local and disjoint-keys-invariant — see [Thread safety](#thread-safety)). The Flutter `_debugDoingThisLayout` / `_debugDoingThisPaint` debug asserts are mirrored by `PipelineOwner::debug_doing_layout` / `debug_doing_paint` (see [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)).

**Alternatives considered (full study in [`docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md`](../../docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md)):**
- `OnceCell<Box<dyn>>` — rejected. `OnceCell::get()` returns `&T`; the trait still has `&mut self` methods that need mutation, so the lock would have to come back under another name.
- Arity-keyed enum dispatch — rejected. The trait is open-set via the blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T` (see [`src/traits/render_box.rs`](src/traits/render_box.rs)). Closing it to a known enum would force every user-defined render object into a derive-macro discipline and break the widget extensibility story.
- `RenderObjectId` indirection (render object lives in a separate slab keyed by ID) — considered. Adds one extra indirection per access and doubles the lifecycle invariants (insert/delete across two slabs). Equivalent soundness-wise but more moving parts than necessary.
- Inner-mutability split (immutable `Arc<dyn>` config + all mutation moved to `RenderState`) — considered. Largest API change of all the options; would force every concrete render object in `src/objects/` to be refactored. Filed as future work.

**Accepted trade-off:** the layout and update paths must hold `&mut RenderTree` for the duration of the phase. Multi-child layout requires the `get_many_mut` primitive. The borrow checker, not a lock, enforces single-writer-per-frame — closer to Flutter's actual model (single-threaded with debug asserts) than the previous `RwLock`-based shape.

### `set_was_repaint_boundary` removed from the trait surface; bit lives on `RenderState::flags`

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (the previous shape required a write lock on the trait object during paint to flip a single bool); strategy clause "Compile-time over runtime" (state bits belong on the bookkeeping layer, not the user-implementable trait surface).

**Choice:** added `RenderFlags::WAS_REPAINT_BOUNDARY` (bit 10 — see [`src/storage/flags.rs`](src/storage/flags.rs)) with `RenderState<P>::set_was_repaint_boundary` / `was_repaint_boundary` accessors. The paint phase at [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) (`paint_subtree`) writes the bit through an atomic store on `state().flags()` rather than locking the trait object. The trait method `RenderObject::set_was_repaint_boundary` is deleted (see [`src/traits/render_object.rs`](src/traits/render_object.rs)).

**Alternatives:** keep the trait method and live with the per-paint write lock — rejected, this is the canonical refusal-trigger violation. Move the bit to a per-tree side table — rejected, would add a second source-of-truth for state already structured around `RenderState<P>`.

**Accepted trade-off:** subclasses that wanted to override `set_was_repaint_boundary` (none currently do) lose the hook. The flag's owner is now framework code, not user code. This mirrors Flutter's actual model where `_wasRepaintBoundary` is a private field on `RenderObject` (`object.dart` line 3560) that no subclass overrides.

### `unsafe impl Send + Sync for RenderTree` removed

**Rule:** constitution Principle III ("zero unsafe in widget/app layer; `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine`"); the prior `unsafe impl` was a soundness carve-out documented in [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.

**Choice:** removed the `unsafe impl Send for RenderTree {}` / `unsafe impl Sync for RenderTree {}` block at the bottom of [`src/storage/tree.rs`](src/storage/tree.rs). The transitive Send+Sync chain still holds via auto-derivation: `Slab<RenderNode>` is auto-`Send + Sync` because `RenderNode` is; `RenderEntry<P>` holds `Box<dyn RenderObject<P>>` and the trait requires `Send + Sync + 'static`; `RenderState<P>` is built on atomics and `Option<T>` fields for geometry/constraints; `NodeLinks` is POD.

**Alternatives:** keep the unsafe impl as defensive cruft — rejected, the safety justification was load-bearing only because of `RwLock`'s interior mutability; with that gone, no unsafe carve-out is needed.

**Accepted trade-off:** net unsafe deletion, one fewer place where the carry-cost of a soundness comment exists.

### Third-party trait calls wrapped in `catch_unwind`; phases return `RenderResult<()>`

**Rule:** design verdict Section 7 ("Partial failure recovery: A render object that panics inside `perform_layout` or `paint` poisons that node only. The pipeline catches via `std::panic::catch_unwind`, marks the node as `RenderError::Poisoned`, drops the in-flight frame, and lets the caller decide.") and Section 10 (the `Poisoned { render_object, phase }` error variant). Mythos Step 12.

**Choice:** every third-party trait call site has its call wrapped in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))`. A panicking render object surfaces as `RenderError::Poisoned { render_object, phase }` rather than aborting the process. Specifically:

- `RenderEntry::layout` ([`src/storage/entry.rs`](src/storage/entry.rs)) wraps `render_object.perform_layout_raw(...)` and returns `RenderResult<ProtocolGeometry<P>>`. On the panic path, state is left untouched (`NEEDS_LAYOUT` stays set) so the next frame can retry. The retry is not unbounded: the pipeline counts consecutive layout failures per node and poisons nodes that fail structurally or exhaust the budget ([`src/pipeline/owner/poison.rs`](src/pipeline/owner/poison.rs)); a poisoned node is skipped in later walks until `mark_needs_layout` freshly invalidates it.
- `PipelineOwner::<PaintPhase>::paint_subtree` ([`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)) wraps `render_object.paint(context, offset)`, returns `RenderResult<()>`, and propagates Poisoned through the recursion via a captured error slot in the children-painting closure.

The phase entry points (`run_layout` / `run_compositing` / `run_paint` / `run_semantics`) now return `RenderResult<()>`. `run_frame` returns `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)` -- the owner **always** comes back at Idle so frame-loop callers can mutex-replace through it on both success and error paths.

`RenderObject<P>::debug_name(&self) -> &'static str` is the static identifier embedded in `RenderError::Poisoned`. Its default body monomorphizes per concrete impl via `core::any::type_name::<Self>()`; calling through `&dyn RenderObject<P>` yields the concrete type name because the vtable carries the monomorphized stub.

**Alternatives:**

- **Process-wide `panic::set_hook`** -- rejected, leaks pipeline concerns into global process state and can't differentiate phase-of-origin.
- **Cache `debug_name` on `RenderEntry<P>` at insertion** -- considered. Would avoid one vtable dispatch per error case. Not adopted because the dispatch happens only on the failure path (cold by definition), and the cache adds a `&'static str` field that pollutes every `RenderEntry<P>` in the common case.
- **Return `(PipelineOwner<Idle>, RenderError)` tuple on error** (shape (a) in the Mythos spec) -- rejected, awkward to compose; pattern-matching on `(_, Result<_>)` is cleaner than splitting the success and error tuples.

**Accepted trade-off:** `AssertUnwindSafe` is documented inline at each wrapper. The render object's internal state may be torn after a panic; the pipeline treats the node as poisoned and lets the caller drop or replace it. Process-level safety is preserved; the render tree itself is not corrupted.

**Note:** `hit_test_raw` is part of the `RenderObject<P>` trait, but the current pipeline owner does not invoke it directly -- hit testing is dispatched at the `RenderView` layer outside the frame pipeline. The catch_unwind helper around hit_test will land when hit testing is wired through the pipeline.

### Multi-source design references in this crate

Strategy clause "Behavior as floor, everything else designed for Rust" treats Flutter as the **semantic** floor, not the design. The structural shape of individual components in this crate has been informed by multiple Rust-side audited references as recorded in prior plans:

- `slab::Slab` storage pattern with `+1/-1` ID offset — internal precedent in [`src/storage/tree.rs`](src/storage/tree.rs); the offset rationale lives in [`docs/architecture.md`](../../docs/architecture.md).
- `Weak<RwLock<PipelineOwner>>` parent back-reference replacing a raw pointer — [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.
- Lock-free atomic dirty tracking (`AtomicRenderFlags` + `AtomicOffset`; geometry/constraints as `Option<T>` mutated via `&mut RenderState`) — documented in [`src/storage/state.rs`](src/storage/state.rs) module docstring.
- Multi-source design references (GPUI, Iced, Makepad, Vello, Skia) — [`docs/plans/2026-03-31-engine-hardening.md`](../../docs/plans/2026-03-31-engine-hardening.md) precedent for citing reference codebases beyond Flutter when the structural pattern fits Rust idioms better.

---

## Thread safety

`flui-rendering` runs in the render pipeline; per strategy clause "sync hot path", the hot frame loop is single-threaded. Sync primitives in this crate are limited to shared-infrastructure objects and lock-free atomics on per-node state. No primitive sits inside `perform_layout` / `paint` on a per-node basis.

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `RenderEntry<P>::render_object` (`src/storage/entry.rs`) | plain `Box<dyn RenderObject<P>>` | Owned by value | Mutable access via `&mut self` from `&mut RenderTree`. The previous `RwLock<Box<dyn>>` was the canonical refusal-trigger violation; removed by the U2 exemplar refactor. |
| `RenderState<P>::flags` (`src/storage/state.rs`) | `AtomicRenderFlags` (wrapping `AtomicU32`) | Lock-free atomics | Bit-level dirty flags + boundary bits. `Acquire/Release` ordering. The new `WAS_REPAINT_BOUNDARY` bit lives here. |
| `RenderState<P>::geometry`, `constraints` (`src/storage/state.rs`) | `Option<ProtocolGeometry<P>>` / `Option<ProtocolConstraints<P>>` | Mutable via `&mut self` | Set and cleared via `&mut RenderState` during layout; no lock required. |
| `RenderState<P>::offset` (`src/storage/state.rs`) | `AtomicOffset` | Lock-free atomics | Paint position. |
| `RenderTree::owner` (`src/storage/tree.rs:65`) | `Option<Arc<RwLock<PipelineOwner>>>` | Shared infrastructure | Allowed per [`docs/PORT.md`](../../docs/PORT.md) lock-decision table. Off the per-node hot path. |
| `PipelineOwner` parent/back-references throughout [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | `Arc<RwLock<PipelineOwner>>`, `Weak<RwLock<PipelineOwner>>` | Shared infrastructure | Soundness-rewrite precedent ([core-crates-hardening Task 7](../../docs/plans/2026-03-31-core-crates-hardening.md)). |
| `RenderTree::nodes` (`src/storage/tree.rs:59`) | `Slab<RenderNode>` | Auto-derived Send+Sync | No `unsafe impl` needed after U2. |
| Viewport listener list (`ScrollableViewportOffset::listeners`, `src/view/viewport_offset.rs`) | `RwLock<Vec<…>>` | Listener registry | Off layout/paint hot path. `FixedViewportOffset`'s former listener list was deleted as speculative API (a fixed offset never notifies). |

Two rows left this table because their sites left the crate: the mouse tracker
lives in `flui-interaction` (`src/routing/mouse_tracker.rs`) and the render-view
error builder in `flui-view` (`src/view/error.rs`); each is accounted for in its
owning crate.

`NodePtr` in `src/pipeline/owner/subtree_arena.rs` is a plain raw-pointer newtype for the disjoint-subtree-borrow substrate ([`SubtreeArena`]) — `!Send + !Sync` by the language default, no manual impl. Confinement to the constructing thread is structural (`SubtreeArena` itself is `!Send + !Sync`, pinned by `static_assertions::assert_not_impl_any!`); there is no runtime thread check (`check_thread` and the pointer's former `unsafe impl Send/Sync` were both deleted once `PipelineCell`/dropped `Send + Sync` bounds made confinement type-enforced). Re-entrancy primitives `RenderTree::get_two_mut` and `get_parent_and_children_mut` (both in `src/storage/tree.rs`) are implemented and shipped; their unsafe is local to each function with unit-testable disjoint-keys invariants.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

- **`PipelineOwner` paint-loop downcasts to `Box<dyn ContainerLayer>`** ([`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)) — the paint phase uses `Box<dyn ContainerLayer>` returned from `RenderObject::paint`. This is correct for compositing-layer heterogeneity but worth periodic audit to ensure the cost stays at the boundary, not in the per-frame inner loop.
- **`docs/PROTOCOL_ARCHITECTURE.md` predates this template** ([`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md)) — a deeper design write-up that overlaps with `## Flutter source mapping` above for protocol-specific concerns. Not migrated under this template in U3; remains as a companion document.
- **`docs/LAYOUT_SYSTEM.md`, `docs/PAINT_SYSTEM.md`, `docs/HIT_TEST_SYSTEM.md`** — subsystem-level deep-dives. Not part of the template surface. Stay as companion documents.

---

## Shipped infrastructure (formerly "Outstanding refactors")

These items were listed as pending in earlier drafts; all are now shipped.

### `RenderTree::get_two_mut` / `get_parent_and_children_mut` — SHIPPED

**File:** [`src/storage/tree.rs`](src/storage/tree.rs) (`get_two_mut`, `get_parent_and_children_mut`).

Tree-aware disjoint-borrow primitives. `get_two_mut(a, b)` returns `(&mut RenderNode, &mut RenderNode)` for two distinct keys; `get_parent_and_children_mut` generalises to a parent + N children. The unsafe is local to each function with a disjoint-keys assertion and is unit-tested.

### `layout_dirty_root` + `layout_subtree_borrowed` — SHIPPED

**Files:** [`src/pipeline/owner/layout.rs`](src/pipeline/owner/layout.rs) (`layout_dirty_root`), [`src/pipeline/owner/subtree_arena.rs`](src/pipeline/owner/subtree_arena.rs) (`layout_subtree_borrowed`).

`layout_dirty_root` is the dispatcher: it obtains disjoint `&mut`s via `SubtreeArena`, constructs a typed `BoxLayoutCtx` with children + callback, and calls `perform_layout_raw` through the erased view. The pipeline-driven path was built directly into this entry point; the phantom stubs that earlier documentation described were never real functions.

### `layout_leaf_only` — SHIPPED

**File:** [`src/storage/entry.rs:296`](src/storage/entry.rs).

The leaf-only layout method is implemented and exercised through the test harness and the pipeline path for pure-leaf objects.

### Move `RenderEntry<P>::clear_needs_paint` / `clear_needs_layout` to `RenderState<P>` — DONE

**File:** [`src/storage/entry.rs`](src/storage/entry.rs).

The forwarding wrappers left over from the previous lock-based API are deleted; every call site clears the flags through `entry.state().clear_needs_*()` directly, so the only API surface is `RenderState`.

### Criterion benchmarks for Mythos Step 14 (deferred -- needs workload generator)

**Files:** new `crates/flui-rendering/benches/frame_throughput.rs`.

**Goal:** Mythos Step 14 prescribed profiling a 1000-node and a 10,000-node frame to verify (a) no `Arc::clone` in the paint loop, (b) cache layout of `RenderEntry<P>`, (c) regressions vs pre-refactor numbers. Today the static memory-footprint assertions landed in `pipeline/dirty.rs` and `storage/state/tests.rs` (see Mythos Step 14 commit); the runtime benchmarks did not.

**Shape:** add a `benches/frame_throughput.rs` Criterion benchmark that:
- Builds a synthetic render tree of N nodes (parametric, e.g. N ∈ {100, 1000, 10000}).
- Marks the root dirty and runs one full `run_frame`.
- Measures wall-clock time, peak memory, and (with `cargo flamegraph`) hot-loop hot spots.

Criterion is already in `flui-rendering` dev-dependencies. The bench harness needs a workload generator (`fn build_flex_tree(depth: u32, children: u32) -> ...`) that produces realistic structures from the `flui-objects` catalog.

**Why deferred:** the workload generator + benchmark is its own scope of work and is best landed when there are real performance questions to answer (a frame is dropping, a particular operation feels slow, etc.). Premature optimisation guidance landed without observed evidence wastes effort.

**Dependencies:** none beyond existing dev-deps.

### Loom test coverage (deferred — miri and proptest already shipped)

**Files:** new `crates/flui-rendering/tests/loom_handle.rs`.

**Note:** `proptest` is already a dev-dependency and is used in `src/virtualization/tests.rs`. The miri half is LANDED: CI's advisory `miri` job runs `cargo +nightly miri test -p flui-rendering --lib pipeline::owner`, interpreting every unit test under `pipeline::owner` — the raw-pointer `SubtreeArena` substrate and the disjoint-borrow layout walks over it. Widening the filter to `storage::tree`'s own `get_two_mut` / `get_parent_and_children_mut` unit tests remains open alongside loom. The remaining deferred test class:

- **Loom tests** for `AtomicRenderFlags` set/clear/read interleaving + private dirty-channel send/recv sequencing across attachment epochs. Needs the `loom` crate gated on `#[cfg(loom)]`.

**Shape:** a new file under `crates/flui-rendering/tests/` plus a dev-dependency.

**Dependencies:** none beyond crate dev-deps.

### Migrate `docs/` companion architecture docs onto template-adjacent shape — DONE

**File:** [`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md), [`docs/LAYOUT_SYSTEM.md`](docs/LAYOUT_SYSTEM.md), [`docs/PAINT_SYSTEM.md`](docs/PAINT_SYSTEM.md), [`docs/HIT_TEST_SYSTEM.md`](docs/HIT_TEST_SYSTEM.md), [`docs/ROADMAP.md`](docs/ROADMAP.md).

These deep-dives stay as companion documents (not under the per-crate template directly); each now opens with a "See also" header line pointing back to this file, linking them into the methodology index.

---

## Notes

- **R12 lint promotion path is symbolic for Trigger 1.** [`docs/PORT.md`](../../docs/PORT.md) reactive-lint-promotion rule names `[workspace.lints.clippy]` as the first-promotion mechanism. The clippy lint vocabulary cannot today express "field of type `RwLock<X>` where `X` is a trait object locked in method `foo`". The grep regression in [`scripts/port-check.sh`](../../scripts/port-check.sh) is the durable enforcement layer; the clippy-promotion column waits for ecosystem expressivity (`dylint` plugin or a future clippy feature).
