### Changed

- `flui_testing` no longer re-exports its modules' items at the crate root: each has one path, its
  module (`flui_testing::host::HeadlessHost`, `flui_testing::replay::TouchScript`,
  `flui_testing::a11y::Action`). `MockWindow`, `ScriptedHttp`, `ScriptedResponse`, `WindowCall` and
  `mock_window` are reached as `flui_platform::testing::…`, `PointerResampling` as
  `flui_runtime::presentation::…`, and the log capture as `flui_testing::log_capture::…`. The agent
  protocol's `Locator`, `Match`, `Node`, `Role` and the other types `Tester` takes and returns are
  in `flui_testing::tester`. `flui::testing` is unchanged.
