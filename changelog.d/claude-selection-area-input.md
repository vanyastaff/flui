### Added

- `SelectionArea` selects with a mouse, pen or trackpad: a drag by character, a double click by word, a triple click a whole text, Shift+click extends.
- `SelectionArea` keys: Shift with the arrows, Home and End move the moving end; select all, copy and Esc. Selecting takes the focus, and a blur keeps the selection with the inactive colour.
- A text that leaves a `SelectionArea` while holding an end of its selection hands the end to the selected text beside it; the selection clears once none is left.
