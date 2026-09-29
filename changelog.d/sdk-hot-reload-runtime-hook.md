### Added

- `flui::view::dev_reload` (`DevReloadHook`, `ReloadEvent`, `ReloadWake`): the development-reload seam of ADR-0094 §1. The host attaches an installed hook once per event loop, polls it at every realm's frame boundary and applies a patch once to every realm; a hook that panics is dropped and the app keeps running.
- `AppConfig::with_dev_reload` and the `dev_reload` field, which install a hook on the application's configuration.
- `flui_hot_reload::WorkerReloadHook` (the host/worker dlopen reload, with its artifact watcher) and `flui_hot_reload::ScenePluginHook` (Android `flui run --scene` frames), the hooks a host installs.

### Changed

- The facade's `hot-reload` feature no longer changes `flui-app`; it only re-exports `flui-hot-reload` as `flui::hot_reload`. `flui-app` names no reload crate under any feature.
- An Android app that wants `flui run --scene` frames installs `ScenePluginHook` (`ScenePluginHook::device_library_path` gives the path the CLI pushes to); the runner no longer loads `libflui_scene.so` on its own.
- A worker's `request_rebuild` now reassembles every realm; before, only the most recently opened window's. A secondary window reloads only when it is opened with the application's configuration.

### Removed

- `flui-app`'s `hot-reload` feature and its optional dependency on `flui-hot-reload`.
- `AppConfig::worker_plugin_path` and `AppConfig::with_worker_plugin_path`. Migrate with `.with_dev_reload(flui::hot_reload::WorkerReloadHook::new(path))`.
