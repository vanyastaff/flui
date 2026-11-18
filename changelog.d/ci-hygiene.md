### Added

- `cargo xtask build-all-targets` links the workspace's examples and benches with the test
  suite's features, reusing a `cargo xtask test` build; `cargo xtask test --nested` runs only the
  nested-cargo group (trybuild suites, generated projects, facade consumers).

### Changed

- CI: every pull request that compiles runs the wide lane (`clippy`, `test` and `public-api` on
  Linux, in parallel; the nested and feature-gated suites, rustdoc, wasm32 and the platform
  typechecks run on main, ADR-0370); the scoped `fast` lane is gone. `miri`, `macos-ci` and
  `test-windows` now block.
