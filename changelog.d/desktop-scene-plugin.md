### Added

- A development scene plugin installed with `AppConfig::dev_reload` (`flui_hot_reload::ScenePluginHook`)
  now draws its scene in place of the widget tree on Windows, macOS, Linux (X11) and iOS, in every
  window, as it did on Android; on every host a frame the plugin draws still serves the app's
  commands, tasks and input deadlines (ADR-0391). On Wayland the windows keep drawing their widget
  trees.
