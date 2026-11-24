### Changed

- `flui-hot-reload`'s surface is flat: `DynLib` and the worker types are imported from the crate
  root (`flui_hot_reload::DynLib`, `flui_hot_reload::WorkerPlugin`), the modules `dynlib`,
  `worker` and `engine` are private, and `strategy` is gone. The environment variable names live in one
  module, `flui_hot_reload::env` (`WORKER_PLUGIN`, `SCENE_PLUGIN`, `HOT_RELOAD`), in place of
  `engine::env` and `strategy::env`.

### Removed

- `flui_hot_reload::ReloadStrategy`, which nothing outside the crate used: `flui run`'s strategies
  are its own.
- `HotReloadDriver::{plugin_kind, plugin_version, reload_count}`, `WorkerReloadDriver::{is_loaded,
  reload_count, fingerprint, lib_path}`, `ScenePlugin::version` and `WorkerPlugin::version`, and
  the `flui_scene_version`, `flui_app_version` and `flui_worker_version` symbols the plugin macros
  exported: each answered 1, and a reload is detected by the artifact's revision.
