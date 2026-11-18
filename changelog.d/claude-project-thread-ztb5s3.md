### Added

- The book has a tutorial: a first window with `run_app`, `Application` and `#[flui::main]`, a
  screen laid out with `Column`, `Row`, `Padding` and `Expanded`, state in a `StatefulView`, a
  text field and its `TextEditingController`, a lazily built `ListView` whose keyed rows keep
  their state, pages and a dialog through a typed `Router`, a Material theme, a `Tester` test of
  the screen, and a widget that follows `PlatformConvention`. Every sample compiles, and the
  ones that do not open a window run. `cargo run --example notes --features material` runs the
  application the tutorial builds, in one program.
- `flui::testing::Tester`, the application test API: `Tester::mount` mounts a view as `run_app`
  does, `flui::testing::find` builds the driver's `Locator`s over the semantics tree, `tap`,
  `long_press`, `drag` and `enter_text` act through the window's input path after the
  actionability checks, and `Tester::expect(locator).to_exist()` (`to_be_gone`, `to_have_count`,
  `to_have_name`, `to_have_value`, `to_be_checked`, `to_be_enabled`, `to_be_focused`) waits while
  the application has a frame or work due and fails at once, with the window's outline, when it is
  idle. Each action has a `try_` twin returning `TestError`.
- `Tester::draw_commands(&locator)` reads the draw commands the located element painted in the
  window's last submitted scene: its render object and every one below it, after the frames due
  now, in paint order and each placed in the window, so a test checks what one `CustomPainter` or
  render object drew beside others. A subtree larger than one inspection is read in several, and
  a frame between those reads starts the read again unless two in a row find the same render
  objects, as under a running animation. `try_draw_commands` refuses an authored
  region (`TestError::NoRenderObject`) and a render object with more children than one inspection
  lists (`TestError::TooManyChildren`) rather than return part of a drawing.

### Changed

- Every Rust sample in the book compiles against the `flui` facade: each chapter is a doctest of
  the facade (`src/book.rs`), the docs workflow runs them on every pull request that touches the
  book, and the concept, catalog and testing chapters show complete samples instead of excerpts.
- `flui create`'s counter and widget templates test through `Tester`; the counter's test no longer
  wraps the app in a `MediaQuery` or computes tap coordinates.

### Removed

- `flui::testing`'s old harnesses: `HeadlessBinding`, `BuildCapabilities`, `MountOptions`,
  `MountOwners`, `Mounted`, `GestureBinding`, `widgets`, `storage`, `replay`, `a11y` and
  `render_snapshot` are no longer reachable through the facade. Use `Tester`,
  `flui::testing::find`, `Tester::inspect_render` and `Tester::draw_commands`; render objects keep
  `flui::testing::rendering`.
- `flui_testing::BuildCapabilities` and `MountOptions::with_capabilities`: a headless bootstrap always
  installs every build capability.
- `flui_testing::HeadlessBinding` is removed (ADR-0083 §4), with `pump_frame`, `pump_presentation`,
  `pump_all`, the per-presentation clocks, the tree it mounted (`with_tree`, `bind_tree`,
  `mount_root` and the `bootstrap` module with `MountOptions`, `MountOwners` and `Mounted`), its
  scheduler, input pipeline and replay driver, and `a11y::NotTreeBound`. A test drives frames with
  `HeadlessHost::pump` or `widgets::lay_out` (a script with `HeadlessHost::run_script` or
  `LaidOut::replay`), runs presentations at independent cadences as separate hosts, and reaches a
  UI runtime's scheduler through `LaidOut::scheduler`.

### Fixed

- `HeadlessHost::pump` fails a frame whose semantics publication found an author error in any open
  window, as the widget harness's frames already did, so `Tester` and a bare host report it too.
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
- A panic in another window's code while a window changes state (its key-cancel handler, its
  gesture cancellation, an observer of its lifecycle) is reported against that window, and the
  change goes on: a window opening beside a focused one with a failing observer now opens
  instead of failing its installation. A failure of the window's own code, or of an application
  lifecycle listener, still fails the change.
