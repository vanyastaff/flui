### Fixed

- Run the complete xtask test suite and strict Clippy on Linux, Windows and
  macOS for changes to the host tool or its build inputs.
- Resolve test reachability from Cargo's manifest paths so linked checkouts,
  including macOS temporary directories, recognize declared tests and mounted
  modules.
