### Added

- **`RenderTree::set_children`** (`flui-rendering`): makes a list the whole child list of a
  parent, moving each listed child from its donor and orphaning each dropped one, and returns a
  `ChildrenUpdate` naming what changed. Both directions of every edge are written together and
  each moved subtree takes the depth of its new position.
- **`PipelineCounters::layout_mark_steps`** (`flui-rendering`): render nodes the
  layout-invalidation walks marked.

### Fixed

- **Element render-child sync wrote render edges by hand** (`flui-view`): the element tree's
  render-child synchronization set parent links and child lists separately, so a cross-parent
  move left the moved nodes at their old depth. It now goes through `RenderTree::set_children`,
  top-down, so the layout queue's shallow-first order sees their real depth.
- **Sibling layout marks walked the same ancestors each time** (`flui-rendering`):
  `mark_needs_layout` on N siblings walked to the relayout boundary N times. A walk now stops
  where an earlier walk already settled the path, so the cost is O(N + depth). Layout passes,
  intrinsic and dry-layout queries, queue evictions and reparenting all forget the settled paths,
  and walks made during layout never stop early.
