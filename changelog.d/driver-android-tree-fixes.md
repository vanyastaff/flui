### Changed

- **`flui-driver`**: `Window::compare_projection`, and `Window::os_tree` through a lane that
  matches against the app's tree, compare the OS tree with the app's tree of its own revision, on
  every lane: the app's tree is read again while it moves between the two reads, and when the
  lane could not match a tree that moved, by the rule the rustdoc of `Window::compare_projection`
  states. A tree that never held still answers `Error::Busy` with `Retry::Never`, which an app
  republishing every frame (a running clock, a progress bar) always does on Android.
