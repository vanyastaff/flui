### Changed

- `flui-rendering` `RenderError::InvalidConstraints` names the render object and a static reason (`invalid_constraints(render_object, reason)`), as `InvalidGeometry` does, instead of a message string.
- `flui-rendering` `RenderError::SemanticsError { message }` is `RenderError::Semantics(SemanticsError)`: the semantics pass's refusal is a typed enum (non-finite sort key, duplicate region key, exhausted region identities, missing or unformed root) instead of its `Display` text.

### Removed

- `flui-rendering` `RenderError` variants that nothing constructed: `NotAttached`, `AlreadyAttached`, `ConfigurationNotSet`, `InvalidParentChild`, `CycleDetected`, `LayoutDuringPaint`, `LayoutDetached`, `RelayoutBoundaryViolation`, `PaintDetached`, `LayerError`, `PhaseOrderViolation`, `RootNotSet`, `CompositingError`, `SemanticsNotEnabled`, `LayoutDepthExceeded`, `ChildIndexOutOfBounds`, with the constructors `relayout_boundary`, `layer`, `compositing`, `semantics`, `layout_depth_exceeded` and `child_index_out_of_bounds`.
