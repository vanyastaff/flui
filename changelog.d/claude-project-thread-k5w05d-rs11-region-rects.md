### Added

- `flui_layer::DamageRect::try_enclosing`: the whole surface pixels a rect touches, clamped to
  the surface, without the anti-aliasing margin `try_covering` adds.

### Changed

- `Renderer::mark_dirty` keeps the rects it is given apart, up to `flui_layer::Region::MAX`, and a
  rect that is not finite repaints the whole frame. A partial frame still redraws their bounding
  box.
