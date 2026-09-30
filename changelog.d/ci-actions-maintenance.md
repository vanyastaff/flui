### Added

- `cargo xtask test --no-trybuild` runs the suite without the trybuild compile-fail suites, now
  their own nextest test group (`trybuild`) beside `nested-cargo`.

### Changed

- CI: `feature-matrix`, `miri`, `bench-compile` and `doc-test` run on main, nightly and the
  `full-ci` label instead of on every pull request.
