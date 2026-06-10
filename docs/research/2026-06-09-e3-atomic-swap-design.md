[← ALT-2 plan](2026-06-09-alt2-arena-element-tree-plan.md)

# E3 — Atomic box→arena swap: design

> **Stage E3** of ALT-2. Collapse the live nested `ElementCore::children: A::Storage` (box graph) onto the slab arena, so the slab `ElementTree` is the single element graph and production `setState` actually rebuilds. **One atomic commit, no mixed box/id intermediate** (a half-swap = some traversals see box children, some see id children → inconsistent graph). E0a/E1/E2/E2.5 have de-risked the prerequisites; E2.5's `reconcile_children_by_id` is the proven, miri-clean engine this stage wires in.

## The one hard problem (and its solution)

`BuildOwner::build_scope` (build_owner.rs:341-373) drains the dirty heap holding `&mut tree.get_mut(id).element` to call `perform_build(&mut self, owner)`. That `&mut element` is borrowed **from the tree**, so `perform_build` cannot also take `&mut ElementTree` to reconcile slab-resident children — the exact double-borrow that cost the render tree PRs #144→#145→U20.1.

**Solution — extract-then-apply, applied at the build seam** (the same discipline E2.5 enforces for children, lifted one level):

```
// build_scope, per dirty id:
let new_views: Vec<Box<dyn View>> = {
    let node = tree.get_mut(id)?;            // &mut element borrowed from tree
    if !node.element().lifecycle().can_build() { continue }
    node.element_mut().build_into_views(owner)   // run behavior build → OWNED child views
};                                            // element borrow ENDS here
reconcile_children_by_id(tree, id, &new_views, owner);  // fresh &mut tree, E2.5 engine
```

`build_into_views` runs the behavior's `build()` (today's `build_or_recover` / `build_proxy_style` half) and **returns the owned child view(s)** — it does NOT touch child storage. The reconcile (today's `finish_*_build` half) is hoisted out to `build_scope`, where it runs the E2.5 reconciler against the slab with a fresh borrow. No `&mut element` is ever live across a `&mut tree` child mutation.

The behaviors **already** carry this split internally (`build_or_recover` produces a `Box<dyn View>`; `finish_single_child_build` reconciles it). E3 only changes *where the seam is cut*.

## Change-list (all in one commit)

1. **`ElementBehavior::perform_build(&mut core, owner)` → `build_into_views(&mut core, owner) -> Vec<Box<dyn View>>`.** Each behavior returns its child view(s) instead of reconciling them:
   - Stateless/Stateful: `vec![child_view]` from `build_or_recover` (single child).
   - Proxy/Inherited: `vec![V::child(view).boxed()]` (the proxied child).
   - Leaf / Render-leaf with no view-children: `vec![]`.
   - RenderObject (single/multi arity, behavior.rs:546/928): return the child view(s) the render element owns; the RenderObject-attach side-effects (creating/attaching the `RenderObject` to the pipeline) stay in `build_into_views` (they touch `core.pipeline_owner`/render tree, NOT the element slab, so no double-borrow) — only the *element*-child reconcile is hoisted.
   - `behavior_commons::finish_single_child_build` / `finish_*_build` are deleted (their reconcile body moves to the E2.5 engine); their build-side helpers stay.
2. **`ElementBase::build_into_views(&mut self, owner) -> Vec<Box<dyn View>>`** (dispatch boundary) replaces `perform_build`. The `#[cfg(debug)]` dirty/should-build gating stays.
3. **`build_scope`** (build_owner.rs): the extract-then-apply loop above. `ElementOwner` is built once and used across both phases (it aliases `BuildOwner` fields, never the tree).
4. **Mount** (`ElementTree::insert` / `mount_root_with_pipeline_owner`): after creating + mounting the element, run `build_into_views` + `reconcile_children_by_id` for the initial children (same extract-then-apply), so the initial subtree is slab-resident from frame 1. (Today `mount` recursively creates box children; that recursion is replaced by the id reconcile.)
5. **`ElementCore::children: A::Storage`** is removed. Element cores no longer own children. The 4 `ElementChildStorage` impls + the trait are deleted (their keyed-reconcile logic already lives in `id_reconcile.rs`; arity is still enforced at the type level by `A: ElementArity`, just no longer carrying storage). `A::Storage` associated type retired.
6. **`ElementBase::visit_children`** (the no-op at unified.rs:186 / behavior_commons.rs:375): deleted. All child traversal goes through `ElementTree::get(id).child_ids()` (the E2.5 field, now the authoritative child list). Callers of `visit_children` rewire to walk `child_ids` via the tree.
7. **Unmount**: deepest-first via `child_ids` over the tree (collect ids, recurse to leaves, `tree.remove_finalized` bottom-up) — no `Box`-drop frees a subtree implicitly. Uses the E2.5 owned-collect borrow discipline.
8. **`setState`** (unified.rs:460): `ElementCore::set_state` flips dirty + `owner.schedule_build_for(self_id, depth)` (off-frame: the E0a lock-free side-queue; mid-frame: direct). The `new_minimal` dummy `BuildContext` is replaced by a real `self_id`+depth-bound context whose `mark_needs_build` actually schedules.
9. **Fixtures**: migrate the ~5 element-tree test fixtures that construct/inspect box children to the id model (`tree.get(id).child_ids()`).

## Invariants the implementation MUST uphold
- **No `&mut` into the slab held across a second slab access**, anywhere (build, mount, unmount, reconcile). Verify with `cargo +nightly miri test`.
- **Zero new `unsafe`.**
- **No mixed intermediate**: the diff compiles + all tests pass only at the end; there is no commit where some children are box-owned and some id-owned.
- **Atomic**: one commit.

## Verification (the E2.5 bar, scaled)
`cargo build -p flui-view`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo nextest run --workspace --test-threads 1`; `cargo +nightly miri test -p flui-view` (element-tree + reconcile + a mount/unmount path); `cargo fmt --all -- --check`; `bash scripts/port-check.sh`. E4 (the next stage) is the e2e production-paint proof that setState now actually repaints.

## Open risks to watch
- **RenderObject behaviors** are the subtle ones: they manage *both* an element child and a render-tree child. Only the **element**-child reconcile is hoisted; the render-attach stays in `build_into_views`. If a render behavior's render-attach needs the child's `ElementId` (assigned only after the id-reconcile inserts it), thread that as a post-reconcile step in `build_scope` (a second fresh-borrow pass), not inside `build_into_views`.
- **InheritedWidget `depend_on` / ancestor walk** uses parent edges — already id-based (`ElementNode.parent`), unaffected, but confirm the dependents map keys stay valid across reconcile.
- **GlobalKey reparent** (E3.x, deferred): E3 keeps the soft-remove/inactive path working through the id reconcile; the cross-parent Active→Active case is E3.x.
