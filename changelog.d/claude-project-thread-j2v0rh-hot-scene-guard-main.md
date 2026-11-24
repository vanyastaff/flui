### Changed

- `flui_hot_reload::ScenePlugin::build_scene` and `HotReloadDriver::build_scene` return a
  `PluginScene<'_>` that borrows the plugin and reads as a `Scene`, so unloading the plugin or
  polling the driver while a scene lives no longer compiles. Both stay `unsafe` with one
  obligation left: no clone of a scene payload outlives the plugin.

### Removed

- `HotReloadDriver::build_scene_or`: it returned an owned scene the borrow above cannot hold. Match
  on `build_scene` and draw the fallback for `None`.
