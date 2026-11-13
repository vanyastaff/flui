### Changed

- One entry on every platform: `run_app(root)` takes only the root view, as Flutter's `runApp`.
  A configured run is `Application::new().config(..).capability::<C>(..).plugin(..).run(root)`;
  on the desktop `run_resident(factory)` starts without a window, with `on_ready` and
  `on_window_error` as before. `LaunchError` is the one error of every run: it gains `Platform` for
  an event-loop failure (the desktop panicked before), `InitialWindow`, `OnReadyPanicked`,
  `Service` and `NoAndroidEntry`.
- `#[flui::main]` marks the function an application starts from. On Android it writes the
  `android_main` the system calls and hands the run its `AndroidApp`; elsewhere it leaves the
  function as it is. `flui create` projects put it on `pub fn run()` in `src/lib.rs` and call it
  from `src/main.rs`.

### Removed

- `AppLaunch`, `AppCapabilities`, `AppRunError`, `StartupWindow`, `run_app_android` and the
  facade's `flui::android_activity`. `Application::new(factory)` becomes `Application::new()` with
  the root given to `run` or the factory to `run_resident`.
