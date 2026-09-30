### Added

- `cargo xtask test --no-trybuild` runs the suite without the trybuild compile-fail suites, now
  their own nextest test group (`trybuild`) beside `nested-cargo`.
