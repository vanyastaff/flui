### Added

- **`flui_testing::MockWindow`** (also `flui_platform::testing::MockWindow` under
  `flui-platform`'s `testing`): the one test double of `PlatformWindow`, standalone or over a
  host's window, with builders for what a test sets, recordings of redraws, commands and input
  regions, an observer (`WindowCall`) that runs the test's code at each call, and values it keeps
  that drop with the window.
- **`flui-native`'s `scripted` feature**: names the headless host's type, window, doubles and test
  controls at the crate root; `testing` includes it, and `flui-app`'s `testing` turns it on.
- **`flui-app`'s `driver`** (and the facade's) now turns on the contract's scenario primitive,
  `flui_platform::capability::scripted`.

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
- **`flui_platform::window_takes_no_frame_verbs!`**: a window double is `MockWindow`, which
  answers both frame verbs itself.
