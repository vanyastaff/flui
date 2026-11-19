### Added

- `SceneVersion::texture_damage` in `flui-layer`: the device footprint of every attached entry
  of a version that draws one of the given external textures, closed over what filters read.

### Fixed

- A window frame that carries a new external-texture image, or a texture's retirement, redraws
  the chunks that draw that texture even when its own patch damaged nothing there, so the image
  is shown on that frame instead of the next repaint of the `Texture` widget.
