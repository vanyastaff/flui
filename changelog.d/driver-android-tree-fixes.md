### Changed

- **`flui-driver`**: the Android lane refuses a `uiautomator dump` while any accessibility
  service is on, not only a screen reader, and names the services in the refusal
  (`not_supported`, `android_window_dump`): the dump suspends Voice Access, Switch Access and
  Samsung's Universal Switch as it does TalkBack. `Window::os_tree` and
  `Window::compare_projection` read the app's tree again when it moved while the OS tree was
  read, and answer `Error::Busy` when it moved on each of three readings.

### Fixed

- **`flui-driver`**: the Android lane's OS tree on a correct app. A device whose last
  accessibility service was turned off (an empty service list) no longer refuses every dump, and
  with it the lane's window, capture and input. The app's root is the decor view's child where
  `FluiView.install` puts it, so an edge-to-edge window is no ambiguity with NativeActivity's own
  content view. A radio button matches the checked state the adapter publishes for its selection.
  The rows a list keeps past its viewport match in the order the adapter adds them, so two
  unnamed rows past the display are no ambiguity. A range's value and the announcer nodes, which
  a dump cannot show, are no difference.
