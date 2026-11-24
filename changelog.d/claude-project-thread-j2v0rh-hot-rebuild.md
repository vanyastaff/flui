### Fixed

- `flui_hot_reload::request_rebuild` called from a worker's code (a gesture handler of the
  `flui create --hot-reload` template or `hot_reload_counter`) now rebuilds the host's widgets. A
  worker `cdylib` links its own copy of `flui-hot-reload`, so the call found the worker image's
  own empty rebuild slot, logged a warning and did nothing. `flui_worker_init` now also receives
  the host's rebuild entry (`HostRequestRebuildFn`), and `hot_reload_worker!` forwards the
  worker's requests to it. The plugin ABI token changes with it, so a worker built before this
  release is refused at load and has to be rebuilt.
