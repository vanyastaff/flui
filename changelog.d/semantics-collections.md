### Added

- **Collections in the semantics model** (`flui-semantics`, `flui-widgets`): a node's place in a
  set (`position_in_set`, `size_of_set` as `SetSize::Known` or `Unknown`), a tree (`level`), a table
  (`TableShape`, `CellPosition` with spans, a row's `row_index`, `SortDirection`) and a container's selection rule
  (`multiselectable`), on `SemanticsConfiguration` and as `Semantics` builders. They are published
  to AccessKit; a level, a table's shape, a cell's position, a sort and a selection rule are
  checked against the roles ARIA 1.2 gives them. A level, a row and a column count from one, as
  ARIA's do. A role-less node that declares a set size publishes as a list.
- `flui-testing`: `A11yNode::size_of_containing_set`, `outline_level`, `row_count`,
  `column_count`, `row_index`, `column_index`, `row_span`, `column_span`, `sort_direction` and
  `is_multiselectable`.

### Changed

- **Places are held to the set and table they sit in.** A position past its known set's size, and a
  cell, a span or a row outside its known table, is not published ("item 8 of 3", "row 5 of 3").
  A set declared with `size_of_set` withholds the totals above it like an authored
  `SemanticsCollection` when it is unknown or holds another set's members.
- **One lazy item is one member.** An item whose root forms no node hands its position to its first
  semantics node only, and a position declared anywhere in the item, a descendant boundary
  included, suppresses the stamp.
- `IndexedSemantics::new`, `RenderIndexedSemantics::new` and
  `SemanticsConfiguration::set_collection_index` take a `usize`; `SliverSlot`,
  `SliverMultiBoxAdaptorParentData::semantic_index` and
  `SemanticSetMapping::{composed_at, interleaved}` carry `usize` and `SetSize` instead of `i32`
  and `Option<i32>`.

### Removed

- `SemanticsConfiguration::{index_in_parent, set_index_in_parent, scroll_child_count,
  set_scroll_child_count}`, replaced by `position_in_set` and `size_of_set`.
