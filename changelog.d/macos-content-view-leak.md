### Fixed

- **`flui-native`**: a closed macOS window no longer leaks its content view. The view's
  `alloc`/`init` reference was never released, so every window that closed kept its view and the
  input and text state it carries for the rest of the process. The window now owns the view and
  releases it after its own teardown.
