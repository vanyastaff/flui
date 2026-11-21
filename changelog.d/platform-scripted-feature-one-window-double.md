### Added

- **`flui_platform::testing::MockWindow`** (under
  `flui-platform`'s `testing`): the one test double of `PlatformWindow`. Standalone, its
  refresh-rate request and image cursor answer as its capabilities say, and it takes input
  regions exactly when built `taking_input_regions`; `MockWindow::over(window)` is a
  `MockWindow<mock_window::Over>` whose state, id, commands and verbs are the wrapped window's,
  with no builders for its state. Both record redraws and the commands and input regions they
  took, run the test's code at each call they take through an observer (`WindowCall`), and keep
  values that drop with the window.
- **`flui-native`'s `scripted` feature**: names the headless host's type, window, doubles and test
  controls at the crate root; `testing` includes it, and `flui-app`'s `testing` turns it on.

### Changed

- **`flui-native`**: `HeadlessPlatform`, `HeadlessHostWindow`, `FakeTextInput`,
  `FakeAccessibility`, `DisplaySpec`, `DisplayChange`, `DisplayChangeError`, `UnknownDisplay` and
  the `Headless*` controls are public only with `scripted`, as are the headless platform's test
  methods on `HostPlatform`. Without it the headless host still runs (`FLUI_HEADLESS`,
  `headless_platform()`) and no public path names a test double. A crate whose tests use them
  enables `flui-native/scripted` on its dev-dependency.

### Removed

- **`flui_runtime::testing::TestWindow`**: use `flui_platform::testing::MockWindow`
  (`with_sizes(physical, logical)` is `with_size(width, height)` at the scale factor, and
  `redraw_calls_handle()` is `redraw_requests()`).
