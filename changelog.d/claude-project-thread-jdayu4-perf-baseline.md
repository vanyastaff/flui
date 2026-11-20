### Changed

- The perf baseline is one file per scenario in `crates/flui-widgets/perf/baseline/`, so changes to different scenarios do not conflict. The check fails on any difference in a work counter, but a cost counter (`allocations`, `reallocations`, `allocated_bytes`, …) only has to stay at or under its baseline: a change that makes a scenario cheaper passes without touching the baseline, and a bless on `main` ratchets it.
