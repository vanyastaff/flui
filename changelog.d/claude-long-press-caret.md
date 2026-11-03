### Added

- `LongPressSelection { Word, Caret }` says what a touch long press in an `EditableText` selects. It is set with `EditableText::long_press` or `DefaultSelectionControls::long_press`, and defaults to `LongPressSelection::for_platform`: iOS moves a caret under the finger, other platforms select the word.
- A long press in an `EditableText` shows the magnifier over the end it moves while the touch is down, following the finger.
