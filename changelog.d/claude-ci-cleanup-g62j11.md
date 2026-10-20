### Changed

- CI: `ci.yml` has four lanes; a push to `main`, the merge queue and a manual run take `full`,
  and the nightly run is gone. New RustSec advisories are checked daily by `advisories.yml`,
  which keeps a "RustSec advisory on main" issue open while a finding stands, and the "CI is red
  on main" issue now closes itself on the next green run of `main`.

### Removed

- CI: the `standalone` and `bench-compile` jobs, `cargo xtask bench-compile` (the `test` job's
  all-targets build links the benches), the `signals` run of `test-features` (the features are
  empty), the Linux leg of `xtask-native.yml` (`checks` and `clippy` cover it) and the weekly
  `bench` job.
