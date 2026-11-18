### Added

- On iOS, a focused text field gets UIKit's caret, selection highlight, handles, loupe and edit
  menu: a `UITextInteraction` is put over the field while it has the keyboard and taken off when
  it loses it, and its gestures begin only over the field and on its selection's handles, so
  touches elsewhere stay the application's. The field then draws none of these itself and its own
  taps, long presses and drags stay out of it; it keeps drawing its text and its composition's
  underline.
- `TextStoreHost::selection_ui` and `SelectionUi` in `flui-platform`: whether the host's platform
  draws the served field's selection UI; `TextInputHandle::selection_ui` reads it for an attached
  field.
- `TextStore::edit_command_enabled`, `TextStore::perform_edit_command` and `EditCommand`: a
  platform's edit menu asks the focused field for Cut, Copy, Paste and Select All, which
  `EditableText` performs with its own clipboard and selection rules.
- `flui-testing`: `RecordingTextStoreHost::set_selection_ui`, `lay_out_with_ime_on`,
  `LaidOut::store_host` and a public `Harness::store_host`.

### Fixed

- On iOS, `selectionRectsForRange:` answers one rectangle per line of the range, with its writing
  direction and which rectangles hold the range's start and end, instead of none.
