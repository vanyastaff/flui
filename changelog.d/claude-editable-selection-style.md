### Changed

- **`flui-widgets`**: `EditableText` highlights its selection with the nearest
  `DefaultSelectionStyle` (else `SelectionStyle::default()`): the active colour while focused,
  the inactive one after a blur, which keeps the selection, as in a `SelectionArea`. A field's
  selection used to paint transparent unless a colour was set. `EditableText::selection_style`
  replaces `selection_color`.

### Added

- **`flui-material`**: `ThemeData::selection`, `primary` at 40 % and `on_surface` at 16 % by
  default; `Theme` provides it as the inherited `DefaultSelectionStyle`, so Material fields and
  selection areas take the theme's colours.
