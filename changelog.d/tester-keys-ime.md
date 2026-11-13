### Added

- `Tester::press_keys` presses key chords in turn to the focused element (`"Control+a"`, `"Shift+Tab"`, `"Enter"`), and `Tester::key_down` / `key_up` hold a key across chords or through its auto-repeats.
- `Tester::compose`, `commit_composition` and `cancel_composition` drive the focused field's input-method composition, with the caret at a character or hidden. Each new verb has a `try_` twin.
- Agent protocol 0.12: an `ime` step `set_composition` takes `caret_hidden`, the composition an input method shows without a caret while it converts.

### Changed

- `input.inject`: pressing a key the keyboard source already holds sends that key's auto-repeat, as WebDriver dispatches a second `keyDown`; before, it sent nothing.
