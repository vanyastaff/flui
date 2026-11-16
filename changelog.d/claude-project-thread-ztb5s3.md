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
  `HeadlessBinding`, `BuildCapabilities`, `MountOptions`, `MountOwners`, `Mounted`, `widgets`,
  `storage`, `replay`, `a11y` and `render_snapshot`. Use `Tester`,
  `flui::testing::find` and `Tester::inspect_render`.

### Fixed

- A window hidden from `Resumed` passes through `Inactive` before `Hidden`, for its lifecycle
  subscribers and the scheduler's lifecycle listeners alike: a presentation now starts in the
  state its window snapshot gives instead of none, and the application's aggregate lifecycle
  walks the same ladder one state at a time (`Paused` back to `Resumed` climbs through `Hidden`
  and `Inactive`).
- A UI runtime starts in the lifecycle its window opens in: over a window that opens hidden,
  unfocused or paused, `UpdateScheduler::lifecycle_state` starts there (frames disabled when
  hidden or paused) instead of `Resumed`, and its listeners no longer hear a start-up change no
  window made. The headless host's `open_window` reports the new window as it opened, as a
  native host does, so a focused new window makes the first one `Inactive`.
- A window installed beside a focused one no longer leaves that one holding its keys: installing
  moves no focus, and the new window's snapshot takes it, so the window that loses it is told
  each held key was cancelled.
