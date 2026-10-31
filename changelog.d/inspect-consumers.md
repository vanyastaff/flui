### Changed

- **The facade and the test harness read the typed snapshot** (`flui`, `flui-testing`):
  `flui::Inspect` (trait and derive) replaces the `flui::Diagnosticable` re-exports,
  `flui::testing::rendering::render_snapshot` replaces `render_diagnostics`, and
  `LaidOut::render_node(id)` (a one-node `SnapshotNode`, read with `get::<T>`) replaces
  `LaidOut::render_property(id, name)`, which returned the field as text. `widgets::rgba(color)`
  turns a colour into the value a colour field reads back, and `LaidOut::render_debug_field` reads the
  `Debug` text of a field its object could not type.
