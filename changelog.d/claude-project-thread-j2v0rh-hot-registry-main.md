### Changed

- `flui_hot_reload::get_worker_build_ptr` looks a build up in the registry of the thread that
  loaded the worker, which is the thread that drives the host's frames; another thread finds none.
  The registry was process-wide.
- `WorkerReloadHook` loads its worker when the host attaches it, on the thread that runs the event
  loop, not when it is made, so a hook made on another thread still registers its builds where the
  host's views look them up.
