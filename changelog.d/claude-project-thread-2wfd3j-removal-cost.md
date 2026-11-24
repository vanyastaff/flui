### Changed

- Unmounting an element that owns no signal skips the signal graph's retirement bookkeeping.
  A 3000-row filter frame runs 2.4% fewer instructions in release.
