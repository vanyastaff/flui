### Added

- **Collections in the semantics model** (`flui-semantics`, `flui-widgets`): a node's place in a
  set (`position_in_set`, `size_of_set` as `SetSize::Known` or `Unknown`), a tree (`level`), a table
  (`TableShape`, `CellPosition` with spans, `SortDirection`) and a container's selection rule
  (`multiselectable`), on `SemanticsConfiguration` and as `Semantics` builders. They are published
  to AccessKit; a level, a table's shape, a cell's position, a sort and a selection rule are
  checked against the roles ARIA 1.2 gives them. A level, a row and a column count from one, as
  ARIA's do.
- `flui-testing`: `A11yNode::size_of_containing_set`, `outline_level`, `row_count`,
  `column_count`, `row_index`, `column_index`, `row_span`, `column_span`, `sort_direction` and
  `is_multiselectable`.

### Changed

- **A lazy list's size is its container's.** The scroll container declares the set's size and
  publishes as a list; its rows declare only their position. Before, the size was stamped on each
  row, where no adapter reads it, so "item 12 of 100" never reached a screen reader. A list whose
  length is unknown publishes no size, and its rows publish no position under an enclosing set of
  known size, which would otherwise read "item 12 of 5". A hand-numbered row inside a lazy list now
  announces its own number against the delegate's size.
- `IndexedSemantics::new` and `RenderIndexedSemantics::new` take a `usize`; `SliverSlot`,
  `SliverMultiBoxAdaptorParentData::{semantic_index, semantic_set_size}` and
  `SemanticSetMapping::{composed_at, interleaved}` carry `usize` and `SetSize` instead of `i32`.

### Removed

- `SemanticsConfiguration::{index_in_parent, set_index_in_parent, scroll_child_count,
  set_scroll_child_count}`, replaced by `position_in_set` and `size_of_set`.
