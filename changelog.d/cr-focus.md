### Added

- **`flui-interaction`**: `FocusNode::register_on_key_cancel` and `KeyCancelHandler`. A node on
  the path that took a key's press is told, with that key-down, when the key ends without its
  release reaching that path: focus moved to another traversal unit, or the window lost
  activation. `Scrollable` and `InteractiveViewer` end their keyboard sequence there
  ([ADR-0079](/docs/adr/ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md)).
- **`flui-testing`**: `assert_gesture_alternatives` performs each command's semantics action
  again under reduced motion, both the user's setting and an animator duration scale of 0, and
  fails a widget whose alternative does not reach the same end state within two frames
  ([ADR-0216](/docs/adr/ADR-0216-gesture-alternatives.md)). `LaidOut::set_system_preferences`
  applies a system-preference revision to a laid-out tree.

### Fixed

- **`flui-interaction`**: a key released after focus moved to another unit no longer reaches any
  key handler. Space held on one button and released after Tab or a route push activated the
  newly focused control, and the covered route's nodes heard the release.
- **`flui-runtime`**: when the one rebuild a frame runs after moving focus off a removed node
  removes the new focus too, the frame now asks for the next one, which moves focus on; focus
  used to stay on the removed node until something else requested a frame.
  `FocusManager::has_orphaned_focus` reports the pending move.
