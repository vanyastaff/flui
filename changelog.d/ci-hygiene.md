### Added

- `cargo xtask build-all-targets` links the workspace's examples and benches with the test
  suite's features, reusing a `cargo xtask test` build; `cargo xtask test --nested` runs only the
  nested-cargo group (trybuild suites, generated projects, facade consumers).

### Changed

- CI: every pull request that compiles runs the wide lane (every Linux job over the whole
  workspace, in parallel); the scoped `fast` lane is gone, and `cargo xtask check-changed` keeps
  the scoped check locally. `miri`, `macos-ci` and `test-windows` now block.
