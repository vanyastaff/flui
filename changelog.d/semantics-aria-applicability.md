### Changed

- **A state or property the declared role does not support is not published** (`flui-semantics`,
  WAI-ARIA 1.2): checked or pressed, selected, expanded, read-only, required, modal, a numeric
  range, a placeholder or a heading level on a role that does not carry it is left out of the
  AccessKit update and routes no action. The frame reports each with `tracing::warn!` once per
  change of its node, after the frame releases the pipeline checkout and the widgets binding's
  lock. A node with no declared role is not checked.
  `flui-testing` fails a frame that left one out.
- **`FilterChip`** (`flui-material`) publishes its selection as a button's pressed state, and a
  **`ListTile`** marked with the new `selectable(true)` is a list-box option that declares whether
  it is selected; any other tile declares no selection, so `selected` alone only recolors it.

### Added

- `flui_semantics::{RoleAttribute, InapplicableAttribute, report_inapplicable}`,
  `SemanticsOwner::take_diagnostics`,
  `PipelineOwner::{take_semantics_diagnostics, restore_semantics_diagnostics}` and
  `PipelineCell::report_semantics_diagnostics`, `PipelineOwner::restore_layer_tree`; with the `testing`
  feature, `PipelineOwner::take_unchecked_semantics_diagnostics`.
