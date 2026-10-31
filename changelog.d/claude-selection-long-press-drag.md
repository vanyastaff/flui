### Added
- `SelectionArea`: after a touch long press selects a word, moving the same finger extends the selection by words, keeping that word selected, and the handles follow. The changes are reported as `SelectionChangeCause::User(SelectionInput::Touch)`.
