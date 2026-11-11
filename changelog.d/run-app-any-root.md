### Changed

- `run_app` and `run_app_android` take any `View` as the root, stateful included, as Flutter's
  `runApp` takes any widget. The `counter` example and `flui create`'s template mount
  `CounterView` directly and drop the `CounterApp` wrapper.
