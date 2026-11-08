### Changed

- `WidgetsBinding::set_on_need_frame` accepts owner-local callbacks without `Send + Sync` bounds.

### Fixed

- A frame callback can install its replacement during invocation without deadlocking or losing the replacement.
