### Added

- `TextureRegistry::has_updates_for` in `flui-layer`: whether a retirement waits, or a texture the
  caller reports as drawn has a pending frame, answered without a walk when no frame is pending.
- `TextureRegistry::take_updates_into` in `flui-layer`: takes the pending updates into a kept
  buffer, folded as `TextureUpdates::merge` folds them.
- `SceneVersion::draws_texture` in `flui-layer`: whether an attached entry of the version draws a
  texture.
- `HeadlessSink::textures` and `LaidOut::sink` in `flui-testing`: the texture frames the frame
  presented last took from its window's registry, as a raster host takes them.

### Changed

- A texture frame no longer rebuilds or repaints the `Texture` widget: it wakes the runtime, and
  a frame that paints nothing publishes the version the window holds with that frame and no
  scene damage, so the raster side redraws the texture's box alone. Frames fed between two frames
  go out in one publication, and a frame with no texture frame waiting still publishes nothing.
- A texture frame is tree work the runner's gate counts (`UiRuntime::has_pending_tree_work`), so
  a frame fed while the previous one renders is not lost to the redraw mark that frame clears.
- A texture frame costs nothing while no attached entry draws the texture (an offstage subtree),
  or while the window refuses the version it holds (ADR-0334 §4); its newest frame waits in the
  registry. A version held for another target repaints for it instead of waiting.
- The headless host's sink takes each window's texture frames with the frames that window's scene
  publishes.

### Removed

- `TextureRegistry::listen`, `TextureRegistry::revision` and
  `TextureListener` from `flui-layer`, `RenderTexture::set_revision` from `flui-objects`, and
  `TextureState` from `flui-widgets`: `Texture` is a stateless widget.
