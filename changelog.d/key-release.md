### Added

- `KeyEvent::release()` and `KeyEvent::with_release(KeyRelease)` say whether a key-down's release will arrive. A host that posts a character with no key, such as Windows' Alt+numpad entry, marks it `KeyRelease::Never`.

### Fixed

- A character typed with Alt+numpad on Windows no longer stays held in the focus manager's key record, so the next loss of activation no longer cancels a key on the old focus's path.
