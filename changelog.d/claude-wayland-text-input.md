### Added

- **`flui-native`**: the native Wayland host serves the focused text field to the input method
  through each seat's `zwp_text_input_v3`: the preedit composes in the field, a commit replaces
  it, a deletion around the caret removes whole characters, and the field's text, caret and
  purpose reach the input method. A compositor without `zwp_text_input_v3` gives its windows no
  text input and logs a warning. `flui-app` still picks winit on Linux.
