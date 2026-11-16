### Fixed

- Android: a character typed while the application moves the caret reaches the field instead of staying in the keyboard.
- Android: an application edit outside the keyboard's composition (a formatter rewriting the text on every key) no longer restarts the keyboard, so a CJK composition and the suggestions survive it.
- Android: tapping the focused field after Back closed the keyboard shows the keyboard again.
- Android: the keyboard's Cut and Copy reach the clipboard (refused in a protected field), and an input method that monitors the extracted text is sent it on every change, within the length it asks for.
