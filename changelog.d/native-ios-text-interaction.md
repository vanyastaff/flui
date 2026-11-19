### Added

- On iOS, a focused text field gets UIKit's caret, selection highlight, handles, loupe and edit
  menu: a `UITextInteraction` is put over the field while it has the keyboard and taken off when
  it loses it. Its gestures begin only where a press lands on the field by the window's own hit
  test, so a control drawn over the field keeps its tap, and on its selection's handles. The field
  then draws none of these itself and its own taps, long presses and drags stay out of it; it keeps
  drawing its text and its composition's underline. The rectangles UIKit draws are cut to the
  field's box and asked for again when the field moves.
- `TextStoreHost::selection_ui` and `SelectionUi` in `flui-platform`: whether the host's platform
  draws the served field's selection UI; `TextInputHandle::selection_ui` reads it for an attached
  field.
- `TextStore::edit_command_enabled`, `TextStore::perform_edit_command` and `EditCommand`: a
  platform's edit menu asks the focused field for Cut, Copy, Paste, Select (the word at the caret)
  and Select All, which `EditableText` performs with its own clipboard and selection rules. On iOS
  a command reaches only the field the menu was offered over, and Paste is offered while the
  pasteboard holds text.
- `TextStoreRead::is_hit_at`: whether a press at a point lands on the field; `EditableText`
  answers it with the window's hit test.
- `flui-testing`: `RecordingTextStoreHost::with_selection_ui`,
  `HeadlessWindow::with_text_store_host_drawing`, `harness::mount_with_selection_ui_on` and
  `lay_out_with_selection_ui_on`.

### Fixed

- On iOS, `selectionRectsForRange:` answers one rectangle per line of the range, with its writing
  direction and which rectangles hold the range's start and end, instead of none.
