### Added

- **`flui_testing::MockWindow`** (also `flui_platform::testing::MockWindow` under
  `flui-platform`'s `testing`): the one test double of `PlatformWindow`. Standalone, its
  refresh-rate request, image cursor and input regions answer as its capabilities say;
  `MockWindow::over(window)` is a `MockWindow<Wrapping>` whose state, id and commands are the
  wrapped window's, with no builders for them. Both record redraws, the commands they took and
  input regions, run the test's code at each call through an observer (`WindowCall`), and keep
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

- **`flui_runtime::testing::TestWindow`**: use `flui_testing::MockWindow`
  (`with_sizes(physical, logical)` is `with_size(width, height)` at the scale factor, and
  `redraw_calls_handle()` is `redraw_requests()`).
