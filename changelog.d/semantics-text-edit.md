### Added

- `EditableText` publishes its text as runs from its layout, one per stretch of one direction on
  one line: a screen reader reads a field by character, word and line, with the caret and
  selection. A field that hides its input publishes the mask alone.
- A screen reader can select text in an `EditableText` and replace the selection. Both go through
  the keyboard's edit path: the composition is committed first, `on_changed` is called, and a
  selection made on the mask selects the characters behind it.
- `TextPainter::line_layouts`, with `TextLineLayout`, `TextLineRun` and `TextCharacter`: each
  line's runs of one direction in logical order, with their grapheme clusters where they are
  painted.
- `text_boundaries::word_starts`, `next_word_start` and `previous_word_start`: the word stops the
  caret moves between, which a field also publishes as its word starts.

### Changed

- `RenderEditable::set_selection` also asks for a semantics update.
