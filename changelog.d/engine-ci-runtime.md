### Fixed

- Clip-mask shaders avoid redundant fixed-loop expansion and unused membership branches, reducing first-use overhead on software GPU backends while preserving the clipping sample grid.
