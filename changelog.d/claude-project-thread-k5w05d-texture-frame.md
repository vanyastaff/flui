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
  while the window refuses the version it holds until the window shows it (ADR-0334 §4), or while
  the window's size or scale changed since that version, whose repaint carries the frame; its
  newest frame waits in the registry. A window whose raster executor died takes its texture frames
  into its slot's carry, so none keeps the window drawing.
- The headless host's sink takes each window's texture frames with the frames that window's scene
  publishes, a scene whose lineage a hot reload renewed in that pump included.

### Removed

- `TextureRegistry::listen`, `TextureRegistry::revision` and
  `TextureListener` from `flui-layer`, `RenderTexture::set_revision` from `flui-objects`, and
  `TextureState` from `flui-widgets`: `Texture` is a stateless widget.
