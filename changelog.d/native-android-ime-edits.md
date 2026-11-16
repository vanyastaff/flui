### Added

- `TextStoreHost::show_keyboard` and `TextInputHandle::show_keyboard`: a text field's own tap asks the platform for its soft keyboard. Every host implements it; only Android shows one, the others' platforms show it by themselves or have none.

### Changed

- `EditableText`'s text-store `document_bounds` is the field's box, not its text and caret: an empty field reports the area it covers, and text scrolled past the box does not widen it.

### Fixed

- Android: a character typed while the application moves the caret reaches the field, and an edit inside a batch the keyboard has not ended survives a snapshot that arrives in the middle of it.
- Android: an application edit made with no composition open, or outside the composition, no longer restarts the keyboard, so the composition and the suggestions survive it.
- Android: tapping the focused field after Back closed the keyboard shows the keyboard again; a drag from the field or a tap on something drawn over it does not.
- Android: the keyboard's Cut and Copy reach the clipboard (refused in a protected field), and an input method that monitors the extracted text is sent it on every change, within the length it asks for.
