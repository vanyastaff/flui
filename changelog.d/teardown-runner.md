### Added

- **The runner's test kit opens the main window.** On the headless platform every window of a
  `flui_app::testing::HeadlessRun` renders with a `StandInRenderer`, so a run opens the windows
  the application opens and runs their frames without a GPU. `HeadlessRunHandle::renderer`
  reads what a window presented and injects a lost device or failing rebuilds;
  `HeadlessRunHandle::wake_deadline` is the instant the loop is asked to wake at.
- **`flui_testing::DropLedger` and `DropSentinel`** (also `flui::testing`): a test plants a
  sentinel in each value it hands the framework and proves, with `live() == 0`, that every one
  was dropped (ADR-0261 §2).

### Changed

- `HeadlessRunHandle::run_owner_turn` also asks the exit question the turn asked the platform
  to ask again, as a desktop loop does before it waits: a last window closed from inside a frame
  now ends the run.
