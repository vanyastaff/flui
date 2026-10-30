### Added

- **`flui-widgets`**: `FocusableSliver` makes a `SliverList`, `SliverGrid` or
  `SliverFixedExtentList` among a `CustomScrollView`'s slivers one Tab stop with arrow-key focus
  over its rows, as `ListView::focusable` does for a list. `GridView::focusable` and
  `GridView::empty_label` do the same for a grid, with two-dimensional arrows: a row of tiles along
  the scroll axis, along the row across it, and a column past a short last row lands on its last
  tile. `CustomScrollView::position` lets a `Scrollable` drive a custom scroll view.
- **`flui-view`**: `SliverMultiBoxAdaptor::map_children`, a low-level hook that wraps every child
  a lazy sliver builds, and `SliverMultiBoxAdaptor::resolved_item_count`.
- **`flui-rendering`**, **`flui-objects`**: `PipelineOwner::sliver_layout` reads a sliver's
  committed constraints and geometry; `RenderSliverSubtreeAnchor` is the sliver twin of
  `RenderSubtreeAnchor`.
