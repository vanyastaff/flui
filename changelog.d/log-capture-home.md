### Added

- **`flui_log::testing`**: the deterministic `tracing` capture (`capture`,
  `CapturedLog`/`CapturedRecord`, `disarm_interest_cache`) lives in its subscriber's crate now.
  The kit needs only `tracing`, so every crate above the log plane can capture its own events
  without a testing-kit edge.

### Changed

- **`flui_testing::log_capture` re-exports `flui_log::testing`** (`flui-testing`): the import
  path keeps its name for the crates whose edges go through the testing kit. Its consumers
  (48 test files) migrate to `flui_log::testing` in a follow-up, which ends the bridge; the
  kit's other moves of this kind (`flui_platform::testing`'s capabilities and window sets)
  are already on main.