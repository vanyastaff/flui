### Changed

- `ClipPathLayer::new`, `PaintClip::Path` and `HitShape::Path` take a `Path` rather than an
  `Arc<Path>`: a `Path` clone already shares its built curves, so the outer `Arc` was a second
  allocation per clip on every paint.

### Fixed

- `Path::translate` of a rounded rectangle whose curves were already built (by a fill, a shadow
  or a hit test) records the move instead of copying the curves, so a card's clip moved into its
  layer allocates nothing.
