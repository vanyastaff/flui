### Changed
- Select GPU and native platform/CLI CI suites from affected code while retaining full main and extended coverage.
- Run native nested-cargo tests and trybuild suites in parallel CI jobs; expose `cargo xtask test --nested --nested-group native|trybuild`.
- Add `cargo xtask platform-test` and `cargo xtask cli-test` for native package suites.
