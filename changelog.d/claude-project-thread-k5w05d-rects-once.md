### Changed

- A partial frame redraws each rect of its damage region in a pass of its own, under that rect's
  scissor, instead of the region's bounding box in one pass. Two changes in opposite corners of a
  window no longer redraw everything between them: on the `raster_share` bench (lavapipe, 1200²,
  a 60 × 60 grid) such a frame redraws 0.12 % of the surface in 4.2 ms of raster instead of 95 %
  in 90 ms.
