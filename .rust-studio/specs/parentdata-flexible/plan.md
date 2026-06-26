# ParentDataView family — Flexible / Expanded / Positioned

Status: **SHIPPED** (2026-06-26). Branch `core1-widgets-slice`.

## Goal

Make `ParentDataView` actually attach its configured parent-data onto the child
render node so `RenderFlex` / `RenderStack` read it during layout — unblocking
the `Flexible`, `Expanded`, and `Positioned` widgets. Before this, `ParentDataView`
had a bespoke, owner-blind element whose `apply_parent_data_to_child` was a stub:
the parent-data never reached the render tree.

## Design (8 steps, as executed)

1. **`element/behavior.rs`** — add `parent_data_config(&self, core) -> Option<Box<dyn ParentData>>`
   to the `ElementBehavior` trait (default `None`) and a `ParentDataBehavior` unit
   struct. The behavior is a *transparent proxy*: `build_into_views` returns the
   wrapped child via `proxy_style_views(core, _, V::child)`; `parent_data_config`
   returns `Some(Box::new(core.view().create_parent_data()))`.

2. **`view/view.rs` + `element/unified.rs`** — add `parent_data_config()` to the
   public `ElementBase` trait (default `None`) and delegate from the unified
   `Element` to `self.behavior.parent_data_config(&self.core)`. This surfaces the
   config to the `ElementTree` (which only sees `&dyn ElementBase`).

3. **`view/parent_data.rs` cutover** — `ParentDataConfig` redefined to a blanket
   over `flui_rendering::parent_data::ParentData + Clone + Default` (so the widget
   config IS the render-side parent-data type, no conversion). `ParentDataView`
   supertrait widened to `Clone + Send + Sync + 'static + Sized`. The
   `impl_parent_data_view!` macro now builds the unified
   `ParentDataElement<V> = Element<V, Single, ParentDataBehavior>` (alias in
   `element/mod.rs`). The bespoke owner-blind `ParentDataElement` struct + its
   stub were deleted.

4. **`tree/element_tree.rs::apply_ancestor_parent_data(child_id)`** — called at
   the tail of `insert` and `update`. No-op unless `child_id` owns a render node.
   Walks strictly upward taking the *nearest* `parent_data_config()`, stopping at
   the first ancestor render object (the render parent that reads the data).
   Drops the tree borrow, locks the pipeline owner, writes the config onto the
   child render node (`set_parent_data`) and `mark_needs_layout(parent_render)`.
   This is Flutter's `RenderObjectElement.attachRenderObject` →
   `_findAncestorParentDataElement` fused with `_findAncestorRenderObjectElement`.

5. **`tree/element_tree.rs::reorder_render_children_after_build`** — THE keystone.
   `RenderBehavior::on_mount` appends a render child to its render parent in
   *attach* order (`add_child`). When a component ancestor (`StatelessView` /
   `ParentDataView`) builds its render descendant in a *later* `build_scope`
   iteration than a render sibling that already attached, the parent's children
   list ends up in attach order, not slot order → wrong offsets. A gated
   (`needs_render_reorder`, set on any render-bearing insert) post-build DFS pass
   walks the element tree in slot order, derives each render parent's correct
   child sequence, and insertion-sorts only the drifted ones. Called at the end
   of `BuildOwner::build_scope`. This is a PRE-EXISTING latent bug surfaced by
   `Expanded` — it affects ANY component child of a multi-child render object.

6. **`flui-widgets/src/flex/flexible.rs`** — `Flexible` (flex=1, fit=Loose) and
   `Expanded` (flex=1, fit=Tight). Child is **required** (constructor injection),
   matching Flutter's `required child`; `proxy_style_views` needs a non-optional
   `child()`. `create_parent_data` → `FlexParentData::new(Offset::ZERO, Some(flex), fit)`.

7. **`flui-widgets/src/stack/positioned.rs`** — `Positioned` over `StackParentData`
   (left/top/right/bottom/width/height builders + `fill`). `RenderStack` reads it
   via `PositionedSpec::from_parent_data`.

8. **Tests** — `tests/flex_parent_data.rs` (3) + `tests/stack_positioned.rs` (2),
   asserting both **sizes** and **offsets** through the real layout pipeline.

## Verification

- `flui-view` + `flui-widgets`: full suites green.
- Workspace nextest (CI-equivalent, `--exclude flui-platform --lib --test-threads 1`):
  **2929 passed, 2 skipped, 0 failed**.
- fmt / clippy (`-D warnings`) clean on both crates; port-check triggers checked
  (`ParentData` is allowlisted under FR-036 #9; FR-033 out of scope; trigger 6
  sees `BoxedView` in flui-widgets, not `Box<dyn View>` in flui-view/element).

## Key learnings

- The walk-UP-from-render-child design (not walk-down-from-ParentDataView) is the
  Flutter-faithful one — `attachRenderObject` finds the nearest ancestor
  ParentDataElement. Update re-application rides on the render child's own
  re-`update` (the reconciler walks children after their parent).
- `child_ids` is set by the reconciler at the END of a parent's reconcile, so it
  is NOT reliable at per-child `insert` tail — the reorder MUST be a post-build
  pass, not an insert-time reposition.
- During `build_scope` the pipeline owner is NOT held by the frame driver — each
  `on_mount` / `apply_ancestor_parent_data` / reorder pass locks it fresh, no
  re-entrancy.
