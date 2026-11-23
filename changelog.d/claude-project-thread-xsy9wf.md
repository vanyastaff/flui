### Changed

- **`flui-cli`**: a project `flui create` writes declares no `cdylib`: the library is an rlib, and
  `flui build android` and `flui run` on an Android device compile it as the `cdylib`
  `NativeActivity` loads with `cargo ndk rustc --lib --crate-type cdylib`. A desktop debug build
  no longer links a second copy of the framework into a shared library on every edit, which took
  0.6–0.8 s of the 2.1 s from a one-line edit to a built counter. A project that still declares
  the `cdylib` builds as before.
- **`flui-cli`**: `flui run` reports a save after 80 ms of quiet instead of 500 ms, on the
  desktop and for an Android scene plugin alike.
- **`flui-hot-reload`**: a host reads its worker or scene artifact every 50 ms instead of every
  500 ms (`timing::ARTIFACT_POLL`, `timing::WATCHER_POLL`), so a rebuilt worker is picked up up
  to half a second sooner.
- **`flui-cli`**: a restart in `flui run` rebuilds while the application keeps running and replaces
  it only with a build that succeeded. A compile error leaves the previous build on screen
  instead of closing the window for the length of the build. Windows, which cannot overwrite a
  running executable, still stops the application first.

### Removed

- **`flui-hot-reload`**: `timing::SOURCE_DEBOUNCE` and `timing::ANDROID_SCENE_DEBOUNCE`. Source
  watching is `flui run`'s, which keeps its own debounce; nothing read these.

### Fixed

- **`flui-hot-reload`**: a rebuilt worker is no longer missed by an idle host. The frame the
  artifact watcher's wake drew read the artifact only once the driver's poll interval had passed
  since the previous frame's read, so a rebuild that landed just after a frame went unseen until
  something else drew; and a rebuild that landed while the watcher thread was starting became its
  baseline and never woke the host.
