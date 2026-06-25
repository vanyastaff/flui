# PR-K execution spec — live BuildContext during build (by-value extraction)

Companion to [ADR-0008]. PR-K is the keystone: it gives the `BuildContext`
threaded into user `build()` a live view of the REAL element tree, so
InheritedView / `find_ancestor_*` / `dispatch_notification` stop being inert
in production. It
is a single **atomic** cross-crate change (no shim — active-dev): there is no
green intermediate checkpoint because the foundation pieces (`take/put`,
`TreeRead`) are dead code until `build_scope` and every `build_into_views`
impl consume them. Land it whole, or stop `BLOCKED`.

F1 (ratified): one PR, two internal commits, both compiling — (1) mechanical
plumbing, (2) wire-real. If commit (1) cannot compile green on its own,
report `BLOCKED`; do not merge a partial.

## The borrow problem and the fix

`build_scope` owns `&mut ElementTree` and today does
`tree.get_mut(id).element_mut().build_into_views(owner)` — the `&mut node`
borrow is live across the whole build, so the behavior cannot also read the
tree (it builds `new_minimal()` over an empty dummy → inert).

Fix = **by-value extraction**: lift the element OUT of its slot so the tree is
free for a shared borrow during the build, then put it back.

```text
phase 1 (extract + build):
    let mut elem = tree.take_element(id);          // slot -> None
    let views = elem.build_into_views(&TreeRead(&*tree), &mut owner);
    tree.put_element(id, elem);                     // slot restored
    owner.apply_inherited_records(tree);           // synchronous dependent recording
phase 2 (reconcile): unchanged — reconcile_children_by_id(tree, id, &views, &mut owner)
```

`&*tree` (shared) and the local `owner` split-borrow are the only live borrows
during the build; `self.dirty_elements` is touched only by `pop()` before and
the sink apply after — never during. No `&mut` spans a second slab access.

## File-by-file ripple (the flag-day surface)

### `tree/element_tree.rs`
- `ElementNode.element: Box<dyn ElementBase>` → `Option<Box<dyn ElementBase>>`.
- `ElementNode::element()` / `element_mut()` keep their `-> &dyn ElementBase`
  signatures but `.expect("element present; only build_scope's take/put window
  removes it")` — so the ~29 existing `.element()` call sites are UNCHANGED.
- Add `ElementTree::take_element(id) -> Option<Box<dyn ElementBase>>`
  (`node.element.take()`) and `put_element(id, Box<dyn ElementBase>)`.
- Add `TreeRead<'a>(&'a ElementTree)` with accessors that read `ElementNode`
  fields surviving the take — `parent(id)`, `depth(id)`, `child_ids(id)`,
  later `inherited_map(id)` — and ONE hole-aware accessor
  `get_element(id) -> Option<&dyn ElementBase>` returning `None` on the
  tombstoned slot (a miss that can never match a `depend_on`/`find_ancestor`
  query in any build profile — not a debug-only assert).

### `view/view.rs` (trait) + every impl
- `ElementBase::build_into_views` signature gains the read view:
  `fn build_into_views(&mut self, tree: &TreeRead<'_>, owner: &mut ElementOwner<'_>) -> Vec<Box<dyn View>>`.
- Update ALL impls (≈16): production `element/unified.rs:307`; hand-rolled real
  `view/error.rs:281` (ErrorElement), `view/parent_data.rs:268`
  (ParentDataElement), `element/root.rs:196` (RootElement); test fixtures
  `binding.rs:1463,1865`, `element/behavior_commons.rs:372`,
  `view/inherited.rs:159`, `view/parent_data.rs:346`, `view/proxy.rs:103`,
  `view/root.rs:535`, `owner/build_owner.rs:723`. Test fixtures that ignore it
  just take `_tree`.

### `element/behavior.rs` (the 6 behaviors)
- `ElementBehavior::build_into_views(&mut self, core, owner)` gains `tree:
  &TreeRead<'_>`; `unified.rs` forwards it.
- Stateless/Stateful build: replace `ElementBuildContext::new_minimal(depth)`
  with a real `ElementBuildContext` over `TreeRead` + the element's id/depth
  (lines 259, 383). Same for the `did_change_dependencies` context (line 464).

### `context/element_build_context.rs`
- Reborrow `ElementBuildContext` on a `TreeRead<'b>` + `&mut ElementOwner`
  instead of `Arc<RwLock<ElementTree>>` + `Arc<RwLock<BuildOwner>>`. The
  ancestor walks (`walk_ancestors_for_inherited`, `walk_strict_ancestors`,
  `find_ancestor_*`, `dispatch_notification`) read via `TreeRead`.
- `depend_on_inherited`: record the dependent into the provider node
  SYNCHRONOUSLY (the provider is a different live slab node; with the building
  element extracted by value there is no `&mut` conflict). Only the
  dependent's mark-dirty defers into `build_scope`'s depth-ordered heap.
- DELETE `new_minimal`, `is_minimal`, the minimal-context `mark_needs_build`
  no-op branch.

### `owner/build_owner.rs`
- Rewrite the `build_scope` drain to take/build-with-`TreeRead`/put (above).
- DELETE `SHARED_DUMMY_TREE`, `SHARED_DUMMY_OWNER`, `shared_dummy_tree`,
  `shared_dummy_owner`, and the 3 tests pinning them
  (`test_shared_dummy_*`, `test_new_minimal_*`).
- Add `clippy::disallowed_methods` (or delete the ctor) so the dummy path
  cannot return.

## Soundness checklist (the critique's blockers — verify each)
1. Tombstone reentrancy: enumerate every `TreeRead::get_element` reachable from
   `build_into_views` and the in-window `did_change_dependencies` and confirm
   none targets the in-flight id needing `.element()`. The GlobalKey same-frame
   rescue (`framework.dart:4567-4601`) runs in phase 2 AFTER `put_element` — add
   a harness test that hits the hole if drain ordering regresses.
2. Same-pass notify: synchronous dependent-record + deferred mark-dirty drained
   into the SAME depth-ordered heap. Test: provider shallower than a
   newly-depending consumer, both dirty one pass — consumer rebuilds with the
   new value.
3. No `&mut` across a second slab access in the new drain — re-derive from the
   actual borrows (element extracted by value, `&*tree` shared, `owner` local).

## Failing-first tests (Definition of Done)
- `theme_provider_delivers_to_depth3_consumer`: a `ThemeProvider` over a depth-3
  consumer whose `build` calls `cx.depend_on::<ThemeProvider>()` returns
  `Some(RED)` after / `None` before (fails today against the dummy tree).
- `same_pass_provider_update_and_new_dependent`.
- `self_mark_during_build_is_noop` (Flutter `framework.dart:5848`).

## Out of scope (later PRs)
O(1) persistent inherited map (PR-2), `ElementBase` shrink (PR-3, fork F2),
field-precise `depend_on` + derives + `Mounted`/`EffectScope` (PR-4). PR-K only
makes the context REAL via the existing `ElementBuildContext` surface.

[ADR-0008]: ../adr/ADR-0008-flui-view-leapfrog-buildcontext-inherited-element.md
