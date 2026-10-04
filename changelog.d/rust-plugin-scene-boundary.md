### Changed
- `HotReloadDriver::poll` reports image updates without constructing a scene. `DevReloadHook::scene_frame` now declares its unsafe plugin lifetime obligation and passes a font reset request that is acknowledged only after successful rendering.
- `Renderer::render_plugin_scene` resets font ownership and cached glyphs at image and scene-source transitions; ordinary frames also clear plugin font identity before reuse. Admitted font bytes are copied into concrete host ownership.
- Artifact stamps preserve native subsecond modification times and distinguish missing metadata. `worker_artifact_stamp` returns an optional `SystemTime`; the coarse `file_mtime` helper is removed.
### Fixed
- Unix plugin loading accepts native filenames that are not UTF-8.
