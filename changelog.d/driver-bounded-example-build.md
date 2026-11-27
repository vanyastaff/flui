### Added

- `Launch::build_wait` in `flui-driver`: the bound on the `cargo build` that
  `Launch::cargo_example` runs before it starts the app, 20 minutes when not named.

### Fixed

- A `Launch::cargo_example` build queued behind another cargo's lock no longer holds the launch
  for as long as the lock is held: past its wait the build is ended and the launch fails with
  `Error::Timeout` awaiting `Awaited::Host`. Cargo's error output on a failed build is in the
  error rather than on the test's stderr.
