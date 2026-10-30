### Added

- **`flui-widgets`**: `SelectionController`, given to a `SelectionArea` with
  `SelectionArea::controller`, reads and moves the area's selection from outside: `texts` lists
  the presented texts in reading order, `select` takes a `DocumentSelection` and refuses ends the
  area does not present with a typed `SelectError`, `select_all` and `clear` do the obvious, and
  `listenable` notifies after every change. A controller's selection does not take the focus and
  hides the touch handles.

### Changed

- **`flui-widgets`**: `SelectionArea::on_selection_changed` receives a `SelectionChange`: the
  selected text and a `SelectionChangeCause` saying whether the user made the change (by pointer,
  touch, keyboard or assistive technology), a controller made it, or a text that held an end left.
