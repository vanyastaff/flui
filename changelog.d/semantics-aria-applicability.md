### Changed

- **A state or property the declared role does not support is not published** (`flui-semantics`,
  WAI-ARIA 1.2): checked or pressed, selected, expanded, read-only, required, modal, a numeric
  range, a placeholder or a heading level on a role that does not carry it is left out of the
  AccessKit update. The frame reports each with `tracing::warn!` once per change of its node,
  after the pipeline checkout is released. A node with no declared role is not checked.
  `flui-testing` fails a frame that left one out.
- **`FilterChip`** (`flui-material`) publishes its selection as a button's pressed state, and a
  **`ListTile`** marked with the new `selectable(true)` is a list-box option that declares whether
  it is selected; any other tile declares no selection, so `selected` alone only recolors it.

### Added

- `flui_semantics::{RoleAttribute, InapplicableAttribute, report_inapplicable}`,
  `SemanticsOwner::{pending_diagnostic, retire_diagnostic}` and
  `PipelineOwner::{pending_semantics_diagnostic, retire_semantics_diagnostic}`; with the `testing`
  feature, `SemanticsOwner::inapplicable_last_flush`.
