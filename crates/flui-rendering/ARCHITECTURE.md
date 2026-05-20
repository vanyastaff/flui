# flui-rendering Architecture

This document is the per-crate template instance for `flui-rendering` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the Flutter → Rust mapping for this crate, the divergence decisions taken so far, the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper architectural write-ups for individual subsystems (protocol, layout, paint, hit-test) live alongside this file under [`docs/`](docs/) and migration plans under [`migration/`](migration/). The Flutter class hierarchy walk lives in [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) as a sibling appendix and is referenced from `## Flutter source mapping` below.

---

## Flutter source mapping

| Flutter source | FLUI module | Notes |
|---|---|---|
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` | [`src/storage/entry.rs`](src/storage/entry.rs), [`src/storage/state.rs`](src/storage/state.rs), [`src/storage/flags.rs`](src/storage/flags.rs), [`src/traits/render_object.rs`](src/traits/render_object.rs) | The `RenderObject` base class is split: trait surface in `traits/render_object.rs`, owned storage in `storage/entry.rs`, mutable per-frame state in `storage/state.rs`, atomic flags in `storage/flags.rs`. The Flutter `AbstractNode` parent-linkage role is in [`src/storage/links.rs`](src/storage/links.rs). |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` `PipelineOwner` (line 1019+) | [`src/pipeline/owner.rs`](src/pipeline/owner.rs) | Single-threaded phase serialisation (`flush_layout`, `flush_paint`, `flush_compositing_bits`, semantics). Holds the root node and dirty lists. The `debug_doing_layout` / `debug_doing_paint` flags on the owner are the FLUI analog of Flutter's `_debugActiveLayout` / `_debugDoingThisPaint` static asserts. |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/box.dart` | [`src/protocol/box_protocol.rs`](src/protocol/box_protocol.rs), [`src/parent_data/box_parent_data.rs`](src/parent_data/box_parent_data.rs) | `BoxConstraints`, `BoxParentData`, `Size`-based geometry. |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver.dart` | [`src/protocol/sliver_protocol.rs`](src/protocol/sliver_protocol.rs), [`src/parent_data/sliver_parent_data.rs`](src/parent_data/sliver_parent_data.rs) | Sliver protocol for scrollable layout. |
| `RenderObjectWithChildMixin`, `ContainerRenderObjectMixin` (`object.dart` lines 4160-4400+) | [`src/storage/links.rs`](src/storage/links.rs), [`src/parent_data/container_mixin.rs`](src/parent_data/container_mixin.rs) | Single-child + variable-children storage. Flutter uses Dart linked lists; FLUI stores `Vec<RenderId>` on the parent. |
| `proxy_box.dart`, `shifted_box.dart`, `flex.dart` | [`src/objects/`](src/objects/) | Concrete render objects: `Padding`, `Center`, `ColoredBox`, `Flex`, `Opacity`, `SizedBox`, `Transform`. |
| Layer-related (`layer.dart`, container layers) | `flui-layer` crate | Compositing layers live in a sibling crate per the layered DAG ([`docs/architecture.md`](../../docs/architecture.md)). |

The full Flutter class hierarchy is enumerated in the sibling appendix [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) (1352 LOC, generated from a class-name sweep of `.flutter/flutter-master/packages/flutter/lib/src/rendering/`). That file is kept as a search index; it is not part of the template proper.

---

## Mapping decisions

This section records places where the Rust shape diverges from the Dart shape and why. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md): state the rule (or absence of rule), the choice, the alternatives considered, the trade-off accepted.

### Render-tree storage uses a `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys

**Rule:** strategy clause "Behavior loyal, structure Rust-native"; constitution Anti-Patterns list ("`Arc<Mutex<>>` for tree structures — use arena/slotmap"); the ID-offset pattern documented in [`docs/architecture.md`](../../docs/architecture.md).

**Choice:** `RenderTree` stores `Slab<RenderNode>`. `RenderId` is a `NonZeroUsize` newtype that adds `+1` to the slab index, so `Option<RenderId>` niche-optimises to 8 bytes for parent / child references. The slab is reached from one strong root (`PipelineOwner::root_id`) and every other node is reached by walking child IDs in `NodeLinks`.

**Alternatives:** Flutter holds the tree as a graph of Dart references with direct child pointers on every render object. Direct translation would require `Arc<RwLock<RenderObject>>` or `Rc<RefCell<RenderObject>>` for parent/child cycles, which the constitution forbids for tree structures. `typed-arena::Arena` was considered but cannot delete individual entries, which the element reconciler needs.

**Accepted trade-off:** one extra indirection (slab lookup) on the tree-walk hot path, paid back by O(1) insert/delete, deterministic ID stability across mutations, and elimination of `Arc<Mutex<>>` cycles. The same pattern is used by `flui-view`'s `ElementTree`.

### `RenderEntry<P>` owns the render object by value (no lock, no interior mutability)

**Rule:** strategy clause "sync hot path, async на краях" (lock contention on the hot path is functionally async-flavoured); [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (`RwLock<Box<dyn RenderObject<P>>>` in `perform_layout` / `paint`).

**Choice:** `RenderEntry<P>::render_object` is a plain `Box<dyn RenderObject<P>>` (see [`src/storage/entry.rs`](src/storage/entry.rs)). Mutable access goes through `&mut self`, which the pipeline obtains via `PipelineOwner::render_tree_mut() -> &mut RenderTree` at phase boundaries. Re-entrant access from a parent to a child during layout uses disjoint-borrow primitives on `RenderTree` (`get_two_mut`, `get_many_mut`; the underlying `unsafe` is local and disjoint-keys-invariant — see [Thread safety](#thread-safety)). The Flutter `_debugDoingThisLayout` / `_debugDoingThisPaint` debug asserts are mirrored by `PipelineOwner::debug_doing_layout` / `debug_doing_paint` (see [`src/pipeline/owner.rs`](src/pipeline/owner.rs)).

**Alternatives considered (full study in [`docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md`](../../docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md)):**
- `OnceCell<Box<dyn>>` — rejected. `OnceCell::get()` returns `&T`; the trait still has `&mut self` methods that need mutation, so the lock would have to come back under another name.
- Arity-keyed enum dispatch — rejected. The trait is open-set via the blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T` (see [`src/traits/render_box.rs`](src/traits/render_box.rs)). Closing it to a known enum would force every user-defined render object into a derive-macro discipline and break the widget extensibility story.
- `RenderObjectId` indirection (render object lives in a separate slab keyed by ID) — considered. Adds one extra indirection per access and doubles the lifecycle invariants (insert/delete across two slabs). Equivalent soundness-wise but more moving parts than necessary.
- Inner-mutability split (immutable `Arc<dyn>` config + all mutation moved to `RenderState`) — considered. Largest API change of all the options; would force every concrete render object in `src/objects/` to be refactored. Filed as future work.

**Accepted trade-off:** the layout and update paths must hold `&mut RenderTree` for the duration of the phase. Multi-child layout requires the `get_many_mut` primitive. The borrow checker, not a lock, enforces single-writer-per-frame — closer to Flutter's actual model (single-threaded with debug asserts) than the previous `RwLock`-based shape.

### `set_was_repaint_boundary` removed from the trait surface; bit lives on `RenderState::flags`

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (the previous shape required a write lock on the trait object during paint to flip a single bool); strategy clause "Compile-time over runtime" (state bits belong on the bookkeeping layer, not the user-implementable trait surface).

**Choice:** added `RenderFlags::WAS_REPAINT_BOUNDARY` (bit 10 — see [`src/storage/flags.rs`](src/storage/flags.rs)) with `RenderState<P>::set_was_repaint_boundary` / `was_repaint_boundary` accessors. The paint phase at [`src/pipeline/owner.rs`](src/pipeline/owner.rs) (`paint_node_recursive`) writes the bit through an atomic store on `state().flags()` rather than locking the trait object. The trait method `RenderObject::set_was_repaint_boundary` is deleted (see [`src/traits/render_object.rs`](src/traits/render_object.rs)).

**Alternatives:** keep the trait method and live with the per-paint write lock — rejected, this is the canonical refusal-trigger violation. Move the bit to a per-tree side table — rejected, would add a second source-of-truth for state already structured around `RenderState<P>`.

**Accepted trade-off:** subclasses that wanted to override `set_was_repaint_boundary` (none currently do) lose the hook. The flag's owner is now framework code, not user code. This mirrors Flutter's actual model where `_wasRepaintBoundary` is a private field on `RenderObject` (`object.dart` line 3560) that no subclass overrides.

### `unsafe impl Send + Sync for RenderTree` removed

**Rule:** constitution Principle III ("zero unsafe in widget/app layer; `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine`"); the prior `unsafe impl` was a soundness carve-out documented in [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.

**Choice:** removed the `unsafe impl Send for RenderTree {}` / `unsafe impl Sync for RenderTree {}` block at the bottom of [`src/storage/tree.rs`](src/storage/tree.rs). The transitive Send+Sync chain still holds via auto-derivation: `Slab<RenderNode>` is auto-`Send + Sync` because `RenderNode` is; `RenderEntry<P>` holds `Box<dyn RenderObject<P>>` and the trait requires `Send + Sync + 'static`; `RenderState<P>` is built on atomics and `OnceCell`; `NodeLinks` is POD.

**Alternatives:** keep the unsafe impl as defensive cruft — rejected, the safety justification was load-bearing only because of `RwLock`'s interior mutability; with that gone, no unsafe carve-out is needed.

**Accepted trade-off:** net unsafe deletion, one fewer place where the carry-cost of a soundness comment exists.

### Multi-source design references in this crate

Strategy clause "Behavior loyal, structure Rust-native" treats Flutter as the **semantic** reference. The structural shape of individual components in this crate has been informed by multiple Rust-side audited references as recorded in prior plans:

- `slab::Slab` storage pattern with `+1/-1` ID offset — internal precedent in [`src/storage/tree.rs`](src/storage/tree.rs); the offset rationale lives in [`docs/architecture.md`](../../docs/architecture.md).
- `Weak<RwLock<PipelineOwner>>` parent back-reference replacing a raw pointer — [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.
- Lock-free atomic dirty tracking (`AtomicRenderFlags` + `OnceCell` + `AtomicOffset`) — documented in [`src/storage/state.rs`](src/storage/state.rs) module docstring ("10x faster than RwLock").
- Multi-source design references (GPUI, Iced, Makepad, Vello, Skia) — [`docs/plans/2026-03-31-engine-hardening.md`](../../docs/plans/2026-03-31-engine-hardening.md) precedent for citing reference codebases beyond Flutter when the structural pattern fits Rust idioms better.

---

## Thread safety

`flui-rendering` runs in the render pipeline; per strategy clause "sync hot path", the hot frame loop is single-threaded. Sync primitives in this crate are limited to shared-infrastructure objects and lock-free atomics on per-node state. No primitive sits inside `perform_layout` / `paint` on a per-node basis.

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `RenderEntry<P>::render_object` (`src/storage/entry.rs`) | plain `Box<dyn RenderObject<P>>` | Owned by value | Mutable access via `&mut self` from `&mut RenderTree`. The previous `RwLock<Box<dyn>>` was the canonical refusal-trigger violation; removed by the U2 exemplar refactor. |
| `RenderState<P>::flags` (`src/storage/state.rs`) | `AtomicRenderFlags` (wrapping `AtomicU32`) | Lock-free atomics | Bit-level dirty flags + boundary bits. `Acquire/Release` ordering. The new `WAS_REPAINT_BOUNDARY` bit lives here. |
| `RenderState<P>::geometry`, `constraints` (`src/storage/state.rs`) | `OnceCell<...>` | Write-once read-many | Set during layout, read during paint. |
| `RenderState<P>::offset` (`src/storage/state.rs`) | `AtomicOffset` | Lock-free atomics | Paint position. |
| `RenderTree::owner` (`src/storage/tree.rs:65`) | `Option<Arc<RwLock<PipelineOwner>>>` | Shared infrastructure | Allowed per [`docs/PORT.md`](../../docs/PORT.md) lock-decision table. Off the per-node hot path. |
| `PipelineOwner` parent/back-references throughout [`src/pipeline/owner.rs`](src/pipeline/owner.rs) | `Arc<RwLock<PipelineOwner>>`, `Weak<RwLock<PipelineOwner>>` | Shared infrastructure | Soundness-rewrite precedent ([core-crates-hardening Task 7](../../docs/plans/2026-03-31-core-crates-hardening.md)). |
| `RenderTree::nodes` (`src/storage/tree.rs:59`) | `Slab<RenderNode>` | Auto-derived Send+Sync | No `unsafe impl` needed after U2. |
| Viewport listener lists (`src/view/viewport_offset.rs:138, 262`) | `RwLock<Vec<…>>` | Listener registry | Off layout/paint hot path. |
| Mouse tracker maps (`src/input/mouse_tracker.rs:294-303`) | `RwLock<HashMap<…>>` | Tracker state | Off layout/paint hot path. |
| Render view error builder (`src/view/error.rs`, via `flui-view` integration) | `static RwLock<Option<...>>` | Process-wide singleton | Off any hot path. |

No `unsafe impl Send/Sync` in this crate. Re-entrancy primitives (`RenderTree::get_two_mut`, `get_many_mut`) will use a small `unsafe` block when added — local invariant (disjoint slab keys), unit-testable. This is queued in [Outstanding refactors](#outstanding-refactors) and not yet implemented.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

- **`PipelineOwner` paint-loop downcasts to `Box<dyn ContainerLayer>`** ([`src/pipeline/owner.rs`](src/pipeline/owner.rs)) — the paint phase uses `Box<dyn ContainerLayer>` returned from `RenderObject::paint`. This is correct for compositing-layer heterogeneity but worth periodic audit to ensure the cost stays at the boundary, not in the per-frame inner loop.
- **`docs/PROTOCOL_ARCHITECTURE.md` predates this template** ([`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md)) — a deeper design write-up that overlaps with `## Flutter source mapping` above for protocol-specific concerns. Not migrated under this template in U3; remains as a companion document.
- **`docs/LAYOUT_SYSTEM.md`, `docs/PAINT_SYSTEM.md`, `docs/HIT_TEST_SYSTEM.md`** — subsystem-level deep-dives. Not part of the template surface. Stay as companion documents.

---

## Outstanding refactors

Concrete cleanups visible from `flui-rendering` outward, sized for an `/aif-implement` dispatch. Each entry names a file and what would need to change.

### `RenderTree::get_two_mut` / `get_many_mut` disjoint-borrow primitives

**File:** [`src/storage/tree.rs`](src/storage/tree.rs).

**Goal:** add tree-aware re-entrancy primitives so a parent's layout phase can hold `&mut RenderNode` for itself and `&mut RenderNode` for each child simultaneously through one disjoint-key call. Today, the multi-child layout path at [`src/pipeline/owner.rs`](src/pipeline/owner.rs) (`layout_node_with_children` and helpers) has empty stubs at `propagate_constraints_to_child` and `sync_child_size_to_parent`; the disjoint-borrow primitive is the prerequisite for filling them in.

**Shape:** `RenderTree::get_two_mut(parent: RenderId, child: RenderId) -> Option<(&mut RenderNode, &mut RenderNode)>` panics if `parent == child`. Implementation uses `split_at_mut` on the slab's underlying entries vec, or `unsafe` with two raw pointers + a disjoint-keys assertion. The unsafe (if used) is local to a single function with a unit-testable safety invariant.

**Dependencies:** none beyond the U2 baseline.

### Migrate `RenderEntry::layout` from synchronous direct invocation to pipeline-driven

**File:** [`src/pipeline/owner.rs`](src/pipeline/owner.rs).

**Goal:** today `RenderEntry::layout` has no production callers; `layout_node_with_children` recurses through children but `propagate_constraints_to_child` (line 680) and `sync_child_size_to_parent` (line 687) are empty stubs. The plan is to wire `RenderEntry::layout` into `layout_node_with_children` once the disjoint-borrow primitive (above) lands. Until then, layout is exercised only through the test fixture in `flui-rendering/tests/`.

**Shape:** `layout_node_with_children` becomes the dispatcher: it computes the child constraint, calls `tree.get_two_mut(parent, child)` to obtain disjoint `&mut`s, calls `child_entry.layout(child_constraints)`, reads the resulting geometry, and writes the parent's `child_offset` from it. Multi-child layout uses `get_many_mut`.

**Dependencies:** disjoint-borrow primitive (above).

### Migrate `RootRenderElement::attach_to_pipeline_owner` / `detach_from_pipeline_owner`

**File:** [`crates/flui-view/src/view/root.rs`](../flui-view/src/view/root.rs) lines 482, 489.

**Goal:** today both methods are `unimplemented!()` blockers. The migration is documented inline in the TODO comments at lines 477-481 and 486-488: insert into `pipeline_owner.render_tree_mut()`, get a `RenderId` back, call `set_root_id(Some(id))`, store the `RenderId` instead of the legacy `Arc<RwLock<RenderView>>` handle. Detach inverts.

**Shape:** straightforward `Arc<RwLock<RenderView>>` → `RenderId` migration. Adopt the ID-offset pattern.

**Dependencies:** none beyond the U2 baseline.

### Move `RenderEntry<P>::clear_needs_paint` / `clear_needs_layout` to `RenderState<P>`

**File:** [`src/storage/entry.rs`](src/storage/entry.rs).

**Goal:** these methods exist on `RenderEntry` for backward compatibility with the previous lock-based API. After the U2 refactor they just forward to `self.state.clear_*`. Worth inlining the call sites and removing the wrapper methods so the only API surface is `RenderState`. Low priority — pure tidy.

**Dependencies:** none.

### Criterion benchmarks for Mythos Step 14 (deferred -- needs workload generator)

**Files:** new `crates/flui-rendering/benches/frame_throughput.rs`.

**Goal:** Mythos Step 14 prescribed profiling a 1000-node and a 10,000-node frame to verify (a) no `Arc::clone` in the paint loop, (b) cache layout of `RenderEntry<P>`, (c) regressions vs pre-refactor numbers. Today the static memory-footprint assertions landed in `pipeline/dirty.rs` and `storage/state/tests.rs` (see Mythos Step 14 commit); the runtime benchmarks did not.

**Shape:** add a `benches/frame_throughput.rs` Criterion benchmark that:
- Builds a synthetic render tree of N nodes (parametric, e.g. N ∈ {100, 1000, 10000}).
- Marks the root dirty and runs one full `run_frame`.
- Measures wall-clock time, peak memory, and (with `cargo flamegraph`) hot-loop hot spots.

Criterion is already in `flui-rendering` dev-dependencies. The bench harness needs a workload generator (`fn build_flex_tree(depth: u32, children: u32) -> ...`) that produces realistic structures from the existing `objects/` catalog.

**Why deferred:** the workload generator + benchmark is its own scope of work and is best landed when there are real performance questions to answer (a frame is dropping, a particular operation feels slow, etc.). Premature optimisation guidance landed without observed evidence wastes effort.

**Dependencies:** none beyond existing dev-deps.

### Property, loom, and miri test coverage (deferred from Mythos Step 13)

**Files:** new `crates/flui-rendering/tests/proptest_tree.rs`, new `crates/flui-rendering/tests/loom_handle.rs`, miri CI invocation.

**Goal:** Mythos Step 13 prescribed four test classes; this work landed only the compile_fail typestate doctests (the cheapest of the four) plus the targeted unit tests added alongside each refactor (disjoint-borrow tree, VisualUpdateNotifier, PipelineOwnerHandle). The three deferred classes are:

- **Property tests** for tree consistency: invariants that hold over any sequence of insert/mark_dirty/run_frame operations -- every reachable RenderId has a state, every dirty RenderId is in the dirty set, no orphans. Needs the `proptest` crate as a dev-dependency.
- **Loom tests** for `AtomicRenderFlags` set/clear/read interleaving + `PipelineOwnerHandle` send/recv sequencing. Needs the `loom` crate gated on `#[cfg(loom)]`.
- **Miri CI gate** for the disjoint-borrow `unsafe` block in `RenderTree::get_two_mut` / `get_parent_and_children_mut`. Today the unsafe is unit-tested for behavior; miri-checking the aliasing model is a CI extension (e.g. `cargo +nightly miri test -p flui-rendering`).

**Shape:** each class is a new file under `crates/flui-rendering/tests/` plus a dev-dependency. Adding all three at once would expand the Mythos chain into infrastructure work; they are properly scoped as a follow-up commit.

**Dependencies:** none beyond crate dev-deps.

### `catch_unwind` plumbing for `RenderError::Poisoned` (deferred from Mythos Step 12)

**Files:** [`src/pipeline/owner.rs`](src/pipeline/owner.rs), [`src/traits/render_object.rs`](src/traits/render_object.rs).

**Goal:** the `RenderError::Poisoned { render_object, phase }` variant landed in Mythos Step 12, but the actual `std::panic::catch_unwind` plumbing around `perform_layout_raw` / `paint` / `hit_test_raw` trait calls is not yet in place. A third-party render object panicking inside one of these methods still aborts the process today.

**Shape:** wrap every trait call site in `pipeline/owner.rs` with `catch_unwind`:

```rust
match std::panic::catch_unwind(AssertUnwindSafe(|| {
    render_object.perform_layout_raw(constraints)
})) {
    Ok(geometry) => /* normal path */,
    Err(_) => return Err(RenderError::poisoned(render_object.debug_name(), "layout")),
}
```

Requires:
- `RenderObject<P>::debug_name(&self) -> &'static str` accessor (already in Mythos Step 11 design; add as a trait method with `type_name::<Self>()` default).
- `flush_layout` / `flush_paint` / `flush_semantics` signatures change from `()` to `RenderResult<()>` so the error can propagate.
- `RendererBinding::draw_frame` updates to handle the new return type (drop the frame, log, retry next tick).

**Why deferred:** flips the return type of the entire flush_* surface, rippling into flui-app's draw loop. Land alongside Mythos Step 7 finalization (per-phase method redistribution) which already touches the same call shapes.

**Dependencies:** debug_name method on RenderObject; or use TypeId-based name lookup as a fallback.

### Per-phase method redistribution (Mythos Step 7 finalization)

**Files:** [`src/pipeline/owner.rs`](src/pipeline/owner.rs), [`crates/flui-app/src/bindings/renderer_binding.rs`](../../crates/flui-app/src/bindings/renderer_binding.rs).

**Goal:** today the typestate scaffold from Mythos Step 1 + the `run_frame` consuming convenience from Mythos Step 7 coexist with the legacy `flush_*` methods on `PipelineOwner<Idle>`. Full migration moves `flush_layout` to `impl PipelineOwner<Layout>` (renamed `run_layout`), `flush_paint` to `<PaintPhase>`, etc., so the compiler refuses to run paint on an owner that hasn't been transitioned to `<PaintPhase>` first.

**Shape:** the legacy `RendererBinding::draw_frame` in `flui-app` calls `owner.write().flush_*()` four times. Migration replaces those four calls with one `let owner = std::mem::replace(...); let (owner, layer_tree) = owner.run_frame(); *guard = owner;` pattern. The `flush_*` methods are then removed (or marked `#[deprecated]`).

**Why deferred:** the migration is mechanical but touches the binding singleton's locking pattern + the singleton's draw-loop call shape. Bundling that with the typestate-skeleton commits would have expanded the diff into `flui-app` significantly. Land as a follow-up commit when the binding is being touched for other reasons (e.g. when the Step 11 extension-trait split finally lands and ripples through the impls anyway).

**Dependencies:** none beyond the Mythos Step 1 + Step 7 baseline.

### Extension-trait split of `RenderObject<P>` (deferred from Mythos Step 11)

**Files:** [`src/traits/render_object.rs`](src/traits/render_object.rs).

**Goal:** the Mythos design (Section 3) prescribed splitting capability-specific methods on `RenderObject<P>` (e.g. `describe_semantics_configuration`, `paint_alpha`, `paint_transform`, `reassemble`) into extension traits (`SemanticsCapability`, `PaintEffectsCapability`, `HotReloadCapability`). Today the methods carry sensible default impls and the trait surface is already lean (17 methods, 6 required); the cost-benefit of moving them out into extension traits — 7 impl-file rewrites in `src/objects/` plus a `flui-view` boundary update — does not pay back its churn at the current scale.

**Shape:** when a second render object actually overrides one of these methods (e.g. a future `RenderParagraph` overriding `describe_semantics_configuration`), revisit. Move that one method to a capability trait at that point. Premature now.

**Dependencies:** none. The user-decision to keep the `Send + Sync + 'static` bound (made during the Mythos planning dialogue) means the broader trait-bound-relaxation half of Step 11 is also off the table; only the extension-trait split remains and is deferred per above.

### Audit pre-existing clippy issues in `src/objects/flex.rs`

**File:** [`src/objects/flex.rs`](src/objects/flex.rs) lines 261, 280, 321, 322, 367.

**Goal:** four `clippy::pedantic` warnings exist (redundant range loop, collapsible `if`, unnecessary `if let`, range-loop-as-index). Pre-existing per git log (not introduced by U2). Reach `cargo clippy --workspace -- -D warnings` clean. Mechanical fixes, no behaviour change.

**Dependencies:** none.

### Migrate `docs/` companion architecture docs onto template-adjacent shape

**File:** [`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md), [`docs/LAYOUT_SYSTEM.md`](docs/LAYOUT_SYSTEM.md), [`docs/PAINT_SYSTEM.md`](docs/PAINT_SYSTEM.md), [`docs/HIT_TEST_SYSTEM.md`](docs/HIT_TEST_SYSTEM.md), [`docs/ROADMAP.md`](docs/ROADMAP.md).

**Goal:** these deep-dives stay as companion documents (not under the per-crate template directly), but a header line in each — `> See also [crates/flui-rendering/ARCHITECTURE.md](../ARCHITECTURE.md) for the per-crate template instance.` — would link them into the methodology index. Trivial doc tidy.

**Dependencies:** none.

---

## Notes

- **R12 lint promotion path is symbolic for Trigger 1.** [`docs/PORT.md`](../../docs/PORT.md) reactive-lint-promotion rule names `[workspace.lints.clippy]` as the first-promotion mechanism. The clippy lint vocabulary cannot today express "field of type `RwLock<X>` where `X` is a trait object locked in method `foo`". The grep regression in [`scripts/port-check.sh`](../../scripts/port-check.sh) is the durable enforcement layer; the clippy-promotion column waits for ecosystem expressivity (`dylint` plugin or a future clippy feature).
- **CLAUDE.md drift.** `CLAUDE.md` still lists `flui-rendering`, `flui-view`, `flui-app`, `flui-hot-reload` as disabled. They are active. `AGENTS.md` and [`docs/crates.md`](../../docs/crates.md) are the authoritative state. Drift fix deferred to a separate housekeeping PR per the plan's `Deferred to Follow-Up Work`.
