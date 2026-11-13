### Added

- `Tester::press_keys` presses key chords in turn to the focused element (`"Control+a"`, `"Shift+Tab"`, `"Enter"`), and `Tester::key_down` / `key_up` hold a key across chords or through its auto-repeats.
- `Tester::compose`, `commit_composition` and `cancel_composition` drive the focused field's input-method composition, with the caret at a character or hidden.
- A `Tester`'s application keeps its documents in a `MemoryStorage` (`Tester::storage`, now also `flui::testing::MemoryStorage`); `TestWindow::storage` launches over a given one and `TestWindow::without_storage` gives none. `Tester::restart` launches the same view again over that storage.
- `Tester::request_close`, `is_closed`, `end_session` and `set_window_focused` play the window's close, the session's end and the window's focus as the operating system reports them. Each new action has a `try_` twin where it can fail.
- Agent protocol 0.12: an `ime` step `set_composition` takes `caret_hidden`, the composition an input method shows without a caret while it converts.

### Changed

- `input.inject`: pressing a key the keyboard source already holds sends that key's auto-repeat, as WebDriver dispatches a second `keyDown`; before, it sent nothing.
- `TestWindow` is `Clone` but no longer `Copy` or `PartialEq`, since it carries the storage.
