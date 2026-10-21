### Fixed

- **A window's state could go back to an older snapshot** (`flui-native`): when two threads read
  `PlatformWindow::state()` at once, the read that began first could publish its sample after the
  other had published a newer one, so the snapshot fell behind the window until the next change,
  and a visibility callback could see `state().visible` disagree with the value it was given. A
  read overtaken by a later one now publishes nothing.
