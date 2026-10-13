### Changed

- **A state or property the declared role does not support is not published** (`flui-semantics`,
  WAI-ARIA 1.2): checked or pressed, selected, expanded, read-only, required, modal, a numeric
  range, a placeholder or a place in a set on a role that does not carry it is left out of the
  AccessKit update and reported once per change of its node with `tracing::warn!`. A node with no
  declared role is not checked. `flui-testing` fails a frame that reported one.
- **`ListTile`** (`flui-material`) declares `selected` only while selected.

### Added

- `flui_semantics::RoleAttribute` and `InapplicableAttribute`, and with the `testing` feature
  `SemanticsOwner::inapplicable_last_flush`.
