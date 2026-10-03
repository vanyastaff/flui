### Changed
- Remove native Windows/macOS and GPU validation jobs and the full-ci label/device-check workflows; use Linux-only CI for PRs, main, nightly and manual runs.
- Run native nested-cargo tests and trybuild suites in parallel CI jobs; expose `cargo xtask test --nested --nested-group native|trybuild`.
- Add `cargo xtask platform-test` and `cargo xtask cli-test` for native package suites.
