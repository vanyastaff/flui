### Fixed

- A retained scene's damage and cull cover what a transform filter (a magnifier) draws from a
  source pixel its input covers only in part: the renderer reads that pixel whole, so content
  ending inside it showed in a partial frame where the scene had culled it. A transform filter
  that is not a uniform scale has no bound inside its boundary, whatever its sampling.
