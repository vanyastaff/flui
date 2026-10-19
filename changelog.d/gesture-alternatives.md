### Added

- **Gesture alternatives** (`flui-widgets`, ADR-0216): `GestureAlternatives` gives every gesture of
  a behavioural widget an `Intent` reached without the gesture: a custom action for assistive
  technology, its keys while focus is in the row, and one pointer through an
  `Alternative::VisibleControl` (`ControlSpec`) or an `Alternative::ContextMenu` (`MenuSpec`,
  opened by a secondary click, a touch long press composed as `Sequenced`, the menu keys,
  `ShowContextMenu` and a control revealed on keyboard focus). `GestureCommand` names each
  operation with a label and keys; `GestureAlternatives::handle` gives the widget's own gesture
  the same intent. Controls mirror in right-to-left locales and are drawn at least the minimum
  target; `WidgetsLocalizations::context_menu_button_label` names the menu control's target.
- **`ShowContextMenuIntent`** (`flui-widgets`): `DefaultFocusTraversal` binds the Menu key and
  Shift+F10 to it (Menu alone on Android, nothing on macOS and iOS);
  `ShowContextMenuIntent::default_keys` lists them per platform.
- **`SingleActivator::trigger` and `modifiers`** (`flui-widgets`): what a
  menu shows as a shortcut hint.
- **`assert_gesture_alternatives`** (`flui-testing`): performs each command of a subject through
  its gesture, assistive technology, its keys and its single-pointer path with a mouse, a finger
  and a virtual pointer, and fails unless every path reaches the same model effect and
  announcements, Escape changes nothing, and the controls meet the minimum target.
- **A closure where a gesture's intent belongs** no longer compiles with a bare trait error:
  `Intent` explains that a gesture performs an intent bound with `Actions`.

### Changed

- **`Dismissible::new` takes `GestureAlternatives`** (`flui-widgets`): a completed dismissal
  performs its first command before `on_dismissed`, and the alternatives reach the same command
  without the drag.

### Fixed

- **A tap recognizer joined contacts of buttons it reports nothing for** (`flui-interaction`): a
  `TapGestureRecognizer` with only secondary-click callbacks held a primary drag back; a press of
  a button with no callback of its family is no longer admitted.
