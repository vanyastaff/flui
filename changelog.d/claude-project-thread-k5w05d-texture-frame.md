### Added

- `TextureRegistry::has_updates` in `flui-layer`: whether a texture frame or retirement waits
  for the raster host to take it.
- `HeadlessSink::textures` and `LaidOut::sink` in `flui-testing`: the texture frames the frame
  presented last took from the window's registry, as a raster host takes them.

### Changed

- A texture frame no longer rebuilds or repaints the `Texture` widget: it wakes the runtime, and
  a frame that paints nothing publishes the version the window holds with that frame and no
  scene damage, so the raster side redraws the texture's box alone. Frames fed between two frames
  go out in one publication, and a frame with no texture frame waiting still publishes nothing.
- The headless host's sink takes the primary window's texture frames with each frame it presents.

### Removed

- `TextureRegistry::listen`, `TextureRegistry::revision` and `TextureListener` from
  `flui-layer`, `RenderTexture::set_revision` from `flui-objects`, and `TextureState` from
  `flui-widgets`: `Texture` is a stateless widget.
