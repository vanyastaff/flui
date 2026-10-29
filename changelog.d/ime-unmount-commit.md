### Added
- `UiRuntime::commit_composition_for_close` commits the composition of a presentation's focused field, keeping its text, for an accepted close to run before its close guard and save.

### Fixed
- An `EditableText` unmounted while an input method composes in it (a route the program changed) keeps the preedit as committed text instead of leaving the controller composing; `on_changed` is not called once dispose begins, and a conversion the input method still had queued is dropped.
- A form reset while an input method composes shows the field's initial value without the preedit, also when the committed text already equals that value.
