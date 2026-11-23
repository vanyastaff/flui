### Added

- `flui_testing::perf::{Baselines, Held, BLESS_COMMAND}`: one holder for every perf target's
  baseline files (ADR-0396). A work counter equals its file, a cost counter (`alloc` or `bytes`
  in its name) stays at or under it, and a file no scenario holds fails.

### Changed

- CI's `test` job checks the perf baseline files of flui-widgets, flui-material and
  flui-cupertino on every pull request (`FLUI_PERF_BASELINE=check`); `perf-gates` on main keeps
  only the callgrind comparison of the `perf_scaling_` scenarios. The files are blessed with the
  `test` job's features: `FLUI_PERF_BASELINE=bless CARGO_PROFILE_DEV_OPT_LEVEL=0 cargo nextest
  run --locked --no-fail-fast --workspace --exclude flui-native --features
  flui/material,flui/cupertino,flui/native --test perf`.

### Removed

- `flui_testing::control_cost::held_to_baseline`: the catalogs' perf targets hold their costs
  through `flui_testing::perf::Baselines`, which holds their element and render-node counts
  exactly instead of as ceilings.
