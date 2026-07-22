# ADR-0017: Build-during-layout is a **binding-driven layout↔build fixpoint**, not Flutter's mid-pass `invokeLayoutCallback` — the reentrancy boundary sits *between* layout passes, where no borrow is held

*Flutter builds a `LayoutBuilder`'s child **inside** `performLayout` via `invokeLayoutCallback`, mutating the element and render trees mid-walk under nothing but a debug flag. FLUI cannot: during `run_layout` the `SubtreeArena` holds `&mut RenderTree` for the whole recursive walk, and building while the pipeline write-lock is held would self-deadlock when mounting render objects. Both are compile-time/structural, not stylistic. The Rust-native answer preserves Flutter's **observable** semantics — the builder sees the real incoming constraints and its child is laid out and painted **in the same frame** — by moving the reentrancy boundary to the one place where neither hazard is live: **between** `run_layout` passes. Each pass briefly takes and restores the `PipelineOwner` under its lock, drops the guard, then lets the binding service builders while it holds `&mut ElementTree` and `&mut BuildOwner`. This requires **no change to `flui-rendering`**.*

---

- **Status:** Accepted — **U1–U3 landed 2026-07-08**, with one design correction found by implementation (see *Correction: the owner is threaded by lock, not by value*). **U4 (parity cross-check + public export) landed 2026-07-09**; `LayoutBuilder` is public.
- **Date:** 2026-07-08
- **Deciders:** chief-architect; consult rendering owner (layout-pass re-drive + relayout-boundary scoping), view owner (`BuildOwner` registry + `build_scope` reentrancy guard), qa-lead (the convergence + no-spurious-rebuild harness)
- **Relates to:** completes **ADR-0003 Decision 2** ("mid-pass-capable contract from day one"; its v1 backend is the next-frame deferred-mutation queue). Sibling in spirit to ADR-0011/0012/0013: close a gap by **reusing existing machinery** rather than inventing a parallel channel.
- **Unblocked:** `LayoutBuilder` (Business.1 / tracker B1.1) shipped publicly in U4. The same fixpoint may also unblock the eventual exact-semantics upgrade of lazy `SliverList`'s documented blank-first-frame divergence.
- **Gate:** ARCH-GATE (this doc) → then DEV-GATE per slice.

---

## Context

`LayoutBuilder` is the canonical build-during-layout widget: its builder must be
called with the **real incoming `BoxConstraints`**, and the child it returns must
be reconciled and laid out before the frame paints.

### What Flutter does

`LayoutBuilder extends ConstrainedLayoutBuilder<BoxConstraints>`. Its render
object (`RenderConstrainedLayoutBuilder` mixin, over `_RenderLayoutBuilder`)
does, in `performLayout`:

1. `rebuildIfNecessary()` — if the element is dirty **or** the incoming
   constraints differ from `_previousConstraints`, invoke the layout callback
   inside `owner!.buildScope(...)` wrapped by `invokeLayoutCallback`. This
   **builds the child element subtree in the middle of the layout walk**,
   creating and attaching render objects as it goes.
2. `child.layout(constraints, parentUsesSize: true)`.
3. `size = constraints.constrain(child.size)` — or `constraints.biggest` when
   there is no child.

The skip condition (`_previousConstraints == constraints && !_needsBuild`) is
what makes an unchanged-constraints layout pass *not* re-invoke the builder.

Flutter's mid-pass mutation is safe only by convention: `invokeLayoutCallback`
sets `_debugDoingThisLayoutWithCallback` and asserts. There is no aliasing
guarantee. ADR-0003 already recorded this: *"Flutter `invokeLayoutCallback`
performs re-entrant build during layout, but it is **unsafe re-entrancy** —
guarded only by a debug flag, with no compile-time aliasing guarantee."*

> **Reference verified (U4, 2026-07-09).** Cross-checked against `.flutter/`
> (Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`):
> `packages/flutter/lib/src/widgets/layout_builder.dart`,
> `packages/flutter/lib/src/rendering/object.dart`
> (`RenderObjectWithLayoutCallbackMixin`),
> `packages/flutter/lib/src/rendering/box.dart` (`debugCannotComputeDryLayout`),
> and `packages/flutter/test/widgets/layout_builder_test.dart`. Steps 1–3 and the
> skip condition above are confirmed verbatim. Findings and the two documented
> divergences are recorded under **Parity findings (U4)** below.

### Why FLUI cannot copy it

Two independent, load-bearing facts — both verified in-tree, not assumed:

1. **The layout walk holds `&mut RenderTree` for its entire duration.**
   `PipelineOwner::layout_node_with_children` builds a
   `SubtreeArena::from_tree(&mut self.render_tree, id, …)` and only drops it
   *after* the recursive walk returns
   (`crates/flui-rendering/src/pipeline/owner/layout.rs:600-665`). Structural
   mutation mid-walk is therefore an aliasing violation, which is exactly why
   the arena exposes **sinks** (`take_pending_removes`, `take_pending_builds`,
   `take_pending_child_requests`, `take_pending_retain_bands`) drained after the
   walk, rather than letting a render object insert a node in place. Note that
   `pending_builds` carries an **already-constructed** render object — it
   presupposes the element build already happened.

2. **The `PipelineOwner` is not reachable through its `Arc<RwLock<…>>` during a
   frame.** The binding does
   `let mut guard = pipeline.write(); let owner = std::mem::take(&mut *guard); let (owner, result) = owner.run_frame();`
   (`crates/flui-binding/src/lib.rs:390-398`; `AppBinding::draw_frame` mirrors
   it). For the whole frame the write guard is held **and** the real owner has
   been moved out by value, leaving `Default::default()` behind. Element mount
   needs the `PipelineOwner` to insert render objects; a build re-entered from
   inside `run_layout` would either deadlock on the guard or silently mutate an
   empty defaulted owner.

A third, softer fact: `BuildOwner::build_scope` already asserts
`!self.building` (`build_owner.rs:426`), so a mid-pass build would trip an
existing reentrancy guard.

### The precedent already in the tree

Lazy `SliverList` is the *other* build-during-layout widget, and it resolved the
same constraint by **deferring**: layout emits `(RenderId, logical_index)`
requests into a pipeline sink; `BuildOwner::service_child_requests` — called
after `run_frame` by both `HeadlessBinding::pump_frame` and
`AppBinding::draw_frame` — builds the missing children, runs a second
`build_scope`, and marks the sliver `needs_layout`. Its cost is a **documented
divergence**: children land one frame late (blank first frame). `SliverList`
itself therefore lives in `flui-view`, co-located with its element;
`flui-widgets` only re-exports it.

**That divergence is acceptable for a scrolling list and unacceptable for
`LayoutBuilder`.** A one-frame-late `LayoutBuilder` is an observable public
semantic (a responsive layout visibly flashing the wrong branch, and
`builder(constraints)` lying about *when* it ran) that we would later have to
unwind. Hence this ADR.

---

## Decision

**Move the reentrancy boundary out of the layout walk and into the gap between
layout passes, and let the binding drive a bounded layout↔build fixpoint.**

The `PipelineOwner` typestate already exposes every transition publicly
(`into_layout` → `into_compositing` → `into_paint` → `into_semantics` →
`into_idle`, in `pipeline/owner/{construction,layout,compositing,paint}.rs`),
and `PipelineOwner<Layout>::run_layout(&mut self)` may be re-driven while the
owner stays in the `Layout` phase. `run_frame` is a convenience wrapper over
that sequence, not the only path. So the binding — the one place holding
`&mut ElementTree`, `&mut BuildOwner`, and the shared pipeline lock — can
interleave:

```text
loop {
    {
        guard = pipeline.write();
        owner = take(guard).into_layout();
        owner.run_layout()?;                          // arena borrow ends inside this call
        restore(guard, owner.into_idle());
    }                                                 // pipeline write-lock dropped here
    if !build_owner.service_layout_builders(
        &mut tree, &pipeline,
    ) { break; }                                      // returns false when nothing rebuilt
    // service_layout_builders marked the rebuilt nodes needs_layout,
    // so the next run_layout re-lays exactly those relayout-boundary subtrees.
}
pipeline.run_frame()?;                                // clean layout, then paint/composite
```

Observable semantics are Flutter's: the builder receives the real incoming
constraints, and its child is reconciled, laid out, and painted **in the same
frame**. The difference is purely internal — N bounded layout passes over the
dirty subtrees instead of one pass with a re-entrant callback.

### Where each requested piece lives

**1. The `invoke_layout_callback` equivalent.** There is none, deliberately.
Instead of a callback invoked *by* the render object *during* its own layout,
the render object **publishes** and the binding **services**:

- `RenderLayoutBuilder` (new, `flui-objects/src/layout/layout_builder.rs`) is a
  single-child box holding an `Arc<LayoutConstraintsCell>` handed to it at
  `create_render_object` time.
- In `perform_layout` it does: `cell.publish(constraints)`; then, if a child
  exists, `child.layout(constraints)` with `parent_uses_size`, and
  `size = constraints.constrain(child_size)`; else `size = constraints.biggest()`.
  It performs **no build** and touches no tree.
- `cell.publish` records the constraints and sets a `needs_build` flag **iff**
  `constraints != last_built_constraints`. This is the direct analogue of
  Flutter's `_previousConstraints == constraints && !_needsBuild` skip.

Because the cell is an `Arc` shared with the element, **no `flui-rendering`
change is required**: the constraints do not have to travel through a new
`LayoutContextApi` capability or a new pipeline sink. (`RenderEntry::layout`
already calls `state.set_constraints`, so a pipeline-side accessor is an
available alternative; the cell is preferred because it also carries the
`needs_build` edge, which the render state does not model.)

**2. The reentrancy boundary.** Between `run_layout` calls, inside
`service_layout_builders`, at binding scope. At that instant: the arena is
dropped (no `&mut RenderTree`), the pipeline write-lock is **released**, and
`BuildOwner::building == false`. All three hazards from *Why FLUI cannot copy
it* are structurally absent — the boundary is chosen precisely because it is the
only point where that is true.

> ### Correction: the owner is threaded by lock, not by value
>
> The first draft of this ADR originally passed the
> `PipelineOwner` **by value** through the fixpoint, on the reasoning that the
> binding holds it as an owned local. **Implementing U1 proved that wrong**, and
> the code follows the correction, not the sketch.
>
> `build_scope` mounts render objects, and an element reaches the
> `PipelineOwner` through the `Arc<RwLock<…>>` it carries from
> `set_pipeline_owner_any` — *not* through any value the binding holds. Building
> while the frame still holds the write guard is therefore a self-deadlock
> (`parking_lot`'s `RwLock` is not reentrant) the instant a builder mounts a
> child. It is invisible while the registry is empty, and fatal on the first
> real `LayoutBuilder` — precisely the "inert infrastructure that breaks on
> first use" failure this ADR exists to prevent.
>
> So each pass takes the owner out under the write lock, lays out, **restores it
> and drops the guard**, and only then services the builders:
>
> ```rust
> // BuildOwner::run_frame_with_layout_builders
> drive_fixpoint(|| {
>     {   // layout under the write lock…
>         let mut guard = pipeline.write();
>         let mut layout = std::mem::take(&mut *guard).into_layout();
>         let result = layout.run_layout();
>         *guard = layout.into_idle();   // restored on the error path too
>         result?;
>     }   // …guard dropped here
>     Ok(owner.service_layout_builders(tree, pipeline))  // builds with the lock free
> })
> ```
>
> This is the same discipline `service_child_requests` already follows (it runs
> *after* `run_frame`, with the guard dropped) — the existing lazy-sliver code
> was right and the ADR's sketch was wrong. `service_layout_builders` therefore
> takes `&Arc<RwLock<PipelineOwner>>`, briefly read-locks for the liveness scan,
> and briefly write-locks for the `mark_needs_layout` batch. A
> `debug_assert!(pipeline.try_read().is_some())` at its head turns a future
> regression from a silent hang into a loud `BUG:` failure.

**3. How `BuildOwner`/build scope is allowed during layout.** It is not. Build
never runs during `run_layout`; it runs strictly between passes. The existing
`assert!(!self.building)` in `build_scope` therefore stays as-is and becomes a
*proof* that the boundary is respected, rather than an obstacle to route around.
`service_layout_builders` mirrors `service_child_requests`:

```rust
// flui-view/src/owner/build_owner.rs
pub(crate) layout_builder_registry: LayoutBuilderRegistry, // RenderId → (ElementId, Arc<LayoutConstraintsCell>)

/// Returns `true` iff at least one builder rebuilt (i.e. re-layout is needed).
pub fn service_layout_builders(
    &mut self, tree: &mut ElementTree, pipeline: &Arc<RwLock<PipelineOwner>>,
) -> bool;
```

Registration happens in the element's `on_mount`, which is the **only** lifecycle
hook receiving `&mut ElementOwner` — the same reason `SliverListAdaptorElement`
registers its `ChildManager` there, and the reason a plain `StatefulView` cannot
host this widget (`BuildContext::owner()` is a stub that unconditionally returns
`None`; `mark_needs_build` is borrow-scoped, so no `'static` rebuild handle
exists). Deregistration in `on_unmount`.

Body of `service_layout_builders`, per registered entry whose cell has
`needs_build`:

1. `tree.mark_needs_build(element_id)` + `schedule_build_for(element_id, depth)`.
2. Once for the batch: `self.build_scope(tree)` — the builder runs here, reading
   `cell.constraints()`, and the returned child view is reconciled through the
   ordinary element `update` path.
3. `cell.commit()` — `last_built_constraints = constraints`, clear `needs_build`.
4. `pipeline.mark_needs_layout(render_id)` for each rebuilt builder.
5. `self.finalize_tree(tree)` — unmount children the reconcile replaced.
6. Return whether step 1 fired for anyone.

**4. Dirty layout/build flag interaction.** Three rules keep the fixpoint from
degenerating:

- `needs_build` is edge-triggered on a constraints **change**, not level-triggered
  on "was laid out". A pass whose constraints equal `last_built_constraints`
  publishes nothing, `service_layout_builders` returns `false`, and the loop
  exits. This is what makes "same constraints ⇒ no spurious rebuild" a
  *structural* property rather than a test-only observation.
- Step 4's `mark_needs_layout` is what makes the *next* `run_layout` cheap:
  `RenderLayoutBuilder` declares itself a **relayout boundary**
  (`sized_by_parent == false`, and it does not read child geometry to compute
  the constraints it passes down), so the re-layout is scoped to its subtree,
  not the whole tree.
- An element marked dirty by ordinary means (`set_state` in an ancestor) already
  rebuilds in the frame's leading `build_scope`; the builder then re-runs with
  whatever constraints the subsequent `run_layout` publishes. `needs_build` is
  additive with, not a replacement for, the normal dirty path.

**5. How constraints reach the builder.** `Arc<LayoutConstraintsCell>`, written
by `RenderLayoutBuilder::perform_layout`, read by the element's `build`. Public
builder shape (object-safe, matching the crate's `Arc<dyn Fn…>` delegate idiom,
cf. `SliverChildBuilderDelegate`):

```rust
Arc<dyn Fn(&dyn BuildContext, BoxConstraints) -> BoxedView + Send + Sync>
```

`BoxConstraints` is `Copy`; the cell holds it behind a private `Mutex`, never
exposed as a lock in the public API (SP-6 / port-check trigger).

**6. Same-frame child replacement and layout.** The builder's output is a fresh
view each invocation, reconciled by the standard element `update` path in step 2:
same concrete view type at the same slot ⇒ the child element and its state are
**preserved**; a different type ⇒ remount. This is Flutter's behavior and needs
no special casing — but it *must* be tested, because "the builder returns a
different branch across a breakpoint" is the widget's entire purpose. Step 4's
`mark_needs_layout` then guarantees the replacement child is laid out by the
next `run_layout` iteration, before `into_compositing`.

**7. Panic / reentrancy guards.**

- **Convergence bound.** The loop runs at most `MAX_LAYOUT_BUILD_PASSES`
  (proposed: 10, matching the order of Flutter's own re-entrancy tolerance). On
  exceeding it: `debug_assert!`/panic with `"BUG: LayoutBuilder failed to
  converge after N passes — a builder's output is changing its own incoming
  constraints"` (per `docs/PANIC-POLICY.md`, an internal-invariant `expect`), and
  in release: `tracing::error!` once + break, painting the last settled tree.
  Non-convergence is reachable only when a builder's child changes the
  constraints the builder itself receives — e.g. a `LayoutBuilder` under a
  shrink-wrapping parent whose branch selection flips the parent's size. Flutter
  hits the same class of bug and asserts on it.
- **No build inside layout.** `build_scope`'s existing `assert!(!self.building)`
  plus a new `debug_assert!(!pipeline.is_in_phase(PhaseKind::Layout))` at the top
  of `service_layout_builders` — a walk-time regression then fails loudly instead
  of corrupting the arena.
- **Nested `LayoutBuilder`s** are legal and converge: the inner one's cell is
  published during the pass that lays out the outer one's freshly built child, so
  it simply needs one more iteration. The bound must therefore be a *pass* count,
  not a per-builder count.
- **Cell/registry lifetime.** `on_unmount` must deregister before the render
  object is disposed, or `service_layout_builders` will `mark_needs_layout` a
  dead `RenderId`. The `SliverListAdaptorBehavior::on_unmount` ordering
  (`sliver_adaptor.rs:403`, "the `service_child_requests` call hits a stale
  entry") is the cautionary precedent.

---

## Tests Required Before Public Export

U4 satisfied this gate before exporting `LayoutBuilder` from
`flui_widgets::prelude`. The public tests live in
`crates/flui-widgets/tests/layout_builder.rs` over the headless widget harness,
which now drives the same layout↔build fixpoint as production and reads back
`Size`/`Offset`; lower-level fixpoint tests assert pass-count behavior.

1. **First build receives real incoming constraints.** Mount under a parent
   imposing a known non-degenerate `BoxConstraints`; assert the builder observed
   exactly those. Must fail against a placeholder that hands over
   `BoxConstraints::UNCONSTRAINED` or `::default()` — this is the regression that
   catches a reprise of `bb58a8fa`.
2. **Same-frame settle.** After a single `tick`/`pump_frame`, the child is
   present, laid out, and painted. Explicitly asserts *no* second frame is
   needed — the property that distinguishes this design from the `SliverList`
   divergence.
3. **Constraint change re-invokes the builder with the new constraints,** and the
   resulting render tree reflects the new branch (assert a computed size that is
   wrong under the old branch).
4. **Unchanged constraints do not re-invoke the builder.** A call-counter in the
   builder; re-pump frames; assert the count is stable. Guards the edge-triggered
   `needs_build`.
5. **Builder output determines layout.** Size/offset assertions derived from
   Flutter's documented algorithm (`constraints.constrain(child.size)`; no child
   ⇒ `constraints.biggest`), not from running the code first.
6. **Child element state is preserved** across a constraint-driven rebuild when
   the builder returns the same view type, and **remounts** when it returns a
   different one.
7. **Nested `LayoutBuilder`s converge** within the pass bound, both branches.
8. **Non-convergence trips the guard** (debug): a deliberately oscillating
   builder panics with the `BUG:` message rather than hanging or silently
   painting a stale tree.
9. **No reentrancy assertion fires:** `build_scope`'s `!building` and the new
   phase debug-assert hold across the whole suite.
10. **Parity cross-check:** with `.flutter/` present, re-derive the
    skip condition and `performLayout` body against
    `widgets/layout_builder.dart` + `RenderObjectWithLayoutCallbackMixin`, and
    record the result. Completed in U4; see **Parity findings (U4)**.

---

## Consequences

**Positive.** Exact Flutter-observable semantics with **zero `flui-rendering`
changes** — no new `LayoutContextApi` capability, no new pipeline sink, no arena
surgery, and the layout hot path is untouched. The unsafe mid-pass mutation
Flutter relies on is never introduced. The design generalizes: lazy `SliverList`
could later adopt the same fixpoint to retire its blank-first-frame divergence.

**Negative.** Up to N (bounded) layout passes per frame when a `LayoutBuilder`'s
constraints change; steady state is one pass, since `needs_build` is
edge-triggered. The binding grows a loop that both `HeadlessBinding::pump_frame`
and `AppBinding::draw_frame` must run identically — a divergence between those
two frame paths would be a silent correctness bug, so the loop belongs in one
shared helper, not copy-pasted. `BuildOwner` gains a second registry.

**Rejected: true mid-pass `invokeLayoutCallback`** (ADR-0003 Decision 2's
long-term option). It requires the `SubtreeArena` to support in-walk node
allocation and splicing while it holds `&mut RenderTree`, plus handing the
`PipelineOwner` to the element layer without going through a non-reentrant
pipeline lock held by the frame driver. That is a render-machine redesign whose
only benefit over this ADR is pass count, and it can be adopted later **without
changing the public `LayoutBuilder` API** — which is precisely the property
ADR-0003 asked its contract to preserve.

**Rejected: ship the one-frame-late version now** (the `SliverList` shape). It is
cheap and it is a public semantic we would have to unwind: callers would observe
`builder` running a frame after layout, and a responsive UI would flash its
previous branch. This was rejected before the public widget shipped and remains
rejected now that `LayoutBuilder` is public.

---

## Implementation sequence (for the follow-up PR)

Strictly ordered; each slice is independently gated.

- **U1 — `flui-view` + binding seam, no widget.** ✅ **Landed 2026-07-08.**
  `LayoutConstraintsCell` (in `flui-objects`, so U2's `RenderLayoutBuilder` can
  hold it without a dependency cycle), `layout_builder_registry` on `BuildOwner`,
  `service_layout_builders`, and the shared bounded fixpoint
  `BuildOwner::run_frame_with_layout_builders` wired into
  `HeadlessBinding::pump_frame` **and** `AppBinding::draw_frame` (the latter via
  `WidgetsBinding`). No public `LayoutBuilder` API or prelude export; the seam is
  inert until U3 registers into it.
  15 `flui-view` unit tests + one wiring test per binding — each verified
  red-then-green by reverting the code under test. Covers the cell's edge-trigger,
  registry register/unregister, stale-entry pruning (by liveness, not dirtiness),
  schedule→build→commit→`mark_needs_layout`, fixpoint termination and bound, the
  debug `BUG:` panic, and the deadlock tripwire.
- **U2 — `flui-objects::RenderLayoutBuilder`.** ✅ **Landed 2026-07-08.**
  Single-child `RenderBox` holding `Arc<LayoutConstraintsCell>`: `perform_layout`
  publishes the real incoming constraints, lays the existing child out under
  those same constraints, sizes to `constraints.constrain(child_size)`, and uses
  `constraints.biggest()` when childless. `compute_dry_layout` deliberately does
  not publish; intrinsics remain unimplemented at the trait default and are not
  claimed. Harness coverage includes the catalog row/guard plus tests for first
  publish, changed constraints, unchanged constraints after commit, child
  pass-through constraints, childless sizing, and dry-layout non-publication. No
  view, element, widget, prelude export, binding, or render-pipeline change.
- **U3 — `LayoutBuilder` view + element in `flui-view`.** ✅ **Landed
  2026-07-08.** Co-located with the element in a private, unexported module, per
  the `SliverList` precedent. The render object mints the
  `Arc<LayoutConstraintsCell>`; `on_mount` reads it back from
  `RenderLayoutBuilder`, registers `RenderId -> (ElementId, cell)`, and
  `on_unmount` deregisters before disposal. The first build creates no child
  until real constraints are published; it never hands
  `BoxConstraints::UNCONSTRAINED` to the builder. Tests cover first-frame
  same-frame layout, constraint-change same-frame rebuild, unchanged-constraint
  skip, lifecycle cleanup, reconciliation across a breakpoint, and nested
  convergence. Public export intentionally waited for U4.
- **U4 — parity cross-check against `.flutter/` (test 10), then** re-export from
  `flui-widgets` + `prelude`, and only then flip tracker B1.1. ✅ **Landed
  2026-07-09.** `LayoutBuilder` is public (`flui_view::element::LayoutBuilder`,
  re-exported as `flui_widgets::LayoutBuilder` and from `flui_widgets::prelude`);
  the element and behavior stay `pub(crate)`. Public tests live in
  `crates/flui-widgets/tests/layout_builder.rs`, transcribing Flutter's own
  oracles. Two divergences accepted and documented below.

---

## Parity findings (U4)

Sources: `.flutter/packages/flutter/lib/src/widgets/layout_builder.dart`,
`.../rendering/object.dart`, `.../rendering/box.dart`,
`.flutter/packages/flutter/test/widgets/layout_builder_test.dart`
(Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`).

| # | Question | Flutter | FLUI | Verdict |
|---|---|---|---|---|
| 1 | Skip condition | `_needsBuild \|\| (layoutInfo != _previousLayoutInfo)` gates `updateChildCallback` (`_rebuildWithConstraints`) | `LayoutConstraintsCell::publish` raises `needs_build` only when constraints differ from the committed value; a widget update dirties the element | **Match** |
| 2 | First pass | `_needsRebuild = true` initially, and `runLayoutCallback()` is the first statement of `performLayout`, so the child exists before it is laid out | The first layout pass has no child (builder runs *between* passes); the node sizes to `constraints.biggest()` for that pass, then the fixpoint builds and re-lays it out | **Internal difference, unobservable** — the intermediate size never survives into compositing/paint. Locked by `layout_builder_loose_constraints_size_follows_the_child` (final size is `constrain(child)`, not `biggest`) |
| 3 | `performLayout` | `child.layout(constraints, parentUsesSize: true)`; `size = constraints.constrain(child.size)`; else `constraints.biggest` | Identical | **Match** |
| 4 | Intrinsics | All four `computeMin/MaxIntrinsic*` `assert(_debugThrowIfNotCheckingIntrinsics())` — **throws** *"LayoutBuilder does not support returning intrinsic dimensions"* outside `debugCheckingIntrinsics` — then `return 0.0` | Returns `0.0`, logs `tracing::error!` | **Documented divergence:** FLUI does not throw. An intrinsic query returns `f32` with no error channel, `docs/PANIC-POLICY.md` reserves panics for internal invariants (not caller misuse), and FLUI has no `debugCheckingIntrinsics` flag to distinguish Flutter's own probe from real use |
| 5 | Dry layout | `computeDryLayout` asserts `debugCannotComputeDryLayout(...)` and returns `Size.zero`; `computeDryBaseline` returns `null` | Returns `Size::ZERO`, logs `tracing::error!`, never publishes | **Match on the value**, same throw-vs-log divergence as #4. FLUI must *not* answer from the currently-built child: it was built for other constraints, so the answer would be confidently wrong. (This corrected the U2 implementation.) |
| 6 | Builder update | `_LayoutBuilderElement.update` → `updateShouldRebuild(old)` (default `true`) → `_needsBuild = true; scheduleLayoutCallback()` | Reconcile updates the element, sees it dirty, schedules it; `build_into_views` calls the new closure | **Match** in observable behavior |
| 7 | Builder error | `_rebuildWithConstraints` catches and substitutes `ErrorWidget.builder`; `finally` still sets `_needsBuild = false` / `_previousLayoutInfo` | `build_or_recover` catches the panic and substitutes the error view; the cell still commits, so the fixpoint converges and the registry is intact | **Match** |

### Divergence: double builder invocation in a rebuild-and-resize frame

Flutter's `_LayoutBuilderElement` defers **all** building to layout
(`markNeedsBuild`/`performRebuild` only call `scheduleLayoutCallback()`), so a
frame in which the widget is rebuilt *and* its constraints change invokes the
builder **once**, with the fresh constraints.

FLUI's element rebuilds through the ordinary dirty path, so such a frame invokes
the builder **twice**: once in the leading `build_scope` with the last-published
constraints, then again in the layout↔build fixpoint with the fresh ones. Both
frames paint the same final child; the extra call is wasted work, not a wrong
result. The builder must therefore be a pure function of `(ctx, constraints)` —
which Flutter also requires. Pinned by
`layout_builder_constraint_change_rebuilds_in_the_same_frame` in
`crates/flui-widgets/tests/layout_builder.rs`, so any change to it is deliberate.

Closing this would mean teaching `build_into_views` to defer to the fixpoint when
its render object is already scheduled for re-layout, and retaining the previous
child view meanwhile. Not attempted: it adds element state for no observable
gain.
