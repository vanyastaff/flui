### Added

- **`flui_testing::widgets::StateCreations`**: a stateful leaf that counts its `create_state` calls,
  for tests that ask whether a subtree was rebuilt or an element reused.
- **`HeadlessBinding::pump_frame_report`**: `pump_frame` returning the frame's `FrameReport`.
