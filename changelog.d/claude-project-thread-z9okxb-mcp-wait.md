### Added

- **`flui mcp` locates and waits** (`flui-cli`): `flui_locate` finds the elements a locator
  (role, name, value, state, ancestor, descendant) matches, and `flui_wait` holds out until a
  condition holds in a window (an element appears, goes or meets an expectation, the window
  settles, frames present) or its timeout passes, so an agent waits for the app instead of
  reading in a loop. A wait's reply is awaited for as long as the wait asks the app to hold it.
