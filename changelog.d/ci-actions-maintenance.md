### Added

- The trybuild compile-fail suites are their own nextest test group (`trybuild`) beside
  `nested-cargo`, so a run can leave them out with `-E 'not group(trybuild)'`.

### Changed

- CI: `feature-matrix`, `miri`, `bench-compile` and `doc-test` run on main, nightly and the
  `full-ci` label instead of on every pull request.
