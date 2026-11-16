### Fixed

- A raster executor that dies while a render holds a slot settles the frame queued behind that
  render only after the render's own settlement, so a late `Presented` no longer arrives for a
  frame whose record the `Failed` settlement already folded (ADR-0305 §9).
