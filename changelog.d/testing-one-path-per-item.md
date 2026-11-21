### Changed

- `flui_testing` no longer re-exports its modules' items at the crate root: each has one path, its
  module (`flui_testing::host::HeadlessHost`, `flui_testing::replay::TouchScript`,
  `flui_testing::a11y::Action`). A double of `flui_platform::testing` that the kit's API hands out
  is re-exported beside that API (`flui_testing::host::{ScriptedHttp, ScriptedResponse,
  FakeHaptics}`), and the others (`MockWindow`, `WindowCall`, `mock_window`) are reached at
  `flui_platform::testing`. `PointerResampling` is reached at `flui_runtime::presentation`, and the
  log capture at `flui_testing::log_capture`. The agent protocol's `Locator`, `Match`, `Node`,
  `Role` and the other types `Tester` takes and returns are in `flui_testing::tester`.
  `flui::testing` is unchanged.
