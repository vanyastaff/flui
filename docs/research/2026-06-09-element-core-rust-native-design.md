[← KTD-9 mechanical design](2026-06-09-wave2-ktd9-element-storage-migration.md) · [Architectural contracts](2026-05-22-architectural-contracts.md) · [Foundations Part II](../FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter)

# Element Core — Rust-Native Design (working setState, the *non-mechanical* way)

> **Supersedes** the mechanical-port framing (slab + `Vec<ElementId>` + unsafe subtree-reborrow). Per the owner's direction: don't transliterate Flutter's element tree — design it Rust-native, the way flui did `RenderObject` (compile-time `Arity`, typestate `PipelineOwner`). This design makes production `setState` work while **eliminating both of the adversarial's FATAL findings by construction**, with **zero `unsafe`** in the element tree.

## The two fatals the mechanical port hit — and how design erases them

1. **Borrow-UB** (mechanical: `build_scope` holds `&mut node` while child storage needs `&mut tree` → the Stacked/Tree-Borrows reborrow UB that cost the render tree PRs #144/#145).
2. **Slab-index-reuse** (mechanical: a stale `ElementId` silently resolves to a *different* element after the slab reuses a freed index → silent wrong-element addressing).

Both are artifacts of *copying Flutter's pointer-tree into a bare `Slab`*. Two Rust-native moves erase them:

### Move 1 — Extract-then-apply rebuild (ownership-leveraged) → no `unsafe`, no reborrow

`build()` returns the child view(s) **owned** (the ephemeral view tree — Xilem's insight, validated in the deep-research). So the rebuild releases the parent borrow *before* touching children:

```
rebuild(tree, id):
    let new_child_views = { let node = tree.get_mut(id)?; node.build_into_views() };  // parent &mut ENDS here
    reconcile(tree, id, new_child_views)        // fresh &mut tree, recurse per child
```

The parent's `&mut node` is **never live across** child insert/update/remove, because the child views are moved out of `build()`. No two simultaneous `&mut` into the arena → **borrow-safe by construction, no raw pointers, no `get_subtree_mut`**. This is strictly better than porting the render tree's `unsafe` subtree-reborrow. It is also *behavior-identical* to Flutter: same depth-first traversal (build E → reconcile E's children → recurse), the borrow is merely released at the natural seam Dart never had to think about.

### Move 2 — Generational arena → stale ids are *detectable*, never silent-wrong

`ElementId` becomes a **generational key** (slotmap-style `(index, generation)`), not a bare 1-based index. A key into a freed-then-reused slot fails the generation check → `get(id)` returns `None` instead of a *wrong* element. The adversarial's slab-reuse fatal (dangling id → silent wrong-element) becomes **impossible**. Idiomatic Rust (the standard answer to ABA on arenas).

## Behavior stays Flutter-faithful (C1)

`setState` → mark the element's gen-key dirty + insert into the depth-ordered `BuildOwner` heap + fire the wake chain (the locked `architectural-contracts.md:54` sentence, now actually true). `build_scope` drains shallow-first; each dirty id → `rebuild` (build_into_views + reconcile). No signals. Memoize (`should_skip_rebuild`/`Memo`, shipped) prunes unchanged subtrees during reconcile.

## Bonus: GlobalKey reparent gets *simpler*, not harder

A `GlobalKey`'d child moving between two live parents in one frame: reconcile finds it by key, and because it lives in the arena, we just **re-point its parent edge** (the gen-key is stable → state preserved). No inactive-queue round-trip. The adversarial's "cross-parent Active→Active state loss" case becomes the natural path, not a special case.

## Contract compliance

- **C1** — setState canonical, no signals; the rebuild-propagation sentence becomes literally true.
- **C4** — `View` object-safe (untouched); element storage is the generational arena; the one `dyn` boundary stays at the arena slot (`Box<dyn ElementBase>`) + the `Vec<BoxedView>` dynamic-children fallback. *(Closed-enum `ElementKind` remains a separable later internal optimization — Contract 2 permits storage re-shaping behind the locked `View` trait.)*
- **C6** — keyed reconcile over the arena (by `node.key()/key_hash()`); reparent by gen-key.
- **C9** — concrete `V` flows into the slot at `create_element`; child edges are gen-keys; **no new `dyn` boundary**, and the second (box-vec) ownership graph is *deleted*.

## Wrinkles, resolved

- **Parent→child config flow**: the child's new view comes *from* the parent's `build_into_views()` → carried into `reconcile_child` → child updates with the fresh config. Preserved.
- **Child-edge writeback**: reconcile produces the parent's new child-key list; re-borrow parent (after children settle) to store it — second parent borrow, never held across child mutation.
- **InheritedWidget depend_on**: ancestor walk via the arena's `parent` edges; dependents keyed by gen-key. Unchanged semantics.
- **mid-build dirty**: marking dirty touches only the `BuildOwner` heap (separate from the arena's `&mut`) → no double-borrow of the tree; shallower-already-built defers, deeper coalesces (heap order). Cleaner than the current `ElementOwner`-split-borrow-aliases-`dirty_elements`.
- **Deadlock (orthogonal, still required)**: global lock order `widgets` before `pipeline_owner`; setState marks via a lock-free side-queue + AtomicBool, never re-entering `widgets.write()` from a build/wake callback.

## Sequenced units (single PR, no `unsafe`)

- **E1 — generational `ElementId` + arena.** Migrate `ElementTree` from `Slab` to a generational arena (slotmap, or a `slab` + parallel generation vec); `ElementId` carries the generation; `get`/`get_mut` generation-check. Ripple: every `ElementId` consumer (mostly mechanical).
- **E2 — `build_into_views` (extract).** Element behaviors' build returns owned child views (the ephemeral view tree) instead of mutating storage inline.
- **E3 — extract-then-apply `reconcile` over the arena** (child edges = gen-keys; keyed; reparent-by-key; remove via arena drop). Replaces `VariableChildStorage`'s box-vec + the positional/keyed split. **E1+E2+E3 land atomically** (the storage shape change).
- **E4 — real tree-bound `BuildContext`** (replaces `new_minimal`; `ctx.set_state`/`depend_on` reach the owner + ancestors).
- **E5 — setState → side-queue schedule_build_for + lock-order/deadlock guard.**
- **E6 — slab-resident root + delete `AppBinding.root_element`/`mount_root`/`rebuild_root`; bootstrap via `attach_root_widget`.**
- **E7 — wake chain** (`on_need_frame` → `request_redraw`).
- **E8 — GATE: end-to-end production-paint setState round-trip** (drive `AppBinding::render_frame`; deep stateful element; assert scene changed; must fail on main first).
- **E9 — full-tree-dirty animation criterion bench** (arena `get_mut` vs box deref; prove no 60fps regression).

## Status

Design articulated. Next: **adversarial validation** of the extract-then-apply + generational-arena approach (does it hold for InheritedWidget, GlobalKey reparent, RenderObjectElement child-slot mgmt, mid-build, perf?), then build the single PR with zero `unsafe` in the element tree.
