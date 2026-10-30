### Added

- **`flui-widgets`**, **`flui-material`**: `autofocus(bool)` on `EditableText`, `RawTextField`,
  `RawTextFormField`, `TextField` and `TextFormField`. The field takes the focus when it mounts,
  as `Focus::autofocus` does: it replaces a route's provisional first focus, and never a focus
  restored on pop.

### Fixed

- **`flui-widgets`**: outside macOS and iOS, Alt+Left and Alt+Right without Shift in a text field
  no longer move the caret by one character. The key goes on to the route, so Alt+Left goes Back
  from inside a field.
