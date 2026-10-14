### Added

- **Pointer back button as the Back key** (`flui-interaction`, `flui-runtime`): the mouse's back
  button (X1) also reaches the focused widget as a `NamedKey::BrowserBack` press and release,
  through `FocusManager::dispatch_pointer_back`. It is held apart from a keyboard's own Back key
  and does not make focus visible.
- **Frame stages for owner work before paint** (`flui-view`):
  `WidgetsBinding::settle_layout_with_builders`, `run_settled_frame` and
  `rebuild_within_frame` split `run_frame_with_layout_builders` so a frame can act on its
  settled tree before painting it.

### Fixed

- **Focus stayed on a removed widget** (`flui-runtime`): removing the focused widget now moves
  focus to the next focusable widget in the same frame, in one focus change, instead of leaving
  it on the removed node until the next input.
- **Pointer presses and window deactivation reach focus** (`flui-runtime`): a pointer press
  hides the keyboard focus indicator, and a window that loses activation forgets its held keys,
  so their repeats and releases after it returns do nothing.
