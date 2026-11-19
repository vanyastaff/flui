### Added

- `SceneVersion::texture_damage` in `flui-layer`: the device footprint of every attached entry
  of a version that draws one of the given external textures, closed over what filters read,
  worked in a `TextureDamageScratch` the caller keeps between frames.
- `RasterBase::thresholded` in `flui-layer`: the full-frame threshold `damage_for` applies,
  for damage a raster side adds to a publication's.

### Fixed

- A window frame that carries a new external-texture image, or a texture's retirement, redraws
  the chunks that draw that texture even when its own patch damaged nothing there, so the image
  is shown on that frame instead of the next repaint of the `Texture` widget; a frame whose patch
  and texture users together cover more than the threshold draws in full.
