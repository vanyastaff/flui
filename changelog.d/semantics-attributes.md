### Added

- **`SemanticsAttributes`** (`flui-semantics`, ADR-0202): what one node publishes, read through
  `SemanticsConfiguration::attributes`; publication reads it in place.

### Removed

- **Duplicate semantics shapes** (`flui-semantics`, `flui-rendering`, `flui-objects`,
  `flui-widgets`, `flui`): `SemanticsProperties`, `SemanticsConfiguration::from_properties`,
  `Semantics::from_properties`, `RenderSemanticsAnnotations::new` (use `from_configuration`),
  `SemanticsNodeData`, `SemanticsNode::to_node_data`, `SemanticsTree::node_data`,
  `SemanticsTreeUpdate`, `SemanticsTreeUpdateBuilder`, `SemanticsSnapshot`,
  `SemanticsNodeSnapshot`, `SemanticsSnapshotError` and `SemanticsOwner::snapshot`. The
  platform handoff is the AccessKit `TreeUpdate` (`tree_to_update`, the owner's callback).
