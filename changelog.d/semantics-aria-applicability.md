### Changed

- **A state or property the declared role does not support is not published** (`flui-semantics`,
  WAI-ARIA 1.2): checked or pressed, selected, expanded, read-only, required, modal, a numeric
  range, a placeholder, a position in a set or a heading level on a role that does not carry it is
  left out of the AccessKit update and reported once per change of its node with
  `tracing::warn!`, after the publication is committed. A node with no declared role is not
  checked. `flui-testing` fails a frame that reported one.
- **`ListTile`** (`flui-material`) declares `selected` only while selected, and a tappable tile is
  a list-box option while selected and a button otherwise.

### Added

- With the `testing` feature: `SemanticsOwner::inapplicable_last_flush`, `InapplicableAttribute`
  and `RoleAttribute` (`flui-semantics`).
