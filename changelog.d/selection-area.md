### Added

- `SelectionArea` in `flui-widgets`: static `Text` and `RichText` beneath it become selectable
  across paragraphs in reading order. A mouse or pen drag selects characters, a double or triple
  click words or paragraphs, Shift+click extends; a touch long press selects a word and shows two
  handles that follow their ends through scrolls; Shift with the arrows, Home and End extends the
  selection, Ctrl/Cmd+A selects everything, Ctrl/Cmd+C copies and Escape clears. A context menu
  offers Copy, Select all and the area's own commands.
- `SelectionController` reads and sets an area's selection without a pointer (`selection`,
  `content`, `paragraphs`, `select`, `select_all`, `clear`), with `AreaSelection`,
  `SelectionPoint`, `SelectableId`, `SelectedContent` and `SelectionError`.
- `SelectionContainer::disabled` keeps a subtree's text out of the enclosing area.
- `SelectionHandles` and `HandleSide` draw the touch handles; `DotHandles` is the theme-free
  default.
- `RenderParagraph::set_selection` and `set_selection_color` in `flui-objects` paint a selection
  behind the glyphs.
