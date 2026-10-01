### Added

- **`flui_painting::text_boundaries`**: the grapheme and word boundaries the painter clusters
  text by and snaps a tap to (ICU4X, UAX #29): `previous_grapheme_boundary`,
  `next_grapheme_boundary`, `is_grapheme_boundary`, `graphemes`, and `word_segments` /
  `word_segments_from` yielding `WordSegment`s. Each query segments from the start of the line
  that holds its offset, so it costs a line, not the whole text.

### Changed

- **`TextEditingController` steps through the painter's boundaries** (`flui-widgets`): the arrow
  keys, Backspace, Delete, the selection setters' snap and the Ctrl/Alt word jumps walk
  `flui_painting::text_boundaries`, and an obscured field's mask counts the same clusters, so
  a caret the keyboard places is one a tap can place
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §6).

### Removed

- **`unicode-segmentation`** is no longer a dependency of `flui-widgets` or the workspace, and
  `deny.toml` bans it for every FLUI crate.
