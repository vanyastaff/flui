### Added

- Web accessibility: a focused text field is mirrored as a native `<input>` or `<textarea>`, and the window's text input edits the field's store through it, so a screen reader reads and edits the field the input method edits. The store stays the source of truth; when the field loses focus or goes away, the store is served on the canvas again.
- Web text input: an editor tells the browser what to fill a field with (`autocomplete` from the field's purpose) and labels the virtual keyboard's Enter key (`enterkeyhint`).
- `InMemoryTextStore::set_text_input`, so a test can give the store what it asks of the input method, and `InMemoryTextStore::app_select`, so a test can select as the application.
