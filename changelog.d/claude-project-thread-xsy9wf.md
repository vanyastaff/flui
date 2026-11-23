### Added

- **`flui-cli`**: `flui run --json` reports `run.watch {paths}` once the sources are watched; a
  save made after it is a change the run sees.

### Changed

- **`flui-cli`**: a project `flui create` writes declares no `cdylib`: the library is an rlib, and
  `flui build android` and `flui run` on an Android device compile it as the `cdylib`
  `NativeActivity` loads with `cargo ndk rustc -p <package> --lib --crate-type cdylib`. A desktop
  debug build no longer links a second copy of the framework into a shared library on every
  edit, which took 0.6–0.8 s of the 2.1 s from a one-line edit to a built counter. A project that
  still declares the `cdylib` builds the same library; one whose workspace's default members are
  several packages now builds the library of the package in the project's directory.
- **`flui-cli`**: `flui run` reports a save after 80 ms of quiet, instead of 500 ms on the
  desktop and 300 ms for an Android scene plugin, and the saves reported while a rebuild runs are
  one rebuild after it, not one each.
- **`flui-hot-reload`**: a host reads its worker or scene artifact every 50 ms instead of every
  500 ms (`timing::ARTIFACT_POLL`, `timing::WATCHER_POLL`), so a rebuilt worker is picked up up
  to half a second sooner. An artifact that failed to load is opened again once it changes, or
  after a second, not on every read.
- **`flui-cli`**: a save in `flui run`, and `r`, rebuild while the application keeps running and
  replace it only with a build that succeeded. A compile error leaves the previous build on
  screen instead of closing the window for the length of the build. Windows, which cannot
  overwrite a running executable, still stops the application first, and `R` still stops,
  rebuilds and starts.

### Removed

- **`flui-hot-reload`**: `timing::SOURCE_DEBOUNCE` and `timing::ANDROID_SCENE_DEBOUNCE`. Source
  watching is `flui run`'s, which keeps its own debounce; nothing read these.

### Fixed

- **`flui-hot-reload`**: a rebuilt worker is no longer missed by an idle host. The frame the
  artifact watcher's wake drew read the artifact only once the driver's poll interval had passed
  since the previous frame's read, so a rebuild that landed just after a frame went unseen until
  something else drew; and a rebuild that landed between making the hook and its watcher thread
  starting became the watcher's baseline and never woke the host.
- **`flui-cli`**: an application that exits while `flui run` rebuilds beside it is reported as
  that exit (`run.app.exit` with its code, and its last output), before the build that replaces
  it starts.
- **`flui-cli`**: on Linux a build no longer starts the next one. `flui run` took the read of
  `Cargo.toml` that every build makes, which Linux reports as an open, for an edit, so after each
  build it rebuilt and restarted the application again.
- **`flui-cli`**: `flui run` keeps seeing edits to `Cargo.toml` after an editor saves it by
  renaming a new file over it, and an overflowing event queue is a change to every watched path
  instead of a lost one. A worker host that exited while the worker built is reported as that
  exit before its successor starts.
