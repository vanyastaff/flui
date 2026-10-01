### Added

- **`flui_painting::text_boundaries`**: the grapheme and word boundaries the painter clusters
  text by and snaps a tap to (ICU4X, UAX #29): `previous_grapheme_boundary`,
  `next_grapheme_boundary`, `is_grapheme_boundary`, `graphemes`, and `word_segments` /
  `word_segments_from` yielding `WordSegment`s. Each query segments from the start of the line
  that holds its offset, so it costs a line, not the whole text.

### Changed

- **The host's fonts load off the owner thread** (`flui-app`, `flui-painting`): an app's first
  frame no longer waits for the host font scan and feed (about 43 ms on a Windows desktop). It
  renders with the bundled faces, and the host's faces arrive from a `flui-host-fonts` thread;
  text laid out in a fallback face (a family or script only the host carries) is laid out again
  at the next frame after they land, as after `register_font`. A build without `bundled-fonts`
  still feeds the host's faces before the first frame, since it has no face of its own
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §7).
- **`FontCollection::with_host_feed` and `HostFontFeed`** (`flui-painting`): a collection with
  the bundled faces now, and the feed that scans the host and adds its faces when run on
  another thread, raising the collection's generation once if it added anything.
  `FontCollection::with_host_fonts` still feeds synchronously. Both register one font file at a time, so a host-fed collection
  keeps about 0.15 MiB of heap instead of 2.4 MiB.
- **`TextEditingController` steps through the painter's boundaries** (`flui-widgets`): the arrow
  keys, Backspace, Delete, the selection setters' snap and the Ctrl/Alt word jumps walk
  `flui_painting::text_boundaries`, and an obscured field's mask counts the same clusters, so
  a caret the keyboard places is one a tap can place
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §6).

### Removed

- **`unicode-segmentation`** is no longer a dependency of `flui-widgets` or the workspace, and
  `deny.toml` bans it for every FLUI crate.
