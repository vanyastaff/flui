### Added

- `Canvas::take_recording` ends the recording made so far, as `finish` does, and leaves the
  canvas empty at the identity transform for the next one; the last shader-free paints it
  interned stay interned, so a canvas that records many runs in turn allocates a repeated paint
  once.

### Changed

- A paint pass records every node into one recorder and one canvas, kept between frames, and
  collects a node's own effect layers into a buffer of the pass: a repaint of 1 000 leaves
  costs about a fifth fewer instructions and one allocation less per leaf.
- `Matrix4::transform_rect` maps an affine matrix's corners without the homogeneous division,
  with the same result.
