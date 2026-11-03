### Fixed

- `Texture`: a new texture frame repaints only the texture instead of its enclosing repaint boundary, often the whole window, so the frame's damage is the texture's box. An enclosing boundary that cannot keep its output (one that holds a leader layer) still repaints every frame.

### Changed

- `Texture` is its own composited layer: a `HitSlop` beside a `Texture` no longer takes a tap that lands on the texture.
