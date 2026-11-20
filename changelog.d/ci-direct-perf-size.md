### Removed

- `cargo xtask perf` and its `--check`, `--bless`, `--instructions`, `--size`, `--graph` and
  `--self-test` modes. flui-widgets' perf scenarios hold their baseline themselves:
  `FLUI_PERF_BASELINE=check` (or `bless`) with
  `cargo nextest run -p flui-widgets --test perf -E 'test(/^perf_/)'`. CI's `perf-gates` job runs
  the callgrind comparison of the `perf_scaling_` scenarios as a step, the `deps` job counts the
  facade's default graph with `cargo tree`, and the `size` job builds the generated projects and
  compares their sizes with `perf/size.toml`.
- The `size` job's comparison of each `main` push with the commit before it, and the cache it kept
  for that comparison. A binary fails past 2% over its `perf/size.toml` line and warns past 2%
  under it; a package count now has to equal its line.
