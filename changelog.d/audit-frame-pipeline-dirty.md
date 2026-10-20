### Changed

- **`RenderNode::set_depth` is removed and `NodeLinks::set_depth` is crate-private** (`flui-rendering`): a render node's depth is
  written by the render tree's edge primitives, so nothing outside can desynchronize it.

### Fixed

- **Layout ran children before their parents** (`flui-rendering`): render nodes mounted by
  elements (inserted parentless, then adopted) all had depth 0, so the layout queue's
  shallow-first order was mark order. A relayout boundary marked before its ancestor laid out
  against stale constraints and again under the ancestor's new ones. `RenderTree::adopt_child`
  and `drop_child` now derive the depth of the moved subtree, and each layout pass re-keys its
  queue from current depths.
- **A layout mark made during paint went unnoticed** (`flui-rendering`): it is now logged,
  still applied next frame, and asserted against by `run_paint` in debug builds.
- **Reactive graph ids could wrap** (`flui-view`): after the last id the counter refuses new
  graphs instead of reusing an id a stale `SignalSlot` could validate against.
- **Per-frame allocation and scope walks in the dirty queues** (`flui-rendering`, `flui-view`):
  the layout batch, the build heap's depth re-key and paint's child list reuse their storage;
  layout-builder scope classification walks each ancestor once per repartition; the layout
  walk's result sinks no longer take a lock per node.
