### Added

- `SubtreeCull::chunks_missed` and `SubtreeCull::reaches_tested` in `flui-layer`: a cull skips a
  run of chunks under one parent a block of them at a time, and says how many reaches it tested;
  `BoundaryContent::commands` gives the commands a content's chunks hold in all.

### Changed

- A partial frame's raster walk no longer tests every chunk of a boundary it enters: on a grid of
  7 200 chunks in one boundary, the frame that changes one cell tests 172 reaches instead of
  7 203, and its raster CPU on lavapipe stays at 0.5–0.6 ms from 200 to 7 200 chunks.
