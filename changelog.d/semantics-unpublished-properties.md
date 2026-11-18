### Removed

- **`flui-semantics`**: the Flutter properties no platform bridge published and no widget set:
  `AttributedString`, `StringAttribute`, `StringAttributeType`, `SemanticsTag`,
  `SemanticsHintOverrides`, `AccessibilityFocusBlockType`, `DebugSemanticsDumpOrder`, and the
  `SemanticsConfiguration` increased and decreased values, tags, hint overrides, scroll index,
  platform view id and text lengths. They leave `flui::semantics` with them.
- **`flui-semantics`**: `SemanticsOwner::announce`. An `Announcer` from
  `Announcements::announcer` (a widget's `LifecycleContext::announcer`) is the one way to speak.
- **`flui-semantics`**: `SemanticsNode::{element_id, set_element_id, with_element_id}`,
  `SemanticsTree::{insert_with_element, iter_dirty, semantics_ids}`, the `prelude` module and
  `VERSION`; nothing read them.
- **`flui-rendering`**: `debug_dump_semantics_tree`, which printed a placeholder instead of the
  tree. `PipelineOwner::semantics_owner` reads the assembled tree.

### Changed

- **`flui-semantics`**: label, value, hint, tooltip, placeholder, keyboard shortcut, language,
  URL, role description and custom-action labels take `impl AsRef<str>` and read back as
  `&str`, so `smol_str` no longer appears in the public API. `SemanticsSortKey` keeps its fields
  private behind `order()` and `name()`; `SemanticsSortKey::named` takes `impl AsRef<str>`.
  `TextRunParts::new` takes `impl AsRef<str>`, and the run's text is read through `TextRun::text`.
- **`flui-widgets`**: `Semantics::{label, value, hint}` and `SliverSemantics::label` take
  `impl AsRef<str>`.
