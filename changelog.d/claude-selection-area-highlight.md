### Added

- **`flui-widgets`**: `SelectionArea` highlights its selection behind the selected glyphs of each
  text it covers. `SelectionStyle` holds the active and inactive (dimmed, after a blur) colours,
  set per area with `SelectionArea::style` or for a subtree with `DefaultSelectionStyle`.
- **`flui-objects`**: `RenderParagraph::set_selection` and `set_selection_color` paint a
  selection highlight behind the paragraph's glyphs.
