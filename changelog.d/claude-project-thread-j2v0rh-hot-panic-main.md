### Fixed

- A panic in a hot-reload plugin's code no longer aborts the host application. The scene function
  of `scene_plugin!`, the root view, mount and frame of `app_plugin!` and the init of
  `hot_reload_worker!` run inside the plugin image's containment: the panic's text goes to stderr
  (logcat on Android), the frame is skipped, and the next frame builds again. A worker whose init
  panicked is refused at load with its registrations removed. `flui_worker_init` now returns
  `bool`, so a worker built before this release is refused and has to be rebuilt.
