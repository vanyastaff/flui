### Added

- `flui_testing::perf::measure` and `Cost`: the window a perf scenario's steps run in, which
  returns their result with what they asked of the heap. `Baselines::hold_measured` holds a
  window's cost to the scenario's baseline file, and `Baselines::hold_scaling` holds one change on
  a small and a large tree and fails when the large one costs more.
- `FLUI_PERF_INSTRUCTIONS=callgrind`: each `hold_measured` counts its window's instructions by
  running its own test again under callgrind, and holds them to the file's `instructions` line,
  which may rise 3%. CI's `perf-gates` job checks those lines in release on `main`, and on a miss
  blesses them and uploads the files.

### Changed

- A `check` or `bless` run compares and rewrites only its own lines of a baseline file: a run
  without `FLUI_PERF_INSTRUCTIONS` the work and heap counters, a run with it the `instructions`
  line; each writes the other's lines back unchanged.
