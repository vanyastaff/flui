### Added

- `SubtreeCull::chunks_missed` and `SubtreeCull::reaches_tested` in `flui-layer`: a cull skips a
  run of chunks under one parent, the boundary itself or one effect, a block of them at a time, and
  says how many reaches it tested; `BoundaryContent::commands` gives the commands a content's
  chunks hold in all.

### Changed

- A partial frame's raster walk no longer tests every chunk of a boundary it enters: the tests a
  run of chunks under one parent costs grow with the logarithm of its length. Chunks that each sit
  under an effect of their own are still tested one by one.
