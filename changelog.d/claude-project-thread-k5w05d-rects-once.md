### Changed

- A partial frame redraws each rect of its damage region in a pass of its own, under that rect's
  scissor, or the region's bounding box in one pass, whichever a single culled walk of the scene
  predicts is cheaper. Two changes in opposite corners of a window no longer redraw everything
  between them: on the `raster_share` bench (lavapipe, 1200², a 60 × 60 grid) such a frame redraws
  0.12 % of the surface in 4.2 ms of raster instead of 95 % in 90 ms. A few changes over one large
  drawing still take the box, which replays the drawing once instead of once per rect.
- A damage region merges two rects that share a whole side into one.
