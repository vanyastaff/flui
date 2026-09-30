### Added

- `cargo xtask test --no-trybuild` runs the suite without the trybuild compile-fail suites, now
  their own nextest test group (`trybuild`) beside `nested-cargo`.

### Changed

- CI: a pull request runs `checks`, `clippy`, `cross-typecheck`, `test`, `test-nested`, `doc` and
  `wasm-check`; `feature-matrix`, `test-features`, `live-smoke`, `miri`, `bench-compile` and
  `doc-test` run on main, nightly and the `full-ci` label.
