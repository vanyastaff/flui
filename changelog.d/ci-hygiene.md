### Changed

- CI: every pull request that compiles runs the wide lane (`clippy` and `test` on Linux, in
  parallel; every other check runs on main, ADR-0370, ADR-0375); the scoped `fast` lane is
  gone. `miri`, `macos-ci` and
  `test-windows` now block.
