### Added

- `RenderBox::REPORTS_INPUT_REGIONS` / `RenderSliver::REPORTS_INPUT_REGIONS`: a render object that
  adds input regions declares it, so the region pass walks only the paths toward such nodes.
- `cargo xtask perf` records the allocations each scenario makes, and
  `cargo xtask perf --instructions [--check]` compares a frame's callgrind instruction count on a
  small and a large tree; CI runs both on `main` in the `perf-gates` job.

### Fixed

- The input-region pass no longer walks the whole render tree every frame: a label change over
  5 000 buttons took 2.2 ms through `HeadlessHost` and now takes 27 µs, as over 500.
