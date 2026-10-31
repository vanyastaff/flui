### Changed

- iOS text input: the keyboard's responder edits the focused field through the shared text-store host, the one macOS serves it through. Edits are applied through the field's store, which also answers selection, marked text, ranges and their rectangles. The keyboard learns about the application's own edits, and about the field's status (secure entry, keyboard type, autocorrection) from the field's store. Return and Backspace still reach the field as key presses, so a single-line field submits on Return.
