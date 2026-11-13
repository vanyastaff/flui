### Added

- `flui::testing::Tester`, the application test API: `Tester::mount` mounts a view as `run_app`
  does, `flui::testing::find` builds the driver's `Locator`s over the semantics tree, `tap`,
  `long_press`, `drag` and `enter_text` act through the window's input path after the
  actionability checks, and `Tester::expect(locator).to_exist()` (`to_be_gone`, `to_have_count`,
  `to_have_name`, `to_have_value`, `to_be_checked`, `to_be_enabled`, `to_be_focused`) waits while
  the application has a frame or work due and fails at once, with the window's outline, when it is
  idle. Each action has a `try_` twin returning `TestError`.

### Changed

- `flui create`'s counter and widget templates test through `Tester`; the counter's test no longer
  wraps the app in a `MediaQuery` or computes tap coordinates.

### Deprecated

- `flui::testing`'s old harnesses moved under the deprecated `flui::testing::harness`:
  `HeadlessBinding`, `BuildCapabilities`, `MountOptions`, `MountOwners`, `Mounted`, `DropLedger`,
  `DropSentinel`, `widgets`, `storage`, `replay`, `a11y` and `render_snapshot`. Use `Tester`,
  `flui::testing::find` and `Tester::inspect_render`.
