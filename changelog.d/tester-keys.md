### Added

- `Tester::press_keys` presses key chords in turn to the focused element (`"Control+a"`, `"Shift+Tab"`, `"Enter"`), and `Tester::key_down` / `key_up` hold a key across chords or through its auto-repeats. Each has a `try_` twin.

### Changed

- `input.inject`: pressing a key the keyboard source already holds sends that key's auto-repeat, as WebDriver dispatches a second `keyDown`; before, it sent nothing.
