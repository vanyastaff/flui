### Added

- **Typed inspection of the element, render and layer trees** (`flui-view`, `flui-rendering`,
  `flui-layer`): each tree implements `flui_foundation::inspect::InspectTree`, so `snapshot`
  reads it as typed fields. An element node is named by its view and links its render object. A
  render node carries its protocol, constraints, size or sliver geometry, offset and dirty flags,
  and links the semantics node it contributes. A layer node carries its own fields and links the
  boundary's render object. `PipelineOwner::inspect` gives the render tree's view;
  `LaidOut::inspect_elements`, `inspect_render` and `inspect_layers` (`flui-testing`) read a
  mounted app.
- **`View::inspect`** (`flui-view`): a provided method through which a view describes its
  configuration in the element tree's snapshot. `#[view(inspect)]` on `#[derive(StatelessView)]`
  or `#[derive(StatefulView)]` fills it from the type's own `#[derive(Inspect)]`.
- `flui_foundation::inspect::kind_of` and `kind_args_of` read a kind and its type arguments from
  a `type_name`, for trees whose nodes are erased.

### Changed

- `flui_foundation::inspect::snapshot` answers `Result<SnapshotNode, SnapshotError>`: `Empty` for
  a tree with no root, `Gone(id)` for a start node that is no longer in the tree, which includes an
  id whose slot now holds a newer node.
