# ADR-0308: The layout↔build fixpoint charges only passes without progress, and an unsettled frame logs and paints

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0017](ADR-0017-build-during-layout-callback-seam.md) §7 only (the
  convergence bound, its debug panic and the claim that nesting fits a pass count), and the §8
  sentence that a growing band "must not trip the fixpoint's `BUG:` bound"
- **Related:** [ADR-0003](ADR-0003-virtualization-core-and-reentrant-build.md) (lazy slivers share
  the fixpoint and its band budget), [ADR-0017](ADR-0017-build-during-layout-callback-seam.md) (the
  fixpoint itself, unchanged)

## Context and problem statement

ADR-0017 builds a `LayoutBuilder`'s child between layout passes: lay out, build what the pass
published, lay out again, until nothing rebuilds. §7 bounds the loop at ten passes in all, calls
reaching the bound a `BUG:` panic in debug, and says nested builders fit that count. They do not:
nesting costs one pass per level, in three ways.

| Frame | Why each level takes a pass | Evidence |
|---|---|---|
| First mount | A fresh builder has no child until its render object publishes constraints, which it does only in `perform_layout`; the pass that builds level *k* mounts level *k*+1 | `crates/flui-view/src/element/layout_builder.rs` (no child until published), `crates/flui-objects/src/layout/layout_builder.rs` (publishes in `perform_layout`) |
| A rebuild above the chain | `LayoutBuilder::new` makes a new builder `Rc`, so a rebuilt level hands its inner builder a new view, whose dirty entry waits in its own scope's bucket for the next pass | `crates/flui-view/src/owner/build_owner.rs` (scope buckets), `crates/flui-view/src/owner/layout_builder.rs` (ready scopes are listed before the drain) |
| A resize | An inner builder builds from constraints published before the outer level rebuilt, so constraint-dependent levels settle one a pass | `crates/flui-view/src/owner/layout_builder.rs` |

Ten nested builders, or fewer around a lazy list whose band uses its own six passes, made a debug
build panic and a release build paint the deep levels unbuilt. Library widgets build through
`LayoutBuilder` (sliders, disclosures, gesture alternatives, drawers), so ordinary pages reach that
depth. And the one cause that legitimately never settles, a builder whose output changes its own
constraints, is application content: `docs/PANIC-POLICY.md` makes a caller-triggerable failure a
non-panic.

How should the fixpoint tell depth from oscillation, and what does a frame that does not settle do?

## Decision drivers

- Legitimate content settles in the frame it changes, at any realistic nesting depth, on first
  mount, after a rebuild and after a resize.
- A builder that keeps rebuilding is caught after a small, fixed number of wasted passes, even
  when its output also mounts fresh builders.
- The frame thread never hangs: there is an absolute bound.
- `docs/PANIC-POLICY.md`: only a FLUI invariant panics; a `BUG:` message names FLUI as the fault.
- The frame path is synchronous and the check is cheap: no per-pass allocation beyond the sets of
  rebuilt scopes, no lock.

## Considered options

1. Raise the pass count (say to 64) and keep the debug panic.
2. Charge only passes that build no scope for the first time in the fixpoint.
3. Charge only passes in which some rebuilt scope is explained neither by a first build, nor by a
   proper ancestor rebuilt in the previous pass, nor by having left the tree; cap all passes at
   256; log and paint when either bound trips.
4. Build mid-pass, as Flutter's `invokeLayoutCallback` does.

## Decision

Chosen option: 3, because it is the only option that separates every legitimate case from
oscillation with a per-node signal.

§1. **Progress.** A pass that serviced a lazy band is progress (`MAX_LAZY_BAND_PASSES` bounds those
passes). Otherwise a pass that rebuilt something is progress when every build-during-layout scope
it rebuilt (compared by `ElementId`):

- built for the first time in this fixpoint, or
- has a proper ancestor (the walk starts at the parent) among the scopes the previous pass rebuilt,
  or
- is no longer in the tree.

Anything else is charged, including a pass with global work alone. The rule is the private
`classify_pass` in `crates/flui-view/src/owner/layout_builder.rs`, returning
`PassOutcome { Settled, Advanced, Charged }`.

§2. **Bounds.** `drive_fixpoint` stops after `MAX_CHARGED_PASSES` (10) charged passes, counted
cumulatively, or `MAX_FIXPOINT_PASSES` (256) passes in all, and returns
`FixpointEnd { Settled, Repeating, PassCap }`. Charged passes are a subset of the passes that asked
for another pass under ADR-0017 §7, and 256 is above 10, so every frame that converged before still
converges.

§3. **An unsettled frame.** On `Repeating` or `PassCap` the frame logs one `tracing::error!` that
names the end and paints the last laid-out tree, in every profile, with no panic. It runs after the
lazy eviction, with no pipeline checkout and no registry lock held (a subscriber is user code).
The leftover work is not rescheduled by the fixpoint.

### Consequences

- Good, because nested builders settle in their frame at any depth below 256, after a mount, a
  rebuild or a resize, and a lazy list under nested builders keeps its own budget.
- Good, because an oscillating scope is charged even when its output mounts a fresh inner builder
  in the same pass: the oscillating scope itself is never fresh and never under a rebuilt ancestor.
- Good, because a run of uncharged repeats strictly deepens the shallowest rebuilt scope, so it is
  no longer than the tree is deep, and fresh scopes need the tree to grow, which §2 caps.
- Bad, because an oscillating builder whose output contains a nested `LayoutBuilder` leaves that
  scope's bucket dirty, and a dirty bucket requests the next frame (`BuildOwner::has_dirty_elements`
  counts scope buckets). Such content keeps requesting frames, as it did in release builds before;
  in debug builds it is no longer throttled by frame-failure backoff. Gating that wake is a
  follow-up.
- Bad, because content that nests build-during-layout 256 levels deep is cut off, with an error.
- Neutral, because a well-behaved frame does the same passes as before: the classification is one
  set lookup per rebuilt scope, plus an ancestor walk for a scope that rebuilds again.

### Confirmation

- Unit rows in `owner_layout_builder_matrix` (`crates/flui-view/src/owner/layout_builder.rs`):
  `layout_builder_fixpoint_is_bounded_and_never_spins` (charged forever ends `Repeating` after 10;
  progress three times past the bound settles; progress forever ends `PassCap` after 256; charged
  passes count through progress) and
  `layout_builder_pass_progress_is_explained_by_a_fresh_or_ancestor_build` (fresh scopes, a repeat
  under a grandparent the previous pass rebuilt, a repeat with no ancestor rebuilt, an oscillating
  scope beside a fresh one, a scope is not its own ancestor, a scope gone from the tree, a lazy
  band, global work alone).
- Public rows in `crates/flui-widgets/tests` (the `reactivity_and_dependencies` and `lazy_slivers`
  tables): `nested_layout_builders_settle_on_their_first_frame` (16 levels, then a rebuild above
  them), `nested_constraint_dependent_layout_builders_settle_after_a_resize` (14 levels padding each
  side by an eighth of their width, each level's last constraints against the oracle),
  `a_layout_builder_feeding_its_own_constraints_paints_without_panicking`, and
  `lazy_list_view_builder_pathological_extents_under_nested_layout_builders_defer_instead_of_panicking`
  (nine builders around a list whose band cannot settle in its budget).

## Pros and cons of the options

### 1. A larger pass count, debug panic kept

- Good, because it is one constant.
- Bad, because any count is a depth limit in disguise: depth is what costs passes.
- Bad, because a real oscillation then wastes that many passes per frame before it is caught.
- Bad, because the debug panic still blames FLUI for application content.

### 2. Charge passes that build nothing new

- Good, because it covers the first mount and the rebuild cascade.
- Bad, because a resize rebuilds levels that already built in this fixpoint, one level a pass, so
  eleven or more constraint-dependent levels stop with stale constraints.
- Bad, because an oscillating builder whose output remounts a subtree with a fresh inner builder is
  never charged, and runs to the hang guard on every frame.

### 3. Charge passes no rebuilt scope's history explains (chosen)

- Good, because it covers all three legitimate cases and charges every oscillation.
- Good, because the rule is a pure function, unit-testable with synthetic trees.
- Bad, because it needs the tree's parent links for scopes that rebuild again.

### 4. Build mid-pass

- Good, because nesting would cost no passes.
- Bad, because ADR-0017 rejected it: the layout walk holds `&mut RenderTree`, and building mounts
  render objects through the checked-out pipeline. Nothing about that has changed.

## More information

The repository already charges re-entries only, never first arrivals, in the build owner's mid-drain
absorption (`MAX_MID_DRAIN_ABSORBS`, `crates/flui-view/src/owner/build_owner.rs`). Revisit if a
profile shows the ancestor walk on a hot frame, or if content needs build-during-layout deeper than
the pass cap.
