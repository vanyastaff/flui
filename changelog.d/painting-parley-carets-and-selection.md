### Changed

- `TextPainter`'s caret, selection, hit-test, line-metric and word-boundary queries
  (`get_offset_for_caret`, `get_boxes_for_selection`, `get_position_for_offset`,
  `get_line_metrics`, `get_word_boundary`) read the Parley layout that measured and painted,
  in the painted box's coordinates, and never build the process font system
  ([ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) §10 step 5). Signatures are
  unchanged.
- `get_position_for_offset` answers a grapheme boundary: a tap over `e` and its combining mark,
  a ZWJ emoji sequence or a CR LF lands before or after it, never inside. A caret query stays per
  scalar, a proportional slice inside a multi-scalar cluster.
- A selection box carries its bidi run's direction (`TextBox::direction`), not the paragraph's,
  and covers one stretch of one direction on one line.
- At a soft wrap, a `Downstream` caret sits at the next line's start and an `Upstream` one at
  the previous line's end; after a hard break, a trailing one included, the caret starts the
  next line.
- Caret and hit queries on truncated text stay in the kept text: an offset in dropped lines or
  in the ellipsis answers the kept text's end.
- `get_line_metrics`: `LineMetrics::left` is the line's first painted x in the paragraph's box
  (it was always 0), so a short line aligned right reports where it starts; `hard_break` is true
  on a line ending at an explicit break or at the end of the paragraph (it was true on every
  line), and false on the last kept line of truncated text.
- Double-tap word boundaries come from ICU4X's word segmenter (non-complex scripts) instead of
  `unicode-segmentation`; the tie-break is unchanged.
- `FontCollection::register_font` and `check_font` load and judge bytes on the collection alone;
  a registration no longer reaches the process font system.

### Removed

- `flui_painting::TextLayout`, `Shaper` and `ResolvedFont`, and `SharedFontSystem::shape`,
  `SharedFontSystem::generation` and the `testing` door `SharedFontSystem::register_font`.
  Carets and selection come from `TextPainter`; register fonts through
  `FontCollection::register_font` or `flui::register_font`.
- The `flui_painting::Family` re-export of `cosmic_text::fontdb::Family`: no public signature
  names it.

### Fixed

- Carets and selection boxes on multi-line text: a selection on a line after the first (`3..5`
  in `"ab\ncd"`) had no box, and a caret after a trailing newline sat on the line before it.
