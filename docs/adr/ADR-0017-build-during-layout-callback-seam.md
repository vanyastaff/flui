# ADR-0017: Build-during-layout is a binding-driven layout↔build fixpoint, not Flutter's mid-pass `invokeLayoutCallback`

- **Status:** Accepted
- **Date:** 2026-07-08
- **Revised:** 2026-09-26 (a LayoutCallbackScope spike; the decision stands)
- **Related:** ADR-0003 (lazy slivers use the same fixpoint)

## Context

`LayoutBuilder` must call its builder with the **real incoming `BoxConstraints`**, and the child it
returns must be reconciled and laid out before the frame paints.

**What Flutter does.** `RenderConstrainedLayoutBuilder.performLayout` runs
`rebuildIfNecessary()` — if the element is dirty or the constraints differ from
`_previousConstraints`, it invokes the builder inside `owner!.buildScope(...)` wrapped by
`invokeLayoutCallback`, building the child element subtree and attaching render objects in the
middle of the layout walk — then `child.layout(constraints, parentUsesSize: true)` and
`size = constraints.constrain(child.size)` (or `constraints.biggest` with no child). The mid-pass
mutation is safe only by convention: a debug flag and an assert, no aliasing guarantee.
(Reference: `widgets/layout_builder.dart`, `rendering/object.dart`'s
`RenderObjectWithLayoutCallbackMixin`.)

**Why FLUI cannot copy it.** Two structural facts:

1. **The layout walk holds `&mut RenderTree` for its whole duration** (`SubtreeArena::from_tree` in
   `pipeline/owner/layout.rs`). Structural mutation mid-walk is an aliasing violation, which is why
   the arena exposes sinks (`take_pending_child_requests`, `take_pending_retain_bands`) drained
   after the walk.
2. **The `PipelineOwner` is not reachable through its lock during a frame.** The binding takes the
   owner out of its `Arc<RwLock<…>>` under the write guard for the frame. Mounting render objects
   needs the owner; a build re-entered from inside `run_layout` would deadlock on the guard or
   mutate an empty defaulted owner.

`BuildOwner::build_scope` also asserts `!self.building`, so a mid-pass build would trip an existing
reentrancy guard.

A one-frame-late `LayoutBuilder` (build after the frame, lay out next frame) is not acceptable: a
responsive layout would visibly flash the wrong branch, and `builder(constraints)` would lie about
when it ran.

## Decision

**Move the reentrancy boundary out of the layout walk and into the gap between layout passes, and
let the binding drive a bounded layout↔build fixpoint.** `PipelineOwner<Layout>::run_layout` can be
re-driven while the owner stays in the `Layout` phase, so the binding — the one place holding
`&mut ElementTree`, `&mut BuildOwner` and the shared pipeline lock — interleaves layout passes with
builder servicing, then runs the rest of the frame on a settled tree.

Observable semantics are Flutter's: the builder receives the real incoming constraints, and its
child is reconciled, laid out and painted **in the same frame**. The difference is internal — a
bounded number of layout passes over dirty subtrees instead of one pass with a re-entrant callback.

### 1. No `invoke_layout_callback`: the render object publishes, the binding services

`RenderLayoutBuilder` (`flui-objects`) is a single-child box holding an `Arc<LayoutConstraintsCell>`.
`perform_layout` calls `cell.publish(constraints)`, lays the existing child out with those
constraints, and sizes to `constraints.constrain(child_size)` (or `constraints.biggest()` with no
child). It performs no build and touches no tree. `publish` raises `needs_build` iff the constraints
differ from the last committed ones — the analogue of Flutter's
`_previousConstraints == constraints && !_needsBuild` skip. Because the cell is shared with the
element, no `flui-rendering` change is needed.

### 2. The boundary: between `run_layout` calls, with the owner threaded by lock

`BuildOwner::service_layout_builders` runs between passes, where the arena is dropped, the pipeline
write lock is **released**, and `building == false` — the only point where all three hazards are
structurally absent.

The owner is threaded **by lock, not by value**. `build_scope` mounts render objects, and an element
reaches the `PipelineOwner` through the `Arc<RwLock<…>>` it carries, not through any value the
binding holds. Building while the frame still holds the write guard would self-deadlock
(`parking_lot`'s `RwLock` is not reentrant) the moment a builder mounts a child. So each pass, in
`run_frame_with_layout_builders`:

```rust
drive_fixpoint(|| {
    {   // layout under the write lock…
        let mut guard = pipeline.write();
        let mut layout = std::mem::take(&mut *guard).into_layout();
        let result = layout.run_layout();
        *guard = layout.into_idle();   // restored on the error path too
        result?;
    }   // …guard dropped here
    Ok(owner.service_layout_builders(tree, pipeline))  // builds with the lock free
})
```

`service_layout_builders` takes `&Arc<RwLock<PipelineOwner>>`, read-locks briefly for the liveness
scan and write-locks briefly for the `mark_needs_layout` batch. A
`debug_assert!(pipeline.try_read().is_some())` at its head turns a regression into a loud failure
instead of a hang.

### 3. Build never runs during layout

`build_scope`'s `assert!(!self.building)` stays and becomes proof the boundary holds. For each
registered builder whose cell has `needs_build`, `service_layout_builders` marks the element dirty,
runs one `build_scope` for the batch (the builder reads `cell.constraints()`; the returned view is
reconciled through the ordinary `update` path), commits the cell, marks the render object
`needs_layout`, finalizes the tree, and returns whether anything rebuilt. Registration happens in
the element's `on_mount` (the only hook with `&mut ElementOwner`); deregistration in `on_unmount`,
before the render object is disposed.

### 4. Dirty-flag interaction

- `needs_build` is **edge-triggered** on a constraints change. A pass with unchanged constraints
  publishes nothing, the service returns `false`, and the loop exits — "same constraints ⇒ no
  rebuild" is structural.
- `RenderLayoutBuilder` is a relayout boundary, so the re-layout after a rebuild is scoped to its
  subtree.
- An element dirtied by ordinary means rebuilds in the frame's leading `build_scope`; `needs_build`
  is additive with the normal dirty path, not a replacement.

### 5. How constraints reach the builder

Through the shared `LayoutConstraintsCell`. The builder is
`Arc<dyn Fn(&dyn BuildContext, BoxConstraints) -> BoxedView + Send + Sync>`. The cell's `Mutex` is
private; no lock appears in the public API.

### 6. Same-frame child replacement

The builder's output is reconciled by the standard element `update` path: same view type at the
same slot preserves the child element and its state; a different type remounts. `mark_needs_layout`
guarantees the replacement is laid out by the next pass, before compositing.

### 7. Guards

- **Convergence bound.** The loop runs at most `MAX_LAYOUT_BUILD_PASSES` (10). Exceeding it is a
  `BUG:` panic in debug ("a builder's output is changing its own incoming constraints") and a single
  `tracing::error!` plus break in release, painting the last settled tree. It is reachable only when
  a builder's child changes the constraints the builder itself receives; Flutter asserts on the same
  class.
- **Nested `LayoutBuilder`s converge**, one extra pass per level, so the bound is a pass count, not a
  per-builder count.
- **Registry lifetime.** Deregistration precedes render-object disposal so the service never marks a
  dead `RenderId`.

### 8. Lazy slivers share the loop, and an unsettled band evicts before paint

Lazy-sliver child requests (ADR-0003) are serviced inside the same fixpoint, beside
`service_layout_builders`, so a fresh band is built, laid out and painted in the frame that asked
for it. The post-`run_frame` service both frame paths still run is the safety net for a frame that
hit a bound.

The band has its own budget, `MAX_LAZY_BAND_PASSES`: a band that keeps growing is content, not a
bug, and must not trip the fixpoint's `BUG:` bound. When that budget trips, the frame stops
servicing builds and `BuildOwner::service_child_requests_evict_only` applies the last pass's retain
bands with no builds — the build requests stay queued for the post-frame service, which builds them
for the next frame — and marks the touched slivers, so the final `run_frame`'s layout positions only
what the band kept. Without this, residents the last band no longer covered would stay attached at
stale offsets and be painted, hit-tested and exposed to semantics for one frame. The band walk also
lays out any already-attached child that a measurement-driven re-query pulls into the band, so no
in-band child is positioned from an extent the pass did not measure.

Paint and hit-test additionally skip any child a pass did not place (the `placed_generation` stamp,
flui-rendering `ARCHITECTURE.md`), which makes the stale-offset hazard structural for every
multi-child object; the eviction remains what keeps out-of-band residents out of the tree.

The R1/R2 invalidation rules in "Revisited" below are a candidate amendment to this section.

## Parity findings

Checked against Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`
(`widgets/layout_builder.dart`, `rendering/object.dart`, `rendering/box.dart`,
`test/widgets/layout_builder_test.dart`).

| # | Question | Flutter | FLUI | Verdict |
|---|---|---|---|---|
| 1 | Skip condition | `_needsBuild \|\| (layoutInfo != _previousLayoutInfo)` gates the rebuild | `publish` raises `needs_build` only on changed constraints; a widget update dirties the element | Match |
| 2 | First pass | The child is built before `performLayout` lays it out | The first pass has no child and sizes to `constraints.biggest()`; the fixpoint then builds and re-lays out | Internal difference, unobservable — the intermediate size never reaches paint. Locked by `layout_builder_loose_constraints_size_follows_the_child` |
| 3 | `performLayout` | `child.layout(constraints, parentUsesSize: true)`; `constrain(child.size)`; else `biggest` | Identical | Match |
| 4 | Intrinsics | Asserts (throws) outside `debugCheckingIntrinsics`, then returns `0.0` | Returns `0.0`, logs `tracing::error!` | Divergence: an intrinsic query has no error channel, panics are reserved for internal invariants, and FLUI has no `debugCheckingIntrinsics` to tell Flutter's own probe from real use |
| 5 | Dry layout | Asserts, returns `Size.zero` | Returns `Size::ZERO`, logs, never publishes | Match on the value; same throw-vs-log divergence. Must not answer from the current child, which was built for other constraints |
| 6 | Builder update | `updateShouldRebuild` → `_needsBuild = true` | Reconcile dirties and schedules the element; the new closure runs | Match |
| 7 | Builder error | Substitutes `ErrorWidget.builder`; still commits | Substitutes the error view; the cell still commits, so the fixpoint converges | Match |

**Divergence: double builder invocation in a rebuild-and-resize frame.** Flutter defers all building
to layout, so a frame where the widget is rebuilt *and* its constraints change invokes the builder
once. FLUI rebuilds through the ordinary dirty path, so such a frame invokes it twice — once in the
leading `build_scope` with the last-published constraints, again in the fixpoint with the fresh
ones. The final child is the same; the extra call is wasted work, not a wrong result. The builder
must be a pure function of `(ctx, constraints)`, which Flutter also requires. Pinned by
`layout_builder_constraint_change_rebuilds_in_the_same_frame`
(`crates/flui-widgets/tests/layout_builder.rs`). Closing it would need `build_into_views` to defer to
the fixpoint and retain the previous child view meanwhile — element state for no observable gain.

## Revisited: a LayoutCallbackScope spike (2026-09-26)

The decision above stands. This section records a spike of the rejected mid-pass alternative and
what a superseding record would need.

**What was built.** Branch `spike/layout-callback`, commits `f7e89e8f6` and `92a47723e`
(24 files, +1116/−63), not merged:

- `Slab<Box<RenderNode>>` in `storage/tree.rs`, so a node's address does not move when the tree
  grows mid-walk.
- `LayoutChildBuilder` and `LayoutChildBuild`, plus
  `PipelineOwner<Layout>::run_layout_with_child_builder`.
- `LayoutCallbackScope`, with a side map in `subtree_arena.rs` that holds the only new `unsafe`.
- `ChildLayout::Built`, used by the fixed-extent list, the variable list and the grid.
- `BuildOwner::build_child_in_layout`.
- 5 `PipelineCell` sites routed through the scope.

**Results.** `layout_passes` from `FrameReport`, reproduced by the reviewer. Configurations:
A = callback on with both rules, D = callback on with both rules below off, B = callback off
with the rules, C = `main`.

| Scenario | A | D | B | C |
|---|---|---|---|---|
| Fixed extent: mount / scroll / jump | 1/1/1 | 1/2/2 | 2/2/2 | 2/2/2 |
| Fixed extent: 30 small scrolls | 30 | 41 | 40 | 42 |
| Variable extent: mount / scroll / jump | 1/1/1 | 1/2/2 | 2/2/2 | 2/2/2 |
| Variable extent: 30 small scrolls | 30 | 41 | 41 | 52 |
| Heterogeneous: mount / scroll / jump | 1/1/1 | 1/2/2 | 2/1/3 | 2/2/3 |
| Heterogeneous: 30 small scrolls | 30 | 36 | 34 | 40 |

- In the over-estimated-extent rows only the mount figure is valid (1 against 3): after the jump
  the viewport sits at the content's end, and the oracle skips `pixels != 0`.
- `list_10k_scroll_one_screen`: A gives 1 pass, 1 layout root and 129 nodes laid out; `main`
  gives 2, 128 and 132. `perf.rs` does not gate these values.
- Builds, frames and paints are identical across configurations, and idle frames are 0.

The callback alone reaches one pass only at mount. Scroll and jump need the callback plus the two
invalidation rules.

**Why §3 is not superseded.**

1. **No double borrow is not structural.** 5 of the 55 `.with_mut(` sites in
   `crates/flui-view/src` (at the spike's head) are routed. `build_child_in_layout` reaches
   `build_scope_impl`, which drains the global dirty heap, so any unrouted path can panic inside
   layout. A `LayoutBuilder` row hit `BUG: PipelineCell::with_mut called reentrantly`.
2. **`LayoutBuilder` rows regress against `main`.**

   | Frame | `main` | Callback on |
   |---|---|---|
   | Mount | 5 passes, 355 nodes laid out | 3 passes, 60,010 nodes laid out |
   | Jump 5000 rows | 9 passes, 831 nodes, 2,144 builds | 5 passes, 30,170 nodes, 25,270 builds |

   Both leave row 5000 unattached. This is the stand-in hazard of
   [ADR-0054](ADR-0054-the-viewport-commits-one-result.md).
3. **The §8 budget is lost.** `MAX_LAZY_BAND_PASSES`
   (`crates/flui-view/src/owner/layout_builder.rs:74`) no longer bounds in-callback work, and
   `lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame`
   (`crates/flui-widgets/tests/lazy_list.rs:899`) fails. The suite ran 4964 of 4965 passing at the
   spike's head.
4. **The scope is unsound from safe code.** `with_created_node_mut(a, |n| scope.adopt(a, b))`, or
   a nested `with_created_node_mut(a, …)`, aliases `&mut`. The scope has no unit tests and was not
   run under miri.
5. **Also open.**
   - The render-children reorder is skipped inside the callback. Hypothesis: paint and semantics
     order is wrong until the next drain.
   - There is no backward-scroll scenario.
   - The spike's `env::var_os` reads in `mark_needs_layout` taint any timing comparison. The
     boxed-node bench showed no visible cost but was not re-run.

**Conditions for a follow-up spike.** A record superseding §3 needs a spike that shows all four:

1. `LayoutBuilder` on the same callback (the box-protocol side), with its rows at or below
   `main`'s work and row 5000 attached.
2. The in-callback build drains only the new child's subtree, and element-side render access
   goes through a scope or attach handle as a type, so no `PipelineCell` access is reachable
   inside a callback.
3. A replacement for the band pass budget: a per-frame bound on in-callback builds.
4. Miri clean on `subtree_arena`, with the `with_created_node_mut`/`adopt` aliasing closed and
   unit tests for use after return and for the adopt restrictions.

Stable node storage (boxed or chunked) is a prerequisite the spike already showed.

**A separable candidate change.** Two invalidation rules, measured alone as B against C:

- **R1.** A service pass that only evicted children the band-emitting pass did not place does not
  re-dirty the sliver's layout; it marks paint, compositing bits and semantics instead.
- **R2.** A layout mark on a child its sliver parent did not place in its last pass stops there.
  The spike checks this in its scheduler (`scheduler.rs:240` on the branch) through
  `as_sliver()`, so it applies to every sliver parent.

Without the callback they bring variable-extent small scrolls from 52 to 41 passes,
heterogeneous small scrolls from 40 to 34, and the heterogeneous scroll from 2 to 1.

- R2 diverges from Flutter's `markNeedsLayout`, which always walks to the relayout boundary. R1
  has no Flutter counterpart, because Flutter collects garbage inside layout.
- The change lands on its own. It amends §8 explicitly and keeps the budget-trip path,
  `service_child_requests_evict_only` (`crates/flui-view/src/owner/build_owner.rs:2288`),
  marking layout, because that path evicts children the last pass did place.
- R2 becomes a per-render-object opt-in, not an `as_sliver()` check.
- It needs `## Mapping decisions` entries in
  [`crates/flui-rendering/ARCHITECTURE.md`](../../crates/flui-rendering/ARCHITECTURE.md#mapping-decisions)
  for R2 and [`crates/flui-view/ARCHITECTURE.md`](../../crates/flui-view/ARCHITECTURE.md#mapping-decisions)
  for R1, tests that fail without each rule, and an updated
  `crates/flui-widgets/perf/baseline.toml`.

Until then R1 and R2 are candidates only.

## Consequences

- Flutter-observable semantics with no `flui-rendering` change: no new `LayoutContextApi`
  capability, no arena surgery, layout hot path untouched. The unsafe mid-pass mutation is never
  introduced.
- Up to a bounded number of layout passes per frame when constraints change; steady state is one.
- `HeadlessBinding::pump_frame` and `AppBinding::draw_frame` must run the loop identically, so it
  lives in one shared helper.
- `BuildOwner` carries a second registry.

## Rejected alternatives

- **True mid-pass `invokeLayoutCallback`.** Needs the `SubtreeArena` to allocate and splice nodes
  while it holds `&mut RenderTree`, and the element layer to reach the `PipelineOwner` without the
  frame driver's non-reentrant lock — a render-machine redesign whose only gain is pass count. It
  could be adopted later without changing the public `LayoutBuilder` API. Built and measured in
  the 2026-09-26 spike ("Revisited"): one pass for plain lazy rows, not yet sound or bounded.
- **Ship the one-frame-late version** (build after the frame). A public semantic we would have to
  unwind: `builder` would run a frame after layout, and a responsive UI would flash its previous
  branch.
