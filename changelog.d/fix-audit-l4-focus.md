### Fixed

- **Arrow keys and the D-pad bring focus in** (`flui-interaction`):
  `FocusManager::focus_in_direction` with nothing focused, or with a scope waiting for its first
  descendant, enters that scope (Down and Right at its first node, Up and Left at its last)
  instead of doing nothing, so a TV remote or arrow key reaches an app without autofocus.
- **A panicking focus listener no longer strands a closed route** (`flui-widgets`): when the
  control a route restores the focus to has a listener that panics, the navigator still makes
  the other routes unfocusable, tells its observers, removes the closed route's overlay entries
  and disposes it, then propagates the panic. One panicking observer or `dispose` no longer
  keeps the rest from running either.
- **Closing a dialog returns the focus to its opener under a nested navigator**
  (`flui-interaction`, `flui-widgets`): a nested navigator whose top changed while a dialog
  covered its page no longer takes the focus when the dialog closes. Reopening a scope's
  descendants to focus drops the first-focus intents recorded inside it while they were barred.
- **A panicking key-cancel handler no longer leaves window focus half updated**
  (`flui-runtime`): every window's focus flag and lifecycle (with its pointer cancellation) are
  updated before the panic propagates.
- **Assistive focus on a text field is shown** (`flui-widgets`): an assistive technology's
  Focus action on `EditableText` makes focus visible, as it does on any `Focus` control.
- **Enabling or disabling a `GestureDetector`'s tap keeps its child** (`flui-widgets`): setting
  or clearing `on_tap` (a `RawButton` toggling `on_press`) no longer rebuilds the detector's
  whole subtree, so the child's state, gestures in flight and semantics identities survive.
