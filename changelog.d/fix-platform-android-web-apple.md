### Fixed

- **Android input method stopped after 2^31 snapshots** (`flui-native`): the snapshot
  generation crosses JNI as its bit pattern, so it wraps with the counter instead of sticking at
  `i32::MAX` and dropping every later edit.
- **Android drag source stayed busy after a panic** (`flui-native`): a call posted to the
  view that panics now runs its failure path, so the drag it started ends and later drags are no
  longer refused as `Unavailable`.
- **Web backend built with `+atomics`** (`flui-native`): the backend's `Send`/`Sync` rest on a
  single-threaded page, so a shared-memory wasm32 build is now a compile error instead of a
  data race.
