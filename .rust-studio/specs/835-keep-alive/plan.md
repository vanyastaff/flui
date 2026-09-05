# #835 — keep-alive for lazy sliver children (post-review design)

Supersedes the pre-review draft. Two independent reviews (adversarial + architectural) rejected
all three candidate routes and converged on the shape below. Where they agreed independently, that
is noted — it is the strongest signal in the record.

## The problem
`SparseChildren::retain_band` (`flui-view/src/element/sparse_children.rs:174`) evicts every lazy
child outside the cache band unconditionally, so an item loses its state when scrolled away.
`SliverMultiBoxAdaptorParentData.keep_alive` (`flui-rendering/src/parent_data/sliver_variants.rs:89`)
exists with **no non-test reader or writer anywhere in the workspace**.

## Rejected routes, with the reason each died

| Route | Why rejected |
|---|---|
| **The issue's own `KeepAlive::new().child(item)` as a `ParentDataView`** | Silently inert in the DEFAULT config. `wrap_builder_in_repaint_boundaries` wraps from *outside* (`sliver_list.rs:98`), so a user's `KeepAlive` is always *inside* the auto-added `RepaintBoundary`; `apply_ancestor_parent_data` breaks at that boundary (`flui-view/src/tree/element_tree.rs:1029`); the item's own render node was never stamped (`element/unified.rs:215`), so the write installs fresh parent data on the wrong node — no panic, no effect. With `repaint_boundaries(false)` it *works*. Worst possible orientation. Also: `SliverMultiBoxAdaptorParentData::default()` carries index **0**, a real index, which trips the uniqueness `debug_assert!` at `virtualized_band.rs:230`. |
| **Bubbling `KeepAliveNotification` + holder id** | `on_notification(&self, TypeId, &dyn Any) -> bool` (`element/behavior.rs:352`) carries no sender identity, no `&ElementTree`, and takes `&self`. Getting a tree handle is a 4-site trait-signature change. Worse: `dispatch_notification` has **zero production callers** and `on_notification` **zero production impls** — only `tests/notifications.rs`. Building on it is this repo's own named dominant defect class (shipped seams never wired). And a lease keyed `(logical_index, holder)` goes stale the moment `reconcile` calls `relocate_sparse_child` (`sparse_children.rs:377`); the liveness sweep meant to fix that misses the `GlobalKey` retake it was invented for, since `forget` (`:128`) reparents rather than removes. |
| **Flag on the item boundary's own parent data** | Not for layer inversion — that render→element channel already exists and is load-bearing (`sliver_list.rs:234` → `pipeline/owner/mod.rs:277` → `build_owner.rs:1624`). Rejected for reintroducing the ownerless `kept_alive` invariant and a second writer on a struct the logical-index stamp owns (`behavior_commons.rs:79-104` deliberately preserves `keep_alive` across restamping). |
| **`drop_child` park + keep-alive bucket (Flutter's shape)** | `storage/tree.rs:551` leaves `parent = None` and `pipeline/scheduler.rs:227` reads parentless as a relayout **root**, so a self-dirtying parked child lays out standalone at cached constraints. Also drags back Flutter's whole `_keepAliveBucket` re-adoption dance that park-in-place deletes. |
| **`InheritedView` `KeepAliveScope` (the `VsyncScope` idiom)** | The sliver *host* needs the registry too, and it is a render behavior that cannot read inherited data ambiently — so this forces a new StatefulView wrapper **plus** a config field on `SliverMultiBoxAdaptor` **plus** wiring in all three sliver widgets. Every other `BuildContext` capability in this codebase resolves from `self.owner`, not from an ancestor (`context/element_build_context.rs:224-249`). |

## The design

**An RAII lease, element-side, on a presentation-scoped registry.** This follows ADR-0018/0021/0030/0037's
established precedent for lifecycle-acquired capabilities, so port-check trigger #22 covers the
discipline mechanically (acquire in `init_state`, never in build/layout/paint).

- `KeepAliveHolds` on `BuildOwner` (holder `ElementId` → held sparse-child `ElementId`), threaded
  through `ElementOwner`; cleared on unmount beside `inherited_dependencies`.
- `BuildContext::keep_alive_lease() -> Option<KeepAliveLease>`; the lease is `#[must_use]` and its
  `Drop` releases the hold. **Leases are keyed by resident `ElementId`, never by logical index** —
  that is what makes relocation free.
- Binding a lease to its item uses the identity that already exists: the nearest ancestor with
  `sliver_slot == Some(_)` *is* the sparse child (`element/unified.rs:209-221`), and nesting
  resolves innermost-first. Needs `ElementBase::sliver_slot()` — the read half of an accessor pair
  whose write half already ships (`element/generic.rs:549` is `ElementCore`-only today).
- Registering trigger #22's 8th token in `scripts/check-frame-capability-scope.sh` lands in the
  **same change** as the capability, per AGENTS.md.

**Park-in-place: do nothing to the render tree.** `walk_virtualizer_band` lays out only
`cache_first..cache_last` (`flui-objects/src/sliver/virtualized_band.rs:249`), so an attached
out-of-band child is simply never laid out — and every one of the four walks now consults the
placed-generation stamp (paint `pipeline/owner/paint.rs:397`, box hit-test `accessors.rs:670`,
sliver hit-test `accessors.rs:979`, semantics `pipeline/owner/semantics.rs:548`). Parking costs
zero render-tree surgery. This is also Flutter-faithful for the case that matters:
`visitChildrenForSemantics` explicitly excludes the keep-alive bucket
(`.flutter/.../rendering/sliver_multi_box_adaptor.dart:418-421`, `// Do not visit children in [_keepAliveBucket]`).

**One guard, in `retain_band` only — never in reconcile eviction.** Both reviews derived this
independently, and it matches Flutter's own split: `_destroyOrCacheChild` is reached only from
`collectGarbage`, while a data-source removal goes through `removeChild` → `updateChild(.., null)`
and destroys regardless of `keepAlive`. Guarding reconcile's unclaimed-resident arm
(`sparse_children.rs:357`) would resurrect the uniqueness assert: a held resident squatting on
index 3 while a keyed resident relocates onto 3 leaves two attached children stamped 3.

**Dead code deleted:** `KeepAliveParentDataMixin`, both `keep_alive` parent-data fields, their
`Hash` lines, the `behavior_commons.rs:81` doc sentence and the `:572` assert. Semver-safe: absent
from `docs/runtime-contract.toml`, and the facade deliberately does not re-export flui-rendering
(root `src/lib.rs:105`).

## Hard prerequisite
Step 6 (the `retain_band` guard) blocks on **PR #883** merging. #835 is what first creates
long-lived attached-but-unlaid children, so without the semantics gate it would convert a latent
gap into a live a11y defect — a screen reader announcing rows at a rect from a pass that no longer
holds. Steps 2–5 are independent and can land first. If #883 were ever rejected, the fallback is
**deferring keep-alive**, not `drop_child`.

## Open questions that must be answered before merge (verified, unbudgeted)

1. **A parked child that keeps animating.** Verified both halves: it lands in `run_paint`'s residue
   scan every frame (`paint.rs:155`, `tracing::warn!` for any dirty node the root descent missed,
   which also evicts its retained capture), and `fire_need_visual_update` fires on a new boundary
   entry (`pipeline/scheduler.rs:242`), so it keeps waking the loop with nothing on screen changing
   — against the premise `just live-smoke`'s occlusion check asserts. Flutter does not warn here;
   `flushPaint` checks `layer.attached` and calls `_skippedPaintingOnLayer()`. At minimum the
   residue scan must distinguish "parked by keep-alive" from "detached subtree".
2. **Bound the parked set.** It is user-controlled and unbounded. `virtualized_band.rs:219` claims
   "O(K) … bounded by viewport" and `RenderSliverList::hit_test` reverse-iterates
   `0..attached_child_count` on every pointer event (`sliver_list.rs:243`). Both become
   O(band + parked). Cap with the house rule's latched-warn half, correct the stale claim in the
   same change, and add a parked count to `debug_fill_properties`.
3. **Release timing.** Flutter's own author documents this as the hard part
   (`automatic_keep_alive.dart:190-260` — a forty-line apology about the last handle dropping
   mid-build/layout). FLUI's band eviction runs between layout passes (`layout_builder.rs:445`)
   and once post-frame. Specify what a release during layout / paint / a post-frame callback does.
4. **`ItemCount::Unknown` zombie.** `clamp_render_item_count` can shrink the count below a held
   index; `virtualized_band.rs:249` then never lays it out and `retain_band` never evicts it.
5. **`repaint_boundaries(false)`** (`list_view.rs:153`, `grid_view.rs:174`): supported, or a loud
   documented no-op.

## Contract change this records
Keep-alive **deletes the evict-before-paint invariant** the render layer documents itself as
relying on (`flui-rendering/ARCHITECTURE.md:98-102`; wired at `flui-view/src/owner/layout_builder.rs:463`,
whose comment says "so a resident the band dropped is gone before anything paints, hit-tests, or
assembles semantics over it"). The placed-generation gate stops being defence-in-depth and becomes
the sole mechanism keeping parked content off screen. That promotion is the headline of the ADR.

## Prime Directive accounting
**ADR-0056** (next free; note `ADR-0047` is already double-assigned — do not compound it, and do
not fold that fix into this work). It must record park-in-place *because* the gate covers all four
phases, naming #883 as prerequisite and `drop_child` as the rejected fallback with its reason —
otherwise a future reader sees "we do nothing on park" with no visible justification.

Replacement tests, mapped from `.flutter/packages/flutter/test/widgets/automatic_keep_alive_test.dart`
(9 names, 12 executions — `void tests({required bool impliedMode})` invoked twice):

| Flutter test | FLUI obligation |
|---|---|
| ListView with/without itemExtent, GridView (`:63`,`:111`,`:161`) | Port all three — one per render family, each has its own band walk |
| `AutomaticKeepAlive double` (`:219`) | Port — two leases on one item; releasing one keeps it alive, the last evicts |
| `double 2` (`:295`) | Port **as a named gap**: reparenting a holder between slivers; document the hole, test current behavior |
| `keepAlive set to true before initState` (`:478`) | Retire — pins Flutter's post-frame fallback, which FLUI has no ordering for. Replace: a lease taken in `init_state` is effective the *same* frame |
| `…widget goes out of scope` (`:515`) | Port — 250 items, jump past the whole window, holds survive. Catches a guard that only works for incremental scrolls |
| `SliverKeepAliveWidget` (`:557`) | Drop, reason recorded — an artifact of the parent-data design this deletes |
| `Listenable has its listener removed once called` (`:570`) | Retire — `Drop` replaces it. Replace: a lease dropped without an explicit call releases |

Plus two FLUI-only pins: a held out-of-band child publishes **no semantics** (#835 owns this
separately from #883's — same mechanism, different premise: "held" vs "parent stopped laying out",
so keep-alive's suite goes red rather than silently regressing a11y if the gate is reverted); and a
held resident the builder stops producing is destroyed anyway (the retain_band-only guard).

Deliberately out of scope, named not dropped: `_SelectionKeepAlive` (`scroll_delegate.dart:799`)
— no selection registrar wired here; and the stale `layoutOffset` on a parked child, which FLUI
inherits identically.

## Build order
1. ~~Repair stale comments~~ — **done**, landed in #883 (paint.rs + two box_protocol blocks).
2. Delete the dead parent-data keep-alive. *(independent)*
3. `ElementBase::sliver_slot()` reader. *(independent)*
4. `KeepAliveHolds` on `BuildOwner` + `BuildContext::keep_alive_lease()` + clear-on-unmount.
5. Trigger-#22 token, same change as (4).
6. **Guard `retain_band` only** — blocks on #883. Factor the two duplicated eviction bodies
   (`sparse_children.rs:151` and the direct `remove_subtree` at `:357`) into one helper first, so
   the guard has exactly one home.
7. `KeepAlive` convenience widget on top of the lease (~30-line `StatefulView`: acquires in
   `init_state`, drops in `did_update_view` when the flag clears). A bool alone is insufficient —
   the deciding state lives in a descendant's own State, and N descendants each want a hold, which
   a bool resolves last-writer-wins.
8. ADR-0056 + the test table.
