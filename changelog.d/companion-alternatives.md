### Added

- **`flui-widgets`**: `Alternative::Companion(CompanionSpec)` backs a behavioural widget's
  commands with controls another widget already draws, such as a tab bar beside the pager it
  turns, page dots, a stepper or Previous and Next buttons. The behavioural widget declares its
  commands as usual (label, `Intent`, keys) and carries a `CompanionLink<I>`, made by whoever
  owns the state both widgets read. The companion calls `CompanionLink::register` while it is
  mounted, publishing for each command the `SemanticsAnchor` of the node that performs it, and
  the registration ends when it drops. Alternatives that no companion backs warn on
  `flui::a11y` once the frame they mounted in has run, and
  `GestureAlternativesError::ForeignIntent` refuses a command whose intent the link does not
  carry ([ADR-0216](/docs/adr/ADR-0216-gesture-alternatives.md)).
- **`flui-material`**: `TabController::step_alternatives` is the "Previous tab" and "Next tab"
  pair a `TabBarView` offers in place of a swipe, selecting the adjacent tab as tapping it does.
  The `TabBar` on the controller is their single-pointer path.
- **`flui-testing`**: `assert_gesture_alternatives` drives a companion's node with assistive
  technology and a mouse, a finger and a virtual pointer, and fails when no companion is
  registered or none has a node for a command; `LaidOut::a11y_nodes_named_by` finds the
  published nodes a `SemanticsAnchor` names.

### Changed

- **`flui-material`**: each `TabBar` tab is one `SemanticsRole::Tab` node with its selected
  state, inside a `SemanticsRole::TabList` node, as a `NavigationBar` destination is.
- **`flui-widgets`**: `Alternative` has a third variant, so an exhaustive `match` on it needs an
  arm for `Companion`.
