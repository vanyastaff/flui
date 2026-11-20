### Added

- **Render-tree edits on `PipelineOwner`** (`flui-rendering`): each edit applies the dirty marks
  it owes, so a caller never follows it with `apply_render_update_impact`.
  `set_render_children(parent, children) -> bool` replaces a child list and marks the parent and
  every donor for layout, compositing bits and semantics; `edit_render_children` runs a batch of
  `RenderChildrenEdit::set_children` / `detach_child` edits and marks each changed parent once (a
  pure reorder for layout only); `update_render_object::<R>(id, f)` runs `f` on the typed render
  object and applies the `RenderUpdateImpact` it returns; `render_object::<R>(id)` reads one;
  `update_parent_data(id, f)` lends a `ParentDataSlot` (read, downcast, install, clear) and
  applies the returned impact to the node's render parent; `set_creation_site(id, site)`.
- **`flui_rendering::testing::raw_tree`** (`flui-rendering`, `testing` feature): unmarked edits
  of a bare `RenderTree` or an owner's tree, for tests that seed a state no owner operation
  builds (a one-sided child edge, a stale child id, flags cleared behind the queues) or that
  exercise a tree's own storage.

### Changed

- **`flui-view` edits the render tree only through the owner's operations**: parent-data
  application and reset, sliver slot stamps, the single-parent and batch render-child syncs,
  render-object updates and releases, the creation site and the root `RenderView`'s
  configuration. The batch sync stays one pass over frame-arena lists.
- **`testing::edit_render_object` and `testing::update_render_object`** go through
  `PipelineOwner::update_render_object`; a stale id or another type panics with one message.

### Removed

- **`PipelineOwner::render_tree_mut`** is crate-private; read the tree with `render_tree` and
  edit it through the operations above.
- **Raw `RenderTree` mutators** are crate-private: `set_children`, `drop_child`, `adopt_child`,
  `append_unlinked_child`, `get_mut`, `insert`, `insert_box`, `insert_sliver`,
  `insert_box_child`, `insert_sliver_child`, `remove_shallow`, `remove_recursive`, `set_root`,
  `clear` and `age_next_free_slot_for_test`; `iter_mut` and `reserve` are deleted.
- **Raw `RenderNode` mutators** are crate-private: `reassemble`, `as_box_mut`, `as_sliver_mut`,
  `parent_data_mut`, `set_parent_data`, `clear_parent_data`, `set_creation_site`,
  `downcast_render_object_mut`, `set_offset`, `advance_layout_generation`, `set_placed_by`,
  `paint_raw`, `clear_layout_cache`, the flag setters (`mark_layout_flag`, `mark_paint_flag`,
  `mark_composited_layer_update_flag`, `mark_paint_changed`, `set_repaint_boundary_flag`,
  `set_was_repaint_boundary`, `mark_needs_compositing`, `mark_needs_compositing_bits_update`)
  and the flag takers and clearers (`take_laid_out_since_paint`, `take_paint_changed`,
  `clear_needs_paint`, `clear_needs_composited_layer_update`, `clear_needs_layout`,
  `clear_needs_compositing`, `clear_needs_compositing_bits_update`); `as_box_unchecked`,
  `as_box_unchecked_mut`, `box_render_object_mut`, `mark_compositing_flag` and
  `layout_leaf_erased` are deleted.
- **`RenderState` flag and placement setters** are crate-private: `flags`, `mark_needs_layout`,
  `clear_needs_layout`, `clear_needs_paint`, `clear_needs_composited_layer_update`,
  `set_geometry_degraded`, `set_relayout_boundary`, `set_offset`,
  `advance_layout_generation`, `set_placed_by`; `clear_needs_compositing`,
  `set_repaint_boundary` and `set_was_repaint_boundary`, which nothing called, are deleted.
