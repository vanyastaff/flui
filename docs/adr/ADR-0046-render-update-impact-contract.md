# ADR-0046: Return explicit impact from render-view updates

*Render-view updates report the exact rendering phases invalidated by changed
configuration, and the rendering owner applies that report once.*

---

- **Status:** Accepted
- **Date:** 2026-08-10
- **Deciders:** @vanyastaff
- **Scope:** `RenderUpdateImpact`, canonical compositing/semantics mark methods,
  and `PipelineOwner::apply_render_update_impact` in `flui-rendering`; the
  `RenderView::update_render_object` contract and `RenderBehavior` update path
  in `flui-view`; every in-workspace `RenderView` implementation and affected
  render-object setter; the parent-data, render-child membership, and
  headless committed-output seams exposed by removing blanket invalidation
- **Issue:** [#534 — Make render-view updates return explicit invalidation
  impact](https://github.com/vanyastaff/flui/issues/534)

---

## Context

`RenderBehavior::on_update` currently mutates the retained render object and
then unconditionally marks it for layout and paint. This defeats the
change-detection already present in render-object setters. Custom layout
delegates compute `should_relayout`, custom painters compute `should_repaint`,
flow delegates distinguish relayout from repaint, and grid delegates compute
`should_relayout`; their results are then discarded at the View → Element →
Render boundary.

The result is behaviorally broader than Flutter and needlessly expensive. An
unchanged delegate still enters layout and paint. The ignored parity tests in
`custom_single_child_layout_test.rs` and
`custom_multi_child_layout_test.rs` pin the concrete divergence: a false
`shouldRelayout` must suppress layout, but FLUI's blanket mark makes that
impossible.

The Flutter reference was checked at commit
`f2d640ef01561447051f582059295a68ca2046ae`:

- `packages/flutter/lib/src/widgets/framework.dart` calls
  `RenderObjectWidget.updateRenderObject` from
  `RenderObjectElement._performRebuild` without adding blanket dirtiness.
- `packages/flutter/lib/src/rendering/custom_paint.dart` lets a painter swap
  independently call `markNeedsPaint` and `markNeedsSemanticsUpdate`.
- `packages/flutter/lib/src/rendering/flow.dart` lets the delegate choose
  layout, otherwise paint, and lets `clipBehavior` require paint and semantics.
- `packages/flutter/lib/src/rendering/sliver_grid.dart` marks layout only when
  the delegate type changes or `shouldRelayout` returns true.
- `packages/flutter/lib/src/rendering/proxy_box.dart` distinguishes clip-source
  changes (paint and semantics) from clip-behavior changes (paint only), and
  uses compositing-bit invalidation only when a compositing predicate changes.

FLUI cannot copy the setter-side owner calls literally: render objects are
stored by value in `flui-rendering`, while the declarative update is dispatched
through `flui-view`. The Rust-native equivalent is a small value returned from
the mutation, followed by one owner-side application after the mutable render
object borrow ends.

### Maintainer-grade pre-code verdict

**ACCEPTABLE.** `flui-rendering` owns rendering phases, dirty queues, and the
meaning of a render update, so it owns both `RenderUpdateImpact` and its
application on `PipelineOwner`, including the ancestor walks and queue policy
for each phase. `flui-view` owns typed update dispatch and therefore owns the
strict owner/RenderId/node/type prevalidation before invoking a widget updater.
Existing sibling primitives were reviewed: `DirtyKind` is a
transport request for exactly one dirty phase and cannot represent a composed
update; painter `Invalidation` and flow `DelegateChange` are narrower local
classifications, not a cross-crate owner contract. The update path is routine
and allocation-free but frame-sensitive, so an opaque copied byte with const
queries is the appropriate performance posture. A strict maintainer would
reject keeping the blanket mark, locating the type in `flui-view`, exposing raw
bits, or preserving the old trait with a shim. The workspace is in active
development, so the trait and all implementations will migrate atomically.

## Decision

### 1. `flui-rendering` owns one opaque, composable value

`flui-rendering` will define:

```rust
#[must_use = "render update impacts must be applied to the PipelineOwner"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderUpdateImpact(u8);
```

The field and individual bits are private. The only named base values are:

- `NONE`: no pipeline work;
- `PAINT`: paint;
- `LAYOUT`: layout and eventual paint;
- `COMPOSITING_BITS`: compositing-bits update and paint;
- `SEMANTICS`: semantics.

The private representation assigns layout to `1 << 0`, paint to `1 << 1`,
compositing to `1 << 2`, and semantics to `1 << 3`. `LAYOUT` and
`COMPOSITING_BITS` additionally include the paint bit.

The public const queries are `is_none`, `needs_layout`, `needs_paint`,
`needs_compositing_bits_update`, and `needs_semantics_update`. `union`,
`BitOr`, and `BitOrAssign` combine independent effects. There is no raw-bits
constructor, bits accessor, ordering, iterator, parser, or fallible result.
`Default` is exactly `NONE`.

`LAYOUT` carries the paint bit because a successful layout must eventually be
painted. `COMPOSITING_BITS` carries the paint bit because recomputing layer
requirements without repainting can leave the retained layer structure stale.
`SEMANTICS` remains independent and can be unioned with either visual impact.

### 2. `RenderView` returns the impact

`RenderView::update_render_object` will return `RenderUpdateImpact`. It has no
default implementation. Every implementation must mutate its retained render
object and return the union of the impacts reported by the setters it invoked.
An unchanged update returns `NONE`; the framework adds no blanket fallback.

Render-object setters that own equality or delegate-policy decisions become
the authoritative source of their impact. In particular:

- custom single- and multi-child layout delegates return `LAYOUT` only for a
  type change or true `should_relayout`;
- custom painter swaps independently union `PAINT` and `SEMANTICS` from
  `should_repaint` and `should_rebuild_semantics`;
- flow returns `LAYOUT`, otherwise `PAINT`, otherwise `NONE` for its delegate,
  and its clip behavior returns `PAINT | SEMANTICS`;
- grid delegates return `LAYOUT` only for a type change or true
  `should_relayout`;
- clip behavior returns `PAINT`, while a changed clip source returns
  `PAINT | SEMANTICS`;
- replacing a render object's owner-lane path target returns
  `PAINT | SEMANTICS`, while replacing a shader-mask target returns `PAINT`;
- physical-model and physical-shape clip-behavior changes return `PAINT`,
  while their shape, border-radius, or source changes return
  `PAINT | SEMANTICS`.

Callback-backed path clips use the exported opaque
`flui_objects::ClipSourceToken`. `fresh()` allocates a private
`Arc<PrivateMarker>` identity, cloning a widget preserves it, and separately
constructing a widget creates a distinct token. A caller may also supply an
existing token (`ClipPath::with_source`, issue #856) to declare "same clip,
new closure" — the case Rust cannot answer structurally because closures do
not compare. **Installing the callback is independent of the impact the token
reports**: a reused identity suppresses the invalidation and still replaces
the registered closure, since a rebuilt closure may capture different state.
Gating the install on the impact left the render object calling the previous
widget's closure, which is what `reusing_a_clip_identity_still_installs_the_new_clipper`
now pins. Equality is implemented only
through private `Arc::ptr_eq`; there is no raw constructor, pointer/integer
accessor, `Copy`, `Hash`, `Default`, process-global counter, or exhaustion
path. `RenderClipPath` and `RenderPhysicalShape` borrow the token in their
setters and clone it only on replacement. This makes callback identity explicit
without publishing an address protocol or adding ambient runtime state.
For value-configured material shapes, `PathClipConfiguration` persists the
effective rounded-rectangle or stadium configuration on
`RenderPhysicalShape`; mounted updates replace the owner-lane clipper only
when that value changes, matching `ShapeBorderClipper::shouldReclip` rather
than treating each rebuilt callback as a new source.

The same audit applies to all other in-workspace implementations. A layout,
paint, compositing, or semantics change must be named precisely; a test fixture
with no observable render change returns `NONE`.

### 3. `PipelineOwner` is the canonical application seam

`PipelineOwner<Phase>` will expose these phase-agnostic methods:

```rust
pub fn mark_needs_compositing_bits_update(&mut self, render_id: RenderId);
pub fn mark_needs_semantics(&mut self, render_id: RenderId);

pub fn apply_render_update_impact(
    &mut self,
    render_id: RenderId,
    impact: RenderUpdateImpact,
);
```

It applies each requested phase at most once, in this order:

1. layout through `mark_needs_layout`;
2. compositing bits through `mark_needs_compositing_bits_update`;
3. paint only when layout was not requested;
4. semantics through `mark_needs_semantics`.

The layout branch deliberately suppresses the immediate paint mark. Layout's
normal completion schedules the necessary paint, avoiding duplicate dirty
walks. A compositing-only request cannot occur through the named constants:
`COMPOSITING_BITS` also requests paint. Enabling semantics later already seeds
the root, so a semantics impact received while semantics are disabled does not
need to remain queued.

`mark_needs_compositing_bits_update` is the canonical Flutter-compatible
operation: it marks/walks the necessary ancestor chain and queues the
responsible root. The former raw
`add_node_needing_compositing_bits_update(id, depth)` operation was removed:
calling such an insertion with only the changed node bypasses that propagation
contract.

Its normative semantics match Flutter
`RenderObject.markNeedsCompositingBitsUpdate`
(`packages/flutter/lib/src/rendering/object.dart:3209-3227`): return if the
target is already dirty; otherwise mark it. If its parent is already dirty,
return and rely on the parent's existing responsible queue entry. Otherwise
walk to the parent exactly when
`(!target.was_repaint_boundary() || !target.is_repaint_boundary_flag())` and
the parent is not a current repaint boundary. If that condition is false, or
the target is parentless, queue the target at its live depth. The
implementation may be iterative, but these flag, stop, and queue decisions are
binding. They distinguish an established boundary (queue self), a newly
introduced or lost boundary (walk through non-boundary ancestors), and a
non-boundary child below a repaint boundary (queue the child without dirtying
the parent). All owner-side compositing requests, including
`DirtyKind::Compositing` replay, use this method.

`mark_needs_semantics` is the canonical semantics entry point. It owns the
semantics-enabled check, live depth lookup, and queue insertion. Raw semantics
queue insertion remains an owner-private implementation detail and does not
own those policies. The existing
`NEEDS_SEMANTICS` storage flag is not the current dirty truth: the semantics
pass consumes the queue and does not clear that flag. The migration must not
set it. Remove the legacy/future flag and its accessors in this migration if a
mechanical call-site audit proves that safe; otherwise leave them explicitly
unused by the canonical path.

Direct mark/apply calls with an absent/stale ID schedule no work, matching the
existing public dirty-mark APIs. The framework update path does not rely on
that tolerance: it validates the stronger active-element invariants first.

Parent-data updates use the same explicit-impact rule instead of retaining a
second blanket-layout path. `ParentDataView::apply_parent_data` is required and
returns `RenderUpdateImpact`. It mutates only configuration-owned fields on
the existing typed parent data, preserving layout-owned offsets and container
links. Initial absence installs `create_parent_data`; an existing value with
the wrong concrete type is a `BUG` invariant panic. The element-tree seam ends
the child-node borrow before applying the returned impact to the render parent.
`ElementBase::apply_parent_data_config` is the documented object-safe bridge.
The unified element forwards it through a second defaulted public
`ElementBehavior::apply_parent_data_config` hook because `ElementKind::Proxy`
erases ordinary proxy and typed parent-data elements behind the same trait
object. Behavior dispatch is the last type-safe point that still knows
`V::ParentData`; retaining both hooks keeps the traits unsealed and avoids an
unsafe or registry-based side channel. Both defaults return `NONE`.
Their public signatures accept `&mut dyn ParentData` and return
`RenderUpdateImpact`; the behavior hook additionally receives
`&ElementCore<V, A>` so `ParentDataBehavior` can read the typed view.

Render-child membership is likewise an owner operation, not a raw tree edit.
Adoption, detachment, and removal invalidate the surviving parent for layout,
compositing bits, and semantics; cross-parent adoption invalidates both
parents. A pure sibling reorder invalidates layout only. Reconciliation
compares the old and final child-ID sequences and reports a reorder only when
they differ, so a same-order update stays clean. These are Flutter's
`adoptChild`/`dropChild` and `ContainerRenderObjectMixin.move` side effects,
made explicit after the blanket render-view mark no longer hides them.

The headless presentation retains the last successfully committed layer tree
across an idle frame. Bootstrap hands its returned tree atomically into the
binding; a later `None` means that frame did not paint, not that prior pixels
vanished. A separate last-frame-painted signal and monotonic paint count keep
tests able to prove `NONE` caused no repaint. Rebinding without a committed
tree clears the prior presentation. This is observation/presentation state,
not manufactured invalidation.

### 4. `RenderBehavior` validates, mutates, releases the borrow, then applies

`RenderBehavior::on_update` is an Active-element operation. Inspection of the
current lifecycle paths found no legitimate non-Active invocation: ordinary
reconciliation updates Active elements, and GlobalKey retake paths activate a
candidate before calling `update`. Therefore a missing `PipelineOwner` is a
broken invariant, not evidence of detachment, and panics with
`expect("BUG: active RenderBehavior must have a PipelineOwner during update")`.
Absence of the behavior's `RenderId`, absence of the corresponding node, or a
concrete render-object type mismatch likewise panics with an
`expect("BUG: …")` message naming that invariant.

Across two non-overlapping `PipelineCell::with_mut` transactions,
`RenderBehavior`:

1. requires its `PipelineOwner`;
2. requires its owned `RenderId`;
3. requires the live node and the expected `V::RenderObject` type;
4. calls `V::update_render_object` and captures the impact;
5. ends the first transaction and its mutable node/render-object borrow;
6. opens a second transaction and calls
   `PipelineOwner::apply_render_update_impact`.

This ordering prevents an aliasing workaround and makes application impossible
until mutation has completed.

### 5. Exports and migration are atomic

`RenderUpdateImpact` is exported from the `flui-rendering` crate root and
prelude. `flui-view` re-exports the same canonical type from its root and
prelude for implementor ergonomics; it does not define a wrapper or alias with
different semantics.

`ClipSourceToken` and `PathClipConfiguration` are intentionally exported
from `flui-objects`, the crate that owns both render objects storing them.
`flui-widgets::ClipPath` and
`PhysicalShape` allocate it during construction; clones share the token and
new configurations do not. The typed builder/setter replaces the temporary raw
`usize` identity seam atomically.

The trait, all production and test implementations, render-object setters,
tests, examples, and documentation migrate in one change. There is no unit
return compatibility trait, conversion shim, deprecated alias, default hook,
or blanket fallback.

No new crate dependency or dependency direction is introduced. The method is
an additive operation on the existing rendering owner, and the re-export
follows the existing `flui-view` → `flui-rendering` dependency. This decision
does not change `docs/runtime-contract.toml` or
`docs/workspace-layers.toml`.

## Consequences

**Positive**

- Delegate policy becomes effective rather than diagnostic-only. False
  `should_relayout` and `should_repaint` answers suppress unnecessary work.
- A single update can express independent visual and accessibility effects
  without losing either one.
- Phase implication and application precedence live once in
  `flui-rendering`, next to the dirty queues they control.
- The update hot path remains allocation-free: one copied byte, const bit
  tests, and the existing dirty walks only when requested.
- Removing the blanket mark makes formerly implicit companion contracts
  explicit: typed parent-data mutation, owner-coupled render membership, and
  retained headless presentation output.
- The opaque representation prevents consumers from manufacturing invalid or
  future-reserved bit combinations.
- Opaque path-source tokens prevent raw callback pointers from becoming public
  identity and introduce no process-global counter into realm-isolated state.

**Negative / trade-offs**

- This is a workspace-wide source-breaking trait migration. Every downstream
  `RenderView` implementation must return an impact.
- `#[must_use]` makes direct setter/updater calls that discard an impact a
  warning; the workspace's warning-as-error policy requires explicit handling.
- Each setter's invalidation semantics become part of its public contract and
  require parity tests, not merely mutation tests for its stored fields.
- `ParentDataView::apply_parent_data` is also source-breaking, but prevents
  unchanged parent-data widgets from silently restoring blanket layout and
  prevents fresh snapshots from erasing layout-owned metadata.
- The two defaulted `apply_parent_data_config` trait methods are additive, but
  a downstream implementation that already defined a same-named inherent or
  extension-trait method may need qualified syntax. The pre-1.0 migration
  ships these seams in `0.3` with no compatibility shim.
- The public owner method accepts a `RenderId`, so direct callers can target a
  stale ID. It intentionally schedules no work in that case; the stricter
  framework path separately panics on its impossible mounted-state failures.
- Creating a callback-backed path widget performs one `Arc` allocation. Updates
  borrow the token and clone its `Arc` only when the source really changes.

**Verification requirements**

- Keep the custom single- and multi-child layout parity pins enabled and green.
- Prove end to end that `LAYOUT` creates no immediate paint queue entry and a
  successful `run_layout` subsequently queues paint.
- Pin the compositing walk for established, newly introduced, and lost target
  boundaries; a child below a repaint boundary; an already-dirty parent;
  parentless and stale targets; and `DirtyKind::Compositing` replay.
- Run targeted mutation testing to prove that restoring blanket invalidation
  or discarding delegate decisions is detected.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Public enum with `None`, `Paint`, `Layout`, `Compositing`, and `Semantics` variants | One update can independently require paint and semantics or compositing and semantics. Adding combination variants creates a closed cross-product and exhaustive-match churn; treating variants as ordered severity loses independent effects. |
| Reuse `DirtyKind` | `DirtyKind` describes one queued request on the cross-thread dirty channel. It cannot compose, and giving it implication rules would conflate transport with the result of a synchronous render-object mutation. |
| Put `RenderUpdateImpact` in `flui-view` | Rendering phases and implication rules are owned by `flui-rendering`. Placing the type above that owner would invert responsibility and prevent lower-layer setters from returning the canonical value without an upward dependency. |
| Keep a private `flui-view` applicator | It would duplicate `PipelineOwner`'s compositing ancestor walk, semantics-enabled/depth policy, and dirty-queue sequencing in a higher crate. The public owner methods are the necessary cross-crate seam and keep dirty application cohesive. |
| Apply compositing with `add_node_needing_compositing_bits_update(id, depth)` | This is a raw queue primitive, not Flutter's `markNeedsCompositingBitsUpdate`: targeting only the changed node skips the necessary ancestor propagation and can queue the wrong responsible root. |
| Set `NEEDS_SEMANTICS` before queueing semantics | The current semantics pass consumes queue entries and does not clear that flag, so setting it would create a second, permanently-stale source of dirty truth. The canonical method is queue-only until the flag participates in a complete set/consume/clear protocol. |
| Expose raw bits or a raw constructor | Callers could create reserved or nonsensical combinations and couple themselves to the byte layout. Named constants plus union cover every supported state. |
| Return `Result<RenderUpdateImpact, _>` | The value reports scheduling effects, not a recoverable operation. Missing owner/ID/node/type states in Active `RenderBehavior` updates are framework bugs, while stale direct-owner IDs already have defined no-op behavior. A result would make every implementor handle an error that cannot originate from its configuration mutation. |
| Give `RenderView::update_render_object` a default returning `NONE` | Existing implementors would compile while silently losing required invalidation. The flag-day trait break is valuable because it forces every implementation to classify its effects. |
| Pass `&mut RenderUpdateImpact` into the updater | A framework-owned mutable accumulator obscures which operation contributed an effect and complicates direct calls. Returning values and unioning them locally keeps effects explicit. `BitOrAssign` remains available for a local, function-owned accumulator. |
| Let setters mark the owner directly | Setters would need owner access or stored handles for synchronous widget updates, broadening coupling and inviting re-entrant dirty work while the render object is mutably borrowed. Returned impact preserves setter authority without owner access. |
| Preserve the unit-returning trait behind a compatibility shim or blanket layout fallback | Active development permits the correct breaking migration. Either compatibility path would retain the original defect and allow new implementations to avoid precise classification indefinitely. |

## References

- [Issue #534](https://github.com/vanyastaff/flui/issues/534)
- Flutter `packages/flutter/lib/src/widgets/framework.dart`
- Flutter `packages/flutter/lib/src/rendering/object.dart`
- Flutter `packages/flutter/lib/src/rendering/custom_paint.dart`
- Flutter `packages/flutter/lib/src/rendering/flow.dart`
- Flutter `packages/flutter/lib/src/rendering/sliver_grid.dart`
- Flutter `packages/flutter/lib/src/rendering/proxy_box.dart`
- [Root port and Definition of Done contract](../../AGENTS.md)
