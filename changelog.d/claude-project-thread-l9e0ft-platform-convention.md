### Added

- `flui_view::environment::PlatformConvention`: the one inherited value naming the platform whose
  conventions a subtree follows (word-jump and command chords, refresh and zoom keys, touch
  handles, long-press selection, the selection toolbar and magnifier, Material page transitions).
  The runtime installs it above every root from the platform its runner names; a plain wrapper
  overrides it for a subtree.
- `PresentationWindow::with_platform` and `HeadlessWindow::with_platform` name that platform; a
  headless test without one follows `TargetPlatform::Unknown`'s neutral conventions.
- `GestureCommand::command_key`: a key with the platform's command modifier (Cmd on macOS and
  iOS, Control elsewhere), resolved from the convention the row is built under.

### Changed

- Every widget that chose by platform reads `PlatformConvention::of` instead of the compile
  target; `TargetPlatform::current()` is a clippy `disallowed-methods` entry outside the app
  runner.
- `GestureCommand::keys` yields owned activators resolved for the platform.

### Removed

- `ThemeData::platform` and `ThemeDataOverrides::platform` in `flui-material`: page transitions
  follow the `PlatformConvention`.
- `Default` for `TargetPlatform`, `CaretHandle`, `LongPressSelection` and `SelectionMenuPolicy`,
  which read the build host: use `for_platform` with `PlatformConvention::of`.
